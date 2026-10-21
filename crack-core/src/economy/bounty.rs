//! Bounties: a member escrows credits on another member's head, and whoever
//! next lands a prank on that member collects them. Placing, the claim rule,
//! and the words for the card and the list.

use super::cards::card_destination;
use super::settings::{ActionKind, ActionPrice};
use super::shop::{cant_afford, check_purchase, BuyError, BuyOutcome, PurchaseFacts, Refusal};
use super::target::{check_target, TargetFacts};
use super::wallet::{ActionPayload, LedgerKind, MoveError};
use crate::db::economy::{self as db, BountyStake};
use crate::messaging::format::{escape, CONTENT_MAX};
use crate::messaging::messages::{
    ECONOMY_BOUNTY_CARD_COLLECTED_PREFIX, ECONOMY_BOUNTY_CARD_COLLECTED_SUFFIX,
    ECONOMY_BOUNTY_CARD_HEAD, ECONOMY_BOUNTY_CARD_ON, ECONOMY_BOUNTY_CARD_PREFIX,
    ECONOMY_BOUNTY_CARD_TAIL, ECONOMY_BOUNTY_COLLECTED_PREFIX, ECONOMY_BOUNTY_COLLECTED_SUFFIX,
    ECONOMY_BOUNTY_LIST_ENDS, ECONOMY_BOUNTY_LIST_FROM, ECONOMY_BOUNTY_LIST_MORE_PREFIX,
    ECONOMY_BOUNTY_LIST_MORE_SUFFIX, ECONOMY_BOUNTY_LIST_NONE, ECONOMY_BOUNTY_LIST_TITLE,
    ECONOMY_BOUNTY_SIGN_ANON, ECONOMY_BOUNTY_SIGN_DASH,
};
use crate::Data;
use chrono::{DateTime, Duration, Utc};
use serenity::all::{ChannelId, GuildId, UserId};
use std::collections::BTreeMap;

/// How long a bounty stands before the sweeper refunds it.
pub const BOUNTY_DAYS: i64 = 7;

/// `/bounty list` shows at most this many targets.
pub const LIST_MAX_TARGETS: usize = 15;

/// What to place.
#[derive(Debug, Clone)]
pub struct BountyRequest {
    pub guild: GuildId,
    pub placer: UserId,
    pub target: UserId,
    pub amount: i64,
    pub note: Option<String>,
    pub signed: bool,
    pub now: DateTime<Utc>,
}

/// A bounty that stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub action_id: i64,
    pub balance: i64,
    pub until: DateTime<Utc>,
}

/// The stake as the `u32` price [`check_purchase`] reads. Zero or less reads
/// as 0 (the minimum check refuses it first); an amount over `u32::MAX`
/// clamps to it, which no wallet that reaches the charge can pay without the
/// charge itself refusing the true amount as can't-afford.
fn stake_price(amount: i64) -> u32 {
    u32::try_from(amount.max(0)).unwrap_or(u32::MAX)
}

/// Check a bounty, refusing for the first reason that applies, in order:
/// economy off, not for sale, buyer opted out, below the minimum stake,
/// can't afford, then the target (bot, self, opted out, immune).
///
/// `purchase.price` is the bounty's own price entry, whose number is the
/// minimum stake. A bounty already standing on the target is no reason to
/// refuse another, so `running_same_kind` is not read.
pub fn check_bounty(
    purchase: &PurchaseFacts,
    amount: i64,
    target: &TargetFacts,
) -> Result<(), Refusal> {
    let min = i64::from(purchase.price.price);
    // The switches and the buyer first: nothing about the amount yet.
    check_purchase(&PurchaseFacts {
        balance: i64::MAX,
        ..*purchase
    })?;
    if amount < min || amount <= 0 {
        return Err(Refusal::BountyTooSmall { min });
    }
    check_purchase(&PurchaseFacts {
        price: ActionPrice {
            price: stake_price(amount),
            on: purchase.price.on,
        },
        ..*purchase
    })?;
    let target = TargetFacts {
        running_same_kind: None,
        ..target.clone()
    };
    check_target(ActionKind::Bounty, &target, None).map(|_| ())
}

/// Place a bounty: check, then escrow the stake in one transaction, the row
/// `active` until [`BOUNTY_DAYS`] from now. `facts` describes the target.
/// The caller validates the note and posts the card and the mod log event.
pub async fn place(
    data: &Data,
    facts: TargetFacts,
    req: BountyRequest,
) -> Result<Placed, BuyError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let (g, placer) = (req.guild, req.placer);
    let settings = data.economy.settings(g);
    let balance = db::balance(pool, g, placer).await.map_err(|err| {
        tracing::warn!("economy: reading balance of {placer} in {g} failed: {err}");
        BuyError::Db
    })?;
    check_bounty(
        &PurchaseFacts {
            enabled: data.economy.is_enabled(g),
            price: settings.config.prices.get(ActionKind::Bounty),
            source_on: true,
            buyer_opted_out: data.economy.is_opted_out(g, placer),
            already_immune_indefinitely: false,
            balance,
        },
        req.amount,
        &facts,
    )
    .map_err(BuyError::Refused)?;
    let until = req.now + Duration::days(BOUNTY_DAYS);
    let charged = db::charge_active(
        pool,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Bounty,
            buyer: placer,
            target: Some(req.target),
            price: req.amount,
            note: req.note.as_deref(),
            signed: req.signed,
            payload: &ActionPayload::Bounty,
            expires_at: Some(until),
        },
        LedgerKind::BountyPlaced,
    )
    .await
    .map_err(|err| match err {
        // The balance moved between the check and the charge.
        MoveError::Insufficient { balance } => BuyError::Refused(cant_afford(req.amount, balance)),
        MoveError::Invalid => BuyError::Db,
        MoveError::Db(err) => {
            tracing::warn!("economy: escrowing a bounty from {placer} in {g} failed: {err}");
            BuyError::Db
        },
    })?;
    Ok(Placed {
        action_id: charged.action_id,
        balance: charged.balance,
        until,
    })
}

/// Whether a bought action collects the bounties on its target (ruling P2):
/// only a prank that landed. A bounced prank landed on the buyer, a blocked
/// one on nobody, and a purchase that is not a prank is no landing at all.
pub fn collects_bounty(kind: ActionKind, outcome: BuyOutcome) -> bool {
    kind.is_prank() && outcome == BuyOutcome::Landed
}

/// Pay `claimer` the bounties on `target` if the purchase collects them.
/// Returns what was paid, 0 when nothing was due or the purchase does not
/// collect.
pub async fn collect(
    pool: &sqlx::PgPool,
    guild: GuildId,
    target: UserId,
    claimer: UserId,
    kind: ActionKind,
    outcome: BuyOutcome,
) -> sqlx::Result<i64> {
    if !collects_bounty(kind, outcome) {
        return Ok(0);
    }
    db::claim_bounties(pool, guild, target, claimer).await
}

/// The line the claimer's private reply gains.
pub fn collected_reply_line(total: i64, currency: &str) -> String {
    format!(
        "{ECONOMY_BOUNTY_COLLECTED_PREFIX} {total} {currency} {ECONOMY_BOUNTY_COLLECTED_SUFFIX}"
    )
}

/// The line the prank's card gains. It does not name the claimer.
pub fn collected_card_line(target: UserId) -> String {
    format!(
        "{ECONOMY_BOUNTY_CARD_COLLECTED_PREFIX} <@{target}> {ECONOMY_BOUNTY_CARD_COLLECTED_SUFFIX}"
    )
}

/// The public card for a placed bounty. An unsigned placer reads "someone".
pub fn bounty_card_text(
    amount: i64,
    currency: &str,
    target: UserId,
    until: DateTime<Utc>,
    note: Option<&str>,
    placer: Option<UserId>,
) -> String {
    let mut text = format!(
        "{ECONOMY_BOUNTY_CARD_PREFIX} {amount} {currency} {ECONOMY_BOUNTY_CARD_ON} <@{target}>{ECONOMY_BOUNTY_CARD_HEAD} <t:{}:f>. {ECONOMY_BOUNTY_CARD_TAIL}",
        until.timestamp()
    );
    if let Some(note) = note.filter(|n| !n.trim().is_empty()) {
        text.push_str("\n> ");
        text.push_str(&escape(note));
    }
    let by = placer.map_or_else(
        || ECONOMY_BOUNTY_SIGN_ANON.to_string(),
        |p| format!("<@{p}>"),
    );
    text.push_str(&format!("\n{ECONOMY_BOUNTY_SIGN_DASH} {by}"));
    text
}

/// Where the card goes: the announce channel, else where the command ran.
pub fn bounty_card_channel(invoked_in: ChannelId, announce: Option<ChannelId>) -> ChannelId {
    card_destination(ActionKind::Bounty, None, invoked_in, announce)
}

/// One target's standing bounties, summed.
struct Heads {
    target: UserId,
    total: i64,
    soonest: DateTime<Utc>,
    /// Placers who signed, once each, in the order they first appear.
    signed: Vec<UserId>,
}

fn group(stakes: &[BountyStake]) -> Vec<Heads> {
    let mut by_target: BTreeMap<UserId, Heads> = BTreeMap::new();
    for s in stakes {
        let h = by_target.entry(s.target).or_insert(Heads {
            target: s.target,
            total: 0,
            soonest: s.expires_at,
            signed: Vec::new(),
        });
        h.total = h.total.saturating_add(s.amount);
        h.soonest = h.soonest.min(s.expires_at);
        if s.signed && !h.signed.contains(&s.placer) {
            h.signed.push(s.placer);
        }
    }
    let mut heads: Vec<Heads> = by_target.into_values().collect();
    // Largest first; ties by member id so the order is stable.
    heads.sort_by(|a, b| b.total.cmp(&a.total).then(a.target.cmp(&b.target)));
    heads
}

fn head_line(h: &Heads, currency: &str, emoji: &str) -> String {
    let mut line = format!(
        "<@{}>: {} {emoji} {currency} · {ECONOMY_BOUNTY_LIST_ENDS} <t:{}:f>",
        h.target,
        h.total,
        h.soonest.timestamp()
    );
    if !h.signed.is_empty() {
        let names: Vec<String> = h.signed.iter().map(|p| format!("<@{p}>")).collect();
        line.push_str(&format!(
            " · {ECONOMY_BOUNTY_LIST_FROM} {}",
            names.join(", ")
        ));
    }
    line
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `/bounty list`: each target with live bounties, the largest total first,
/// with the soonest end and the signed placers (an unsigned placer is never
/// named). At most [`LIST_MAX_TARGETS`] targets, then a line saying how many
/// more; and if even those are too long for Discord, fewer.
pub fn bounty_list_text(stakes: &[BountyStake], currency: &str, emoji: &str) -> String {
    let head = format!("**{ECONOMY_BOUNTY_LIST_TITLE}**");
    let heads = group(stakes);
    if heads.is_empty() {
        return format!("{head}\n{ECONOMY_BOUNTY_LIST_NONE}");
    }
    let lines: Vec<String> = heads
        .iter()
        .take(LIST_MAX_TARGETS)
        .map(|h| head_line(h, currency, emoji))
        .collect();
    for keep in (0..=lines.len()).rev() {
        let mut out = vec![head.clone()];
        out.extend(lines[..keep].iter().cloned());
        let left_out = heads.len() - keep;
        if left_out > 0 {
            out.push(format!(
                "{ECONOMY_BOUNTY_LIST_MORE_PREFIX} {left_out} {ECONOMY_BOUNTY_LIST_MORE_SUFFIX}"
            ));
        }
        let text = out.join("\n");
        if utf16_len(&text) <= CONTENT_MAX {
            return text;
        }
    }
    head
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::discord::fake::FakeEconomyDiscord;
    use crate::economy::immunity::Protection;
    use crate::economy::settings::EconomySettings;
    use crate::economy::shop::{buy, BuyRequest};
    use crate::DataInner;
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const A: UserId = UserId::new(10);
    const B: UserId = UserId::new(11);
    const C: UserId = UserId::new(12);
    const T: UserId = UserId::new(20);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// The moment this test run starts, on the real clock: the database
    /// judges a bounty's liveness by its own (`claim_bounties`,
    /// `live_bounty_stakes`), so a fixed date goes stale.
    fn t0() -> DateTime<Utc> {
        static START: std::sync::OnceLock<DateTime<Utc>> = std::sync::OnceLock::new();
        // Whole microseconds, as the database stores them.
        *START
            .get_or_init(|| DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap())
    }

    fn clean() -> TargetFacts {
        TargetFacts {
            is_bot: false,
            is_self: false,
            is_owner: false,
            top_role_position: Some(1),
            bot_top_role_position: Some(5),
            in_voice: None,
            server_mute: false,
            server_deaf: false,
            opted_out: false,
            protection: None,
            has_mirror: false,
            running_same_kind: None,
            can_join_destination: None,
            current_nick: None,
            display_name: "t".to_string(),
        }
    }

    fn facts() -> PurchaseFacts {
        PurchaseFacts {
            enabled: true,
            price: ActionPrice {
                price: 50,
                on: true,
            },
            source_on: true,
            buyer_opted_out: false,
            already_immune_indefinitely: false,
            balance: 1000,
        }
    }

    #[test]
    fn bounty_refusal_order() {
        // Everything wrong at once; fix one reason at a time, in order.
        let mut p = PurchaseFacts {
            enabled: false,
            price: ActionPrice {
                price: 50,
                on: false,
            },
            balance: 10,
            ..facts()
        };
        let mut t = TargetFacts {
            is_bot: true,
            is_self: true,
            opted_out: true,
            protection: Some(Protection::Indefinitely),
            ..clean()
        };
        let amount = 30;
        assert_eq!(check_bounty(&p, amount, &t), Err(Refusal::EconomyOff));
        p.enabled = true;
        assert_eq!(check_bounty(&p, amount, &t), Err(Refusal::NotForSale));
        p.price.on = true;
        assert_eq!(
            check_bounty(&p, amount, &t),
            Err(Refusal::BountyTooSmall { min: 50 })
        );
        // Below the minimum is said before "can't afford".
        let amount = 60;
        assert_eq!(
            check_bounty(&p, amount, &t),
            Err(Refusal::CantAfford { short: 50 })
        );
        p.balance = 60;
        assert_eq!(check_bounty(&p, amount, &t), Err(Refusal::TargetIsBot));
        t.is_bot = false;
        assert_eq!(check_bounty(&p, amount, &t), Err(Refusal::TargetIsSelf));
        t.is_self = false;
        assert_eq!(check_bounty(&p, amount, &t), Err(Refusal::TargetOptedOut));
        t.opted_out = false;
        assert_eq!(
            check_bounty(&p, amount, &t),
            Err(Refusal::TargetImmune(Protection::Indefinitely))
        );
        t.protection = None;
        assert_eq!(check_bounty(&p, amount, &t), Ok(()));
        // The exact minimum is fine; zero and negative never are.
        assert_eq!(check_bounty(&p, 50, &t), Ok(()));
        for bad in [0, -5, 49] {
            assert_eq!(
                check_bounty(&p, bad, &t),
                Err(Refusal::BountyTooSmall { min: 50 }),
                "{bad}"
            );
        }
        // A bounty already standing on them is no reason to refuse another.
        let busy = TargetFacts {
            running_same_kind: Some(t0()),
            ..t.clone()
        };
        assert_eq!(check_bounty(&p, amount, &busy), Ok(()));
        // No mirror, no reach check.
        let mirrored = TargetFacts {
            has_mirror: true,
            is_owner: true,
            top_role_position: Some(9),
            ..t
        };
        assert_eq!(check_bounty(&p, amount, &mirrored), Ok(()));
        // An amount no u32 holds is can't-afford, not a wrap into a cheap one.
        let huge = i64::from(u32::MAX) + 10;
        assert!(matches!(
            check_bounty(&p, huge, &mirrored),
            Err(Refusal::CantAfford { .. })
        ));
        assert_eq!(
            crate::economy::shop::refusal_text(&Refusal::BountyTooSmall { min: 50 }, "credits"),
            "A bounty starts at 50 credits."
        );
    }

    #[test]
    fn only_a_landed_prank_collects() {
        for kind in ActionKind::ALL {
            for outcome in [BuyOutcome::Landed, BuyOutcome::Bounced, BuyOutcome::Blocked] {
                assert_eq!(
                    collects_bounty(*kind, outcome),
                    kind.is_prank() && outcome == BuyOutcome::Landed,
                    "{kind:?} {outcome:?}"
                );
            }
        }
        assert!(collects_bounty(ActionKind::Rename, BuyOutcome::Landed));
        assert!(!collects_bounty(ActionKind::Rename, BuyOutcome::Bounced));
        assert!(!collects_bounty(ActionKind::Rename, BuyOutcome::Blocked));
        assert!(!collects_bounty(ActionKind::Mirror, BuyOutcome::Landed));
        assert!(!collects_bounty(ActionKind::Bounty, BuyOutcome::Landed));
    }

    #[test]
    fn bounty_card_text_names_the_placer_only_when_signed() {
        let until = t0();
        let ts = until.timestamp();
        let unsigned = bounty_card_text(75, "credits", T, until, None, None);
        assert_eq!(
            unsigned,
            format!(
                "💰 75 credits on <@20>'s head until <t:{ts}:f>. Land a prank on them to collect it.\n— someone"
            )
        );
        assert!(!unsigned.contains("<@10>"));
        let signed = bounty_card_text(75, "credits", T, until, Some("good luck"), Some(A));
        assert_eq!(
            signed,
            format!(
                "💰 75 credits on <@20>'s head until <t:{ts}:f>. Land a prank on them to collect it.\n> good luck\n— <@10>"
            )
        );
        // A note's markdown is escaped, as on a prank card.
        assert!(bounty_card_text(5, "c", T, until, Some("**x**"), None).contains("\\*\\*x\\*\\*"));
        assert_eq!(
            collected_card_line(T),
            "💰 The bounty on <@20> was collected."
        );
        assert_eq!(
            collected_reply_line(80, "credits"),
            "💰 You collected 80 credits of bounty."
        );
    }

    #[test]
    fn bounty_card_goes_to_announce_else_where_it_ran() {
        let here = ChannelId::new(3);
        let ann = ChannelId::new(4);
        assert_eq!(bounty_card_channel(here, Some(ann)), ann);
        assert_eq!(bounty_card_channel(here, None), here);
    }

    fn stake(target: UserId, placer: UserId, amount: i64, signed: bool, ends: i64) -> BountyStake {
        BountyStake {
            target,
            placer,
            amount,
            signed,
            expires_at: DateTime::from_timestamp(1_800_000_000 + ends, 0).unwrap(),
        }
    }

    #[test]
    fn bounty_list_text_groups_sorts_and_hides_unsigned_placers() {
        assert_eq!(
            bounty_list_text(&[], "credits", "🪙"),
            "**Bounties**\nNo bounties right now."
        );
        let list = bounty_list_text(
            &[
                stake(T, A, 30, true, 500),
                stake(UserId::new(21), C, 200, false, 100),
                stake(T, B, 40, false, 300),
                stake(T, A, 10, true, 900),
            ],
            "credits",
            "🪙",
        );
        let ts = |s: i64| 1_800_000_000 + s;
        assert_eq!(
            list,
            format!(
                "**Bounties**\n<@21>: 200 🪙 credits · ends <t:{}:f>\n<@20>: 80 🪙 credits · ends <t:{}:f> · from <@10>",
                ts(100),
                ts(300)
            )
        );
        // The unsigned placers are never named.
        assert!(!list.contains("<@11>") && !list.contains("<@12>"));
    }

    #[test]
    fn bounty_list_text_caps_targets_and_stays_under_discords_limit() {
        let many: Vec<BountyStake> = (0..20)
            .map(|i| stake(UserId::new(100 + i), A, 1000 - i as i64, false, 10))
            .collect();
        let list = bounty_list_text(&many, "credits", "🪙");
        assert_eq!(list.lines().count(), 1 + LIST_MAX_TARGETS + 1);
        assert!(list.ends_with("…and 5 more"), "{list}");
        // The largest total is first, the sixteenth target is not shown.
        assert!(list.lines().nth(1).unwrap().starts_with("<@100>"));
        assert!(!list.contains("<@115>"));
        // Long names push lines out: it still fits, and says how many left.
        let long = "x".repeat(60);
        let wide: Vec<BountyStake> = (0..15)
            .map(|i| stake(UserId::new(100 + i), A, 100 - i as i64, true, 10))
            .collect();
        let currency = long.repeat(2);
        let list = bounty_list_text(&wide, &currency, "🪙");
        assert!(list.encode_utf16().count() <= CONTENT_MAX, "{}", list.len());
        assert!(list.contains("more"), "{list}");
        let shown = list.lines().filter(|l| l.starts_with("<@")).count();
        let more: usize = list
            .lines()
            .last()
            .and_then(|l| l.trim_start_matches("…and ").split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert_eq!(shown + more, 15);
    }

    // ---- against the database ----

    macro_rules! db_test {
        ($name:ident, |$pool:ident: PgPool| $body:expr) => {
            #[sqlx::test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) {
                $body.await
            }
        };
    }

    async fn fund(pool: &PgPool, user: UserId, credits: i64) {
        db::credit_earnings(
            pool,
            G,
            &[db::EarnRow {
                user,
                voice: credits,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    async fn bounty_data(pool: &PgPool) -> Data {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        db::save_settings(pool, &s).await.unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data, G).await);
        data
    }

    fn request(placer: UserId, amount: i64) -> BountyRequest {
        BountyRequest {
            guild: G,
            placer,
            target: T,
            amount,
            note: Some("hi".into()),
            signed: true,
            now: t0(),
        }
    }

    async fn count(pool: &PgPool, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    db_test!(placing_a_bounty_escrows_it, |pool: PgPool| async {
        let data = bounty_data(&pool).await;
        fund(&pool, A, 500).await;
        let placed = place(&data, clean(), request(A, 120)).await.unwrap();
        assert_eq!(placed.balance, 380);
        assert_eq!(placed.until, t0() + Duration::days(7));
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 380);
        // The row stands, active, aimed at the target, ending in seven days.
        let rows = db::active_bounties(&pool, G).await.unwrap();
        let [r] = rows.as_slice() else {
            panic!("{rows:?}")
        };
        assert_eq!(r.id, placed.action_id);
        assert_eq!((r.buyer, r.target), (A, Some(T)));
        assert_eq!(r.expires_at, Some(t0() + Duration::days(7)));
        assert_eq!(r.note.as_deref(), Some("hi"));
        assert!(r.signed);
        assert_eq!(r.payload, ActionPayload::Bounty);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'bounty_placed' AND delta = -120"
            )
            .await,
            1
        );
        // A second one on the same head is fine; the list sees both.
        place(&data, clean(), request(A, 50)).await.unwrap();
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 330);
        // Refused: below the minimum, too poor, a protected target. Nothing moves.
        assert_eq!(
            place(&data, clean(), request(A, 49)).await,
            Err(BuyError::Refused(Refusal::BountyTooSmall { min: 50 }))
        );
        assert_eq!(
            place(&data, clean(), request(A, 331)).await,
            Err(BuyError::Refused(Refusal::CantAfford { short: 1 }))
        );
        let immune = TargetFacts {
            protection: Some(Protection::Indefinitely),
            ..clean()
        };
        assert_eq!(
            place(&data, immune, request(A, 60)).await,
            Err(BuyError::Refused(Refusal::TargetImmune(
                Protection::Indefinitely
            )))
        );
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 330);
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            2
        );
        // The stakes feed the list.
        let stakes = db::live_bounty_stakes(&pool, G).await.unwrap();
        assert_eq!(stakes.iter().map(|s| s.amount).sum::<i64>(), 170);
    });

    async fn mirror_for(pool: &PgPool, data: &Data, owner: UserId) {
        fund(pool, owner, 150).await;
        let r = BuyRequest {
            guild: G,
            buyer: owner,
            kind: ActionKind::Mirror,
            target: None,
            nick: None,
            note: None,
            signed: false,
            now: t0(),
        };
        buy(
            data,
            &FakeEconomyDiscord::default(),
            clean(),
            clean(),
            None,
            r,
        )
        .await
        .unwrap();
    }

    fn rename(by: UserId) -> BuyRequest {
        BuyRequest {
            guild: G,
            buyer: by,
            kind: ActionKind::Rename,
            target: Some(T),
            nick: Some("Bean".into()),
            note: None,
            signed: false,
            now: t0(),
        }
    }

    db_test!(
        a_landed_prank_collects_every_other_bounty,
        |pool: PgPool| async {
            let data = bounty_data(&pool).await;
            fund(&pool, A, 200).await;
            fund(&pool, B, 200).await;
            fund(&pool, C, 200).await;
            place(&data, clean(), request(A, 50)).await.unwrap();
            place(&data, clean(), request(B, 70)).await.unwrap();
            // C also has a bounty up; it stands when C collects.
            let own = place(&data, clean(), request(C, 60)).await.unwrap();
            let me = TargetFacts {
                is_self: true,
                ..clean()
            };
            let bought = buy(
                &data,
                &FakeEconomyDiscord::default(),
                clean(),
                me,
                None,
                rename(C),
            )
            .await
            .unwrap();
            assert_eq!(bought.outcome, BuyOutcome::Landed);
            let got = collect(&pool, G, T, C, ActionKind::Rename, bought.outcome)
                .await
                .unwrap();
            assert_eq!(got, 120);
            // C paid 60 + 120 (rename) and got 120 back.
            assert_eq!(
                db::balance(&pool, G, C).await.unwrap(),
                200 - 60 - 120 + 120
            );
            let left = db::active_bounties(&pool, G).await.unwrap();
            assert_eq!(
                left.iter().map(|r| r.id).collect::<Vec<_>>(),
                [own.action_id]
            );
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_ledger WHERE kind = 'bounty_won'"
                )
                .await,
                2
            );
            // Collected once: the same landing again pays nothing.
            assert_eq!(
                collect(&pool, G, T, C, ActionKind::Rename, BuyOutcome::Landed)
                    .await
                    .unwrap(),
                0
            );
        }
    );

    db_test!(a_bounced_prank_collects_no_bounty, |pool: PgPool| async {
        let data = bounty_data(&pool).await;
        fund(&pool, A, 200).await;
        fund(&pool, C, 200).await;
        place(&data, clean(), request(A, 50)).await.unwrap();
        // The target holds a mirror, so the rename bounces onto C.
        mirror_for(&pool, &data, T).await;
        let mirrored = TargetFacts {
            has_mirror: true,
            ..clean()
        };
        let me = TargetFacts {
            is_self: true,
            ..clean()
        };
        let bought = buy(
            &data,
            &FakeEconomyDiscord::default(),
            mirrored,
            me,
            None,
            rename(C),
        )
        .await
        .unwrap();
        assert_eq!(bought.outcome, BuyOutcome::Bounced);
        let got = collect(&pool, G, T, C, ActionKind::Rename, bought.outcome)
            .await
            .unwrap();
        assert_eq!(got, 0);
        // The bounty stands, unpaid.
        assert_eq!(db::active_bounties(&pool, G).await.unwrap().len(), 1);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'bounty_won'"
            )
            .await,
            0
        );
        // A blocked prank collects nothing either, though one is up for it.
        for outcome in [BuyOutcome::Blocked, BuyOutcome::Bounced] {
            assert_eq!(
                collect(&pool, G, T, C, ActionKind::Rename, outcome)
                    .await
                    .unwrap(),
                0
            );
        }
        assert_eq!(db::active_bounties(&pool, G).await.unwrap().len(), 1);
        // It was collectable all along: a landing takes it. Without this, a
        // bounty nobody could collect would pass every line above.
        assert_eq!(
            collect(&pool, G, T, C, ActionKind::Rename, BuyOutcome::Landed)
                .await
                .unwrap(),
            50
        );
    });
}
