//! Crowdfunds: members pool credits for a prank against a target. A filled
//! fund runs exactly like a bought prank, with the target checked again at
//! that moment; one that can't land refunds everyone. An unfilled fund is
//! refunded by the sweeper after [`FUND_HOURS`] (`expire_fund`).

use super::discord::{EconDiscordError, EconomyDiscord};
use super::settings::{validate_nick, ActionKind, ActionPrice, EconomyConfig};
use super::shop::{
    act, cant_afford, captured_restore, check_purchase, db_failed, payload_for, prank_expiry,
    refusal_text, running_name_pranks, BuyError, PurchaseFacts, Refusal,
};
use super::target::{check_target, check_target_with, TargetFacts, VoiceStep};
use super::wallet::{ActionPayload, MoveError};
use crate::db::economy::{self as db, ActionRow, FundError, OpenedFund, Pledged};
use crate::messaging::messages::{
    ECONOMY_FUND_WHY_DB, ECONOMY_FUND_WHY_LEFT, ECONOMY_FUND_WHY_NOT_IN_VOICE,
    ECONOMY_FUND_WHY_OTHER, ECONOMY_FUND_WHY_PERMISSIONS,
};
use crate::Data;
use chrono::{DateTime, Duration, Utc};
use serenity::all::{GuildId, UserId};

/// How long a fund has to fill before the sweeper refunds it.
pub const FUND_HOURS: i64 = 24;

/// `/crowdfund list` shows at most this many funds.
pub const LIST_MAX_FUNDS: usize = 15;

/// Check the target of a new fund, refusing for the first reason that
/// applies: [`check_target`]'s order, less two steps. The voice step is
/// left to the moment the fund fills ("re-checked at that moment"), and the
/// mirror never applies: a fund is never bounced (spec 4.2).
pub fn check_fund_start(kind: ActionKind, t: &TargetFacts) -> Result<(), Refusal> {
    // The one order, shared: with the voice step skipped, and no mirror so
    // there is no bounce.
    let no_mirror = TargetFacts {
        has_mirror: false,
        ..t.clone()
    };
    check_target_with(kind, &no_mirror, None, VoiceStep::Skip).map(|_| ())
}

/// What a pledge of `amount` takes, or why it is refused. `left` is what the
/// fund still needs; `mine` what this member has already put in, against a
/// per-member `max` (a fund takes two people: [`db::fund_share`]). More than
/// either allows is capped; nothing left of the member's share is refused.
/// Below the minimum is refused unless it covers all that the member may
/// still put in (a last top-up may be small).
pub fn check_pledge_amount(
    min: i64,
    left: i64,
    mine: i64,
    max: i64,
    amount: i64,
) -> Result<i64, Refusal> {
    if left <= 0 {
        return Err(Refusal::FundClosed);
    }
    let share_left = max - mine;
    if share_left <= 0 {
        return Err(Refusal::PledgeOverShare { max });
    }
    let room = left.min(share_left);
    if amount <= 0 || (amount < min && amount < room) {
        return Err(Refusal::PledgeTooSmall { min });
    }
    Ok(amount.min(room))
}

/// What to open.
#[derive(Debug, Clone)]
pub struct FundRequest {
    pub guild: GuildId,
    pub starter: UserId,
    pub target: UserId,
    pub kind: ActionKind,
    /// The new nickname, for a rename.
    pub nick: Option<String>,
    pub note: Option<String>,
    pub signed: bool,
    pub first_pledge: Option<i64>,
    /// The permission check for the kind, made by the caller (it needs the
    /// cache); applied after the target is checked.
    pub preflight: Result<(), Refusal>,
    pub now: DateTime<Utc>,
}

/// Open a fund, taking the starter's first pledge if there is one. Checks,
/// in order: the economy, `Crowdfund` and the kind are on; the starter hasn't
/// opted out; a rename has a valid name; the target ([`check_fund_start`]);
/// the preflight; the first pledge (at least the minimum, affordable). The
/// payload is the prank's, with nothing to restore yet: that is captured when
/// the fund fills. The caller validates the note and posts the card.
pub async fn start(
    data: &Data,
    facts: TargetFacts,
    req: FundRequest,
) -> Result<OpenedFund, BuyError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let (g, starter, kind) = (req.guild, req.starter, req.kind);
    if !ActionKind::FUNDABLE.contains(&kind) {
        return Err(BuyError::Invalid);
    }
    let settings = data.economy.settings(g);
    let cfg = &settings.config;
    let crowdfund = cfg.prices.get(ActionKind::Crowdfund);
    let price = cfg.prices.get(kind);
    check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price: ActionPrice {
            price: price.price,
            on: price.on && crowdfund.on,
        },
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(g, starter),
        already_immune_indefinitely: false,
        balance: i64::MAX,
    })
    .map_err(BuyError::Refused)?;
    let nick = match (kind, req.nick.as_deref()) {
        (ActionKind::Rename, Some(raw)) => Some(validate_nick(raw).map_err(|_| BuyError::Invalid)?),
        (ActionKind::Rename, None) => return Err(BuyError::Invalid),
        _ => None,
    };
    check_fund_start(kind, &facts).map_err(BuyError::Refused)?;
    if kind == ActionKind::Jail && cfg.jail_channel_id.is_none() {
        return Err(BuyError::Refused(Refusal::NoJailChannel));
    }
    req.preflight.map_err(BuyError::Refused)?;
    let amount = i64::from(price.price);
    let first = match req.first_pledge {
        None => None,
        Some(asked) => {
            let min = i64::from(crowdfund.price);
            let taken = check_pledge_amount(min, amount, 0, db::fund_share(amount), asked)
                .map_err(BuyError::Refused)?;
            let balance = db::balance(pool, g, starter)
                .await
                .map_err(|e| db_failed(&format!("reading balance of {starter} in {g}"), &e))?;
            if balance < taken {
                return Err(BuyError::Refused(cant_afford(taken, balance)));
            }
            Some((taken, req.signed))
        },
    };
    let payload = payload_for(kind, &facts, None, cfg, nick.as_deref(), None, None)?;
    db::open_fund(
        pool,
        &db::NewAction {
            guild: g,
            kind,
            buyer: starter,
            target: Some(req.target),
            price: amount,
            note: req.note.as_deref(),
            signed: req.signed,
            payload: &payload,
            expires_at: Some(req.now + Duration::hours(FUND_HOURS)),
        },
        first,
    )
    .await
    .map_err(|err| match err {
        FundError::AlreadyOpen => BuyError::Refused(Refusal::FundAlreadyOpen),
        // The balance moved between the check and the pledge.
        FundError::Money(MoveError::Insufficient { balance }) => {
            let wanted = first.map_or(0, |(a, _)| a);
            BuyError::Refused(cant_afford(wanted, balance))
        },
        FundError::Closed | FundError::OverShare { .. } | FundError::Money(_) => BuyError::Db,
        FundError::Db(e) => db_failed(&format!("opening a fund for {starter} in {g}"), &e),
    })
}

/// A pledge to make.
#[derive(Debug, Clone, Copy)]
pub struct PledgeRequest {
    pub guild: GuildId,
    pub pledger: UserId,
    pub fund: i64,
    pub amount: i64,
    pub signed: bool,
}

/// A pledge that was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PledgeDone {
    pub pledged: Pledged,
    /// The fund as it was when the pledge was made: its target and kind.
    pub fund: ActionRow,
}

/// Pledge to an open fund. Checks, in order: the economy and `Crowdfund` are
/// on; the pledger hasn't opted out; the fund is open in this server; it is
/// not aimed at the pledger; the amount (at least the minimum, unless it
/// covers what the fund still needs). When the pledge fills the fund,
/// `pledged.filled` is set and the caller must [`execute`] it at once.
pub async fn pledge(data: &Data, req: PledgeRequest) -> Result<PledgeDone, BuyError> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let (g, pledger) = (req.guild, req.pledger);
    let settings = data.economy.settings(g);
    let crowdfund = settings.config.prices.get(ActionKind::Crowdfund);
    check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price: crowdfund,
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(g, pledger),
        already_immune_indefinitely: false,
        balance: i64::MAX,
    })
    .map_err(BuyError::Refused)?;
    let view = db::fund_view(pool, g, req.fund, pledger)
        .await
        .map_err(|e| db_failed(&format!("reading fund {} in {g}", req.fund), &e))?;
    let Some(db::FundView {
        row: fund,
        price,
        pledged,
        mine,
    }) = view
    else {
        return Err(BuyError::Refused(Refusal::FundClosed));
    };
    if fund.target == Some(pledger) {
        return Err(BuyError::Refused(Refusal::PledgeOnSelf));
    }
    let min = i64::from(crowdfund.price);
    let wanted = check_pledge_amount(
        min,
        price.saturating_sub(pledged),
        mine,
        db::fund_share(price),
        req.amount,
    )
    .map_err(BuyError::Refused)?;
    let done = db::pledge(pool, req.fund, g, pledger, wanted, req.signed)
        .await
        .map_err(|err| match err {
            FundError::Closed => BuyError::Refused(Refusal::FundClosed),
            FundError::OverShare { max } => BuyError::Refused(Refusal::PledgeOverShare { max }),
            FundError::Money(MoveError::Insufficient { balance }) => {
                BuyError::Refused(cant_afford(wanted, balance))
            },
            FundError::AlreadyOpen | FundError::Money(_) => BuyError::Db,
            FundError::Db(e) => db_failed(&format!("pledging from {pledger} in {g}"), &e),
        })?;
    Ok(PledgeDone {
        pledged: done,
        fund,
    })
}

/// Why a filled fund did not land. Everyone was refunded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotLanded {
    Refused(Refusal),
    Discord(EconDiscordError),
    /// The database failed under us; the refund was still tried.
    Db,
}

/// A filled fund that landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub action_id: i64,
    pub until: Option<DateTime<Utc>>,
    /// The starter first, then each pledger once, in pledge order.
    pub contributors: Vec<UserId>,
    /// The starter if the fund is signed, plus each signed pledger once.
    pub signers: Vec<UserId>,
}

/// Give a filled fund's pledges back (`failed`), logging a refund that fails:
/// reconcile finds the row `pending` and refunds it.
async fn refund(pool: &sqlx::PgPool, id: i64, reason: &str) {
    match db::refund_fund(pool, id, reason).await {
        Ok(true) => {},
        Ok(false) => tracing::warn!("economy: fund {id} was no longer refundable"),
        Err(err) => tracing::error!("economy: refunding fund {id} failed: {err}"),
    }
}

/// What a filled fund is run against: where, the target as of now, and the
/// caller's permission check for the kind.
#[derive(Debug, Clone)]
pub struct FillFacts {
    pub guild: GuildId,
    pub target: TargetFacts,
    pub preflight: Result<(), Refusal>,
}

/// Run a filled fund, now `pending`, like a bought prank.
/// 1. [`check_target`] with no mirror (voice is checked here, as the fund
///    fills), then the preflight the caller made for the kind;
/// 2. rebuild the payload, capturing what to restore as a purchase does, and
///    its end; write both in one statement (a fill leaves the row without an
///    end, which reconcile reads as never prepared, never acted on);
/// 3. do it in Discord;
/// 4. make the row `active`.
///
/// Anything that stops it refunds every pledge. `facts` is the target as of
/// now; the target is never the pledger, so `is_self` is cleared. The caller
/// posts the card.
pub async fn execute(
    data: &Data,
    discord: &dyn EconomyDiscord,
    fill: FillFacts,
    fund: &ActionRow,
    cfg: &EconomyConfig,
    now: DateTime<Utc>,
) -> Result<Executed, NotLanded> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(NotLanded::Db);
    };
    let FillFacts {
        guild: g,
        target: facts,
        preflight,
    } = fill;
    let (id, kind) = (fund.id, fund.kind);
    let Some(target) = fund.target else {
        refund(pool, id, "failed").await;
        return Err(NotLanded::Db);
    };
    let facts = TargetFacts {
        is_self: false,
        has_mirror: false,
        ..facts
    };
    // The switch for the prank itself is read again: turning it off stops a
    // fund that has not landed yet.
    let on = if cfg.prices.get(kind).on {
        Ok(())
    } else {
        Err(Refusal::NotForSale)
    };
    let refused = on
        .and_then(|()| check_target(kind, &facts, None).map(|_| ()))
        .and_then(|()| {
            if kind == ActionKind::Jail && cfg.jail_channel_id.is_none() {
                Err(Refusal::NoJailChannel)
            } else {
                Ok(())
            }
        })
        .and(preflight);
    if let Err(r) = refused {
        refund(pool, id, "refused").await;
        return Err(NotLanded::Refused(r));
    }
    let running = match running_name_pranks(pool, g, target, kind).await {
        Ok(r) => r,
        Err(_) => {
            refund(pool, id, "failed").await;
            return Err(NotLanded::Db);
        },
    };
    let nick = match &fund.payload {
        ActionPayload::Rename { name, .. } => Some(name.as_str()),
        _ => None,
    };
    let restore = captured_restore(&running, &facts.current_nick);
    let payload = match payload_for(
        kind,
        &facts,
        None,
        cfg,
        nick,
        restore,
        facts.current_nick.clone(),
    ) {
        Ok(p) => p,
        Err(_) => {
            refund(pool, id, "failed").await;
            return Err(NotLanded::Db);
        },
    };
    let expires = prank_expiry(kind, &running, now, cfg);
    // Read before acting: a failed read must not leave an effect behind.
    let pledges = match db::fund_pledges(pool, id).await {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!("economy: reading the pledges of fund {id} failed: {err}");
            refund(pool, id, "failed").await;
            return Err(NotLanded::Db);
        },
    };
    match db::prepare_fund(pool, id, &payload, expires).await {
        Ok(true) => {},
        Ok(false) => {
            tracing::warn!("economy: fund {id} was no longer pending at execution");
            return Err(NotLanded::Db);
        },
        Err(err) => {
            tracing::error!("economy: preparing fund {id} failed: {err}");
            refund(pool, id, "failed").await;
            return Err(NotLanded::Db);
        },
    }
    if let Err(err) = act(discord, g, target, kind, &payload).await {
        tracing::warn!(
            "economy: fund {id}: {kind:?} on {target} in {g} failed in Discord: {err:?}"
        );
        let reason = if err == EconDiscordError::UnknownMember {
            "left"
        } else {
            "failed"
        };
        refund(pool, id, reason).await;
        return Err(NotLanded::Discord(err));
    }
    // The effect has landed. If this write fails the row stays `pending`,
    // prepared, and reconcile resolves it; the contributors still get their card.
    match db::activate_action(pool, id, kind, expires).await {
        Ok(true) => {},
        Ok(false) => tracing::warn!("economy: fund {id} was no longer pending at activation"),
        Err(err) => tracing::error!("economy: activating fund {id} failed: {err}"),
    }
    let mut contributors = vec![fund.buyer];
    let mut signers = if fund.signed {
        vec![fund.buyer]
    } else {
        Vec::new()
    };
    for p in &pledges {
        if !contributors.contains(&p.user) {
            contributors.push(p.user);
        }
        if p.signed && !signers.contains(&p.user) {
            signers.push(p.user);
        }
    }
    Ok(Executed {
        action_id: id,
        until: expires,
        contributors,
        signers,
    })
}

/// Why a fund could not land, in a few words for the card.
pub fn not_landed_reason(why: &NotLanded, currency: &str) -> String {
    match why {
        NotLanded::Refused(Refusal::TargetNotInVoice) => ECONOMY_FUND_WHY_NOT_IN_VOICE.to_string(),
        NotLanded::Refused(r) => refusal_text(r, currency).trim_end_matches('.').to_string(),
        NotLanded::Discord(EconDiscordError::MissingPermissions) => {
            ECONOMY_FUND_WHY_PERMISSIONS.to_string()
        },
        NotLanded::Discord(EconDiscordError::NotConnected) => {
            ECONOMY_FUND_WHY_NOT_IN_VOICE.to_string()
        },
        NotLanded::Discord(EconDiscordError::UnknownMember) => ECONOMY_FUND_WHY_LEFT.to_string(),
        NotLanded::Discord(EconDiscordError::Other(_)) => ECONOMY_FUND_WHY_OTHER.to_string(),
        NotLanded::Db => ECONOMY_FUND_WHY_DB.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::discord::fake::{Call, CallKind, FakeEconomyDiscord};
    use crate::economy::effects::sweep;
    use crate::economy::immunity::Protection;
    use crate::economy::settings::EconomySettings;
    use crate::DataInner;
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const A: UserId = UserId::new(10);
    const B: UserId = UserId::new(11);
    const C: UserId = UserId::new(12);
    const T: UserId = UserId::new(20);
    const VC: serenity::all::ChannelId = serenity::all::ChannelId::new(5);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// The moment this test run starts, on the real clock: the database
    /// judges a fund's liveness by its own, so a fixed date goes stale.
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
            in_voice: Some(VC),
            server_mute: false,
            server_deaf: false,
            opted_out: false,
            protection: None,
            has_mirror: false,
            running_same_kind: None,
            can_join_destination: None,
            current_nick: Some("Orig".to_string()),
            display_name: "orig".to_string(),
        }
    }

    #[test]
    fn fund_start_refusal_order() {
        // Everything wrong at once; fix one reason at a time, in order.
        let mut t = TargetFacts {
            is_bot: true,
            is_self: true,
            is_owner: true,
            top_role_position: Some(9),
            server_mute: true,
            opted_out: true,
            protection: Some(Protection::Indefinitely),
            running_same_kind: Some(t0()),
            in_voice: None,
            ..clean()
        };
        let k = ActionKind::Mute;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::TargetIsBot));
        t.is_bot = false;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::TargetIsSelf));
        t.is_self = false;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::OutOfReach));
        t.is_owner = false;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::OutOfReach));
        t.top_role_position = None;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::CantTell));
        t.top_role_position = Some(1);
        // Not being in voice is no reason at the start, even for a mute:
        // that is looked at again when the fund fills.
        t.running_same_kind = None;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::AlreadyMuted));
        t.server_mute = false;
        assert_eq!(check_fund_start(k, &t), Err(Refusal::TargetOptedOut));
        t.opted_out = false;
        // A mirror is no bounce for a fund: it is read past, so immunity is next.
        t.has_mirror = true;
        assert_eq!(
            check_fund_start(k, &t),
            Err(Refusal::TargetImmune(Protection::Indefinitely))
        );
        t.protection = None;
        t.running_same_kind = Some(t0());
        assert_eq!(
            check_fund_start(k, &t),
            Err(Refusal::AlreadyRunning { until: t0() })
        );
        t.running_same_kind = None;
        assert_eq!(check_fund_start(k, &t), Ok(()));
        // Every fundable prank passes for a clean target out of voice.
        for kind in ActionKind::FUNDABLE {
            let out_of_voice = TargetFacts {
                in_voice: None,
                ..clean()
            };
            assert_eq!(check_fund_start(*kind, &out_of_voice), Ok(()), "{kind:?}");
        }
    }

    #[test]
    fn a_pledge_below_the_minimum_must_cover_what_is_left() {
        // min 10, a share of 90 so far untouched.
        let check = |left, amount| check_pledge_amount(10, left, 0, 90, amount);
        assert_eq!(check(50, 10), Ok(10));
        for bad in [9, 0, -4] {
            assert_eq!(
                check(50, bad),
                Err(Refusal::PledgeTooSmall { min: 10 }),
                "{bad}"
            );
        }
        // Exactly what is left may be small; more than that is capped.
        assert_eq!(check(4, 4), Ok(4));
        assert_eq!(check(4, 8), Ok(4));
        assert_eq!(check(4, 3), Err(Refusal::PledgeTooSmall { min: 10 }));
        assert_eq!(check(50, 500), Ok(50));
        // Nothing left: closed.
        assert_eq!(check(0, 10), Err(Refusal::FundClosed));
    }

    #[test]
    fn a_pledge_is_capped_by_the_members_share() {
        // A fund of 90: 45 each. 30 already put in leaves 15.
        let check = |mine, amount| check_pledge_amount(10, 60, mine, 45, amount);
        assert_eq!(check(0, 45), Ok(45));
        assert_eq!(check(0, 500), Ok(45));
        assert_eq!(check(30, 40), Ok(15));
        // What is left of the share may be below the minimum when it is all
        // the member can put in; less than that is too small.
        assert_eq!(check(40, 5), Ok(5));
        assert_eq!(check(40, 4), Err(Refusal::PledgeTooSmall { min: 10 }));
        // Nothing left of it: over share, whatever the fund needs.
        assert_eq!(check(45, 10), Err(Refusal::PledgeOverShare { max: 45 }));
        assert_eq!(check(50, 10), Err(Refusal::PledgeOverShare { max: 45 }));
        // A closed fund is closed first.
        assert_eq!(
            check_pledge_amount(10, 0, 45, 45, 10),
            Err(Refusal::FundClosed)
        );
    }

    #[test]
    fn why_a_fund_did_not_land_is_in_words() {
        let r = |w| not_landed_reason(&w, "credits");
        assert_eq!(
            r(NotLanded::Refused(Refusal::TargetNotInVoice)),
            "they aren't in voice"
        );
        assert_eq!(
            r(NotLanded::Refused(Refusal::TargetImmune(
                Protection::Indefinitely
            ))),
            "They're immune until a moderator lifts it"
        );
        assert_eq!(
            r(NotLanded::Discord(EconDiscordError::MissingPermissions)),
            "CrackTunes lacks the permission"
        );
        assert_eq!(
            r(NotLanded::Discord(EconDiscordError::NotConnected)),
            "they aren't in voice"
        );
        assert_eq!(
            r(NotLanded::Discord(EconDiscordError::UnknownMember)),
            "they left the server"
        );
        assert_eq!(
            r(NotLanded::Discord(EconDiscordError::Other("x".into()))),
            "Discord refused"
        );
        assert_eq!(r(NotLanded::Db), "something went wrong");
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

    async fn fund_wallet(pool: &PgPool, user: UserId, credits: i64) {
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

    async fn fund_data(pool: &PgPool, tweak: impl FnOnce(&mut EconomySettings)) -> Data {
        let mut s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        tweak(&mut s);
        db::save_settings(pool, &s).await.unwrap();
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data, G).await);
        data
    }

    fn fund_request(starter: UserId, kind: ActionKind, first: Option<i64>) -> FundRequest {
        FundRequest {
            guild: G,
            starter,
            target: T,
            kind,
            nick: (kind == ActionKind::Rename).then(|| "Bean".to_string()),
            note: Some("gg".into()),
            signed: true,
            first_pledge: first,
            preflight: Ok(()),
            now: t0(),
        }
    }

    fn pledge_request(pledger: UserId, fund: i64, amount: i64, signed: bool) -> PledgeRequest {
        PledgeRequest {
            guild: G,
            pledger,
            fund,
            amount,
            signed,
        }
    }

    async fn count(pool: &PgPool, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

    async fn row_of(pool: &PgPool, id: i64) -> ActionRow {
        db::action_by_id(pool, G, id).await.unwrap().unwrap()
    }

    /// A mute fund (90) filled by A (45, signed) and B (45, unsigned), each
    /// at their share; returns the filled row.
    async fn filled_mute(pool: &PgPool, data: &Data) -> ActionRow {
        fund_wallet(pool, A, 100).await;
        fund_wallet(pool, B, 100).await;
        let f = start(data, clean(), fund_request(A, ActionKind::Mute, Some(45)))
            .await
            .unwrap();
        let p = pledge(data, pledge_request(B, f.action_id, 45, false))
            .await
            .unwrap();
        assert!(p.pledged.filled);
        row_of(pool, f.action_id).await
    }

    async fn run(
        data: &Data,
        fake: &FakeEconomyDiscord,
        facts: TargetFacts,
        row: &ActionRow,
    ) -> Result<Executed, NotLanded> {
        let cfg = data.economy.settings(G).config;
        execute(
            data,
            fake,
            FillFacts {
                guild: G,
                target: facts,
                preflight: Ok(()),
            },
            row,
            &cfg,
            t0(),
        )
        .await
    }

    db_test!(
        a_filled_fund_lands_and_runs_like_a_prank,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 100).await;
            fund_wallet(&pool, B, 100).await;
            fund_wallet(&pool, C, 100).await;
            // Rename costs 120: A opens with 50 (signed), B adds 40 unsigned,
            // C's 30 fills it (signed).
            let opened = start(
                &data,
                clean(),
                fund_request(A, ActionKind::Rename, Some(50)),
            )
            .await
            .unwrap();
            assert_eq!((opened.pledged, opened.balance), (50, Some(50)));
            let id = opened.action_id;
            // Still funding: its payload keeps the name and no restore yet.
            let row = row_of(&pool, id).await;
            assert_eq!(row.state, crate::economy::wallet::ActionState::Funding);
            assert_eq!(row.expires_at, Some(t0() + Duration::hours(24)));
            assert_eq!(
                row.payload,
                ActionPayload::Rename {
                    name: "Bean".into(),
                    restore: None
                }
            );
            let b = pledge(&data, pledge_request(B, id, 40, false))
                .await
                .unwrap();
            assert!(!b.pledged.filled);
            let c = pledge(&data, pledge_request(C, id, 100, true))
                .await
                .unwrap();
            // Capped at what was left.
            assert_eq!((c.pledged.taken, c.pledged.filled), (30, true));
            let row = row_of(&pool, id).await;
            let fake = FakeEconomyDiscord::default();
            let done = run(&data, &fake, clean(), &row).await.unwrap();
            // Discord got exactly the prank's call, with the shop's audit kind.
            assert_eq!(
                fake.calls(),
                vec![Call::Nick(T, Some("Bean".into()), ActionKind::Rename)]
            );
            assert_eq!(done.action_id, id);
            assert_eq!(done.until, Some(t0() + Duration::hours(24)));
            assert_eq!(done.contributors, vec![A, B, C]);
            assert_eq!(done.signers, vec![A, C]);
            // The row is active, ends in a day, and remembers the old name.
            let row = row_of(&pool, id).await;
            assert_eq!(row.state, crate::economy::wallet::ActionState::Active);
            assert_eq!(row.expires_at, Some(t0() + Duration::hours(24)));
            assert_eq!(
                row.payload,
                ActionPayload::Rename {
                    name: "Bean".into(),
                    restore: Some("Orig".into())
                }
            );
            // Nobody was refunded: 120 left their wallets between them.
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_ledger WHERE kind = 'pledge_refund'"
                )
                .await,
                0
            );
            // A day later the sweeper puts the old name back.
            sweep(&data, &fake, t0() + Duration::hours(25), &|_| true).await;
            assert_eq!(
                fake.calls().last(),
                Some(&Call::Nick(T, Some("Orig".into()), ActionKind::Rename))
            );
            assert_eq!(
                row_of(&pool, id).await.state,
                crate::economy::wallet::ActionState::Ended
            );
        }
    );

    async fn assert_everyone_refunded(pool: &PgPool, id: i64, reason: &str) {
        assert_eq!(db::balance(pool, G, A).await.unwrap(), 100);
        assert_eq!(db::balance(pool, G, B).await.unwrap(), 100);
        assert_eq!(
            count(
                pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'pledge_refund'"
            )
            .await,
            2
        );
        let row = row_of(pool, id).await;
        assert_eq!(row.state, crate::economy::wallet::ActionState::Failed);
        let why: Option<String> =
            sqlx::query_scalar("SELECT end_reason FROM economy_actions WHERE id = $1")
                .bind(id)
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(why.as_deref(), Some(reason));
    }

    db_test!(
        a_filled_fund_whose_target_is_immune_refunds_everyone,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            // They bought immunity while it filled.
            let immune = TargetFacts {
                protection: Some(Protection::Indefinitely),
                ..clean()
            };
            assert_eq!(
                run(&data, &fake, immune, &row).await,
                Err(NotLanded::Refused(Refusal::TargetImmune(
                    Protection::Indefinitely
                )))
            );
            assert!(fake.calls().is_empty());
            assert_everyone_refunded(&pool, row.id, "refused").await;
        }
    );

    db_test!(
        a_filled_fund_whose_target_is_not_in_voice_refunds_everyone,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            let away = TargetFacts {
                in_voice: None,
                ..clean()
            };
            assert_eq!(
                run(&data, &fake, away, &row).await,
                Err(NotLanded::Refused(Refusal::TargetNotInVoice))
            );
            assert!(fake.calls().is_empty());
            assert_everyone_refunded(&pool, row.id, "refused").await;
        }
    );

    db_test!(
        a_failed_preflight_at_fill_refunds_everyone,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            let cfg = data.economy.settings(G).config;
            let gap = Refusal::Unavailable {
                missing: serenity::all::Permissions::MUTE_MEMBERS,
            };
            assert_eq!(
                execute(
                    &data,
                    &fake,
                    FillFacts {
                        guild: G,
                        target: clean(),
                        preflight: Err(gap)
                    },
                    &row,
                    &cfg,
                    t0()
                )
                .await,
                Err(NotLanded::Refused(gap))
            );
            assert!(fake.calls().is_empty());
            assert_everyone_refunded(&pool, row.id, "refused").await;
        }
    );

    db_test!(
        a_discord_failure_at_fill_refunds_everyone,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            fake.fail_next(CallKind::Mute, EconDiscordError::MissingPermissions);
            assert_eq!(
                run(&data, &fake, clean(), &row).await,
                Err(NotLanded::Discord(EconDiscordError::MissingPermissions))
            );
            // It was tried, once.
            assert_eq!(fake.calls(), vec![Call::Mute(T, true, ActionKind::Mute)]);
            assert_everyone_refunded(&pool, row.id, "failed").await;
        }
    );

    db_test!(
        a_fund_ignores_the_targets_mirror_at_fill,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            let mirrored = TargetFacts {
                has_mirror: true,
                is_self: true,
                ..clean()
            };
            let done = run(&data, &fake, mirrored, &row).await.unwrap();
            assert_eq!(fake.calls(), vec![Call::Mute(T, true, ActionKind::Mute)]);
            assert_eq!(done.contributors, vec![A, B]);
            // A signed the fund; B pledged unsigned.
            assert_eq!(done.signers, vec![A]);
            assert_eq!(done.until, Some(t0() + Duration::minutes(5)));
        }
    );

    db_test!(
        a_mirror_does_not_hide_immunity_at_fill,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            // A mirror answers a bought prank before immunity is read; a
            // fund has no mirror, so the immunity still refuses it.
            let both = TargetFacts {
                has_mirror: true,
                protection: Some(Protection::Indefinitely),
                ..clean()
            };
            assert_eq!(
                run(&data, &fake, both, &row).await,
                Err(NotLanded::Refused(Refusal::TargetImmune(
                    Protection::Indefinitely
                )))
            );
            assert!(fake.calls().is_empty());
        }
    );

    db_test!(
        a_fund_follows_the_running_name_prank_for_lock_and_restore,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 100).await;
            fund_wallet(&pool, B, 100).await;
            // Someone's rename is running with an older name to restore.
            let running = crate::economy::wallet::ActionPayload::Rename {
                name: "Old".into(),
                restore: Some("Way back".into()),
            };
            let end = t0() + Duration::hours(3);
            sqlx::query(
                "INSERT INTO economy_actions (guild_id, kind, buyer_id, target_id, price, payload, state, expires_at)
                 VALUES (1, 'rename', 30, 20, 120, $1, 'active', $2)",
            )
            .bind(sqlx::types::Json(&running))
            .bind(end)
            .execute(&pool)
            .await
            .unwrap();
            let f = start(&data, clean(), fund_request(A, ActionKind::Lock, Some(30)))
                .await
                .unwrap();
            let p = pledge(&data, pledge_request(B, f.action_id, 30, false))
                .await
                .unwrap();
            assert!(p.pledged.filled);
            let row = row_of(&pool, f.action_id).await;
            let fake = FakeEconomyDiscord::default();
            let done = run(&data, &fake, clean(), &row).await.unwrap();
            // A lock changes nobody; it ends with the running rename.
            assert!(fake.calls().is_empty());
            assert_eq!(done.until, Some(end));
            assert_eq!(
                row_of(&pool, f.action_id).await.payload,
                ActionPayload::Lock {
                    locked: Some("Orig".into())
                }
            );
        }
    );

    db_test!(starting_a_fund_checks_in_order, |pool: PgPool| async {
        let data = fund_data(&pool, |_| {}).await;
        fund_wallet(&pool, A, 40).await;
        let go = |first: Option<i64>| {
            let data = data.clone();
            async move { start(&data, clean(), fund_request(A, ActionKind::Mute, first)).await }
        };
        // Too small, then too poor.
        assert_eq!(
            go(Some(9)).await,
            Err(BuyError::Refused(Refusal::PledgeTooSmall { min: 10 }))
        );
        assert_eq!(
            go(Some(50)).await,
            Err(BuyError::Refused(Refusal::CantAfford { short: 5 }))
        );
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            0
        );
        // The target is refused before the pledge is looked at.
        let immune = TargetFacts {
            protection: Some(Protection::Indefinitely),
            ..clean()
        };
        assert_eq!(
            start(&data, immune, fund_request(A, ActionKind::Mute, Some(1))).await,
            Err(BuyError::Refused(Refusal::TargetImmune(
                Protection::Indefinitely
            )))
        );
        // The preflight comes after the target, before the pledge.
        let gap = Refusal::Unavailable {
            missing: serenity::all::Permissions::MUTE_MEMBERS,
        };
        let mut r = fund_request(A, ActionKind::Mute, Some(1));
        r.preflight = Err(gap);
        assert_eq!(start(&data, clean(), r).await, Err(BuyError::Refused(gap)));
        // A rename needs a valid name; a name on a mute is ignored.
        let mut r = fund_request(A, ActionKind::Rename, None);
        r.nick = None;
        assert_eq!(start(&data, clean(), r).await, Err(BuyError::Invalid));
        let mut r = fund_request(A, ActionKind::Rename, None);
        r.nick = Some("   ".into());
        assert_eq!(start(&data, clean(), r).await, Err(BuyError::Invalid));
        // Drag is never funded.
        assert_eq!(
            start(&data, clean(), fund_request(A, ActionKind::Drag, None)).await,
            Err(BuyError::Invalid)
        );
        // Opening with no pledge at all is allowed; a second like it is not.
        let ok = go(None).await.unwrap();
        assert_eq!((ok.pledged, ok.balance), (0, None));
        assert_eq!(
            go(Some(20)).await,
            Err(BuyError::Refused(Refusal::FundAlreadyOpen))
        );
        // A different prank on the same target is a different fund.
        let other = start(
            &data,
            clean(),
            fund_request(A, ActionKind::Deafen, Some(20)),
        )
        .await
        .unwrap();
        assert_eq!(other.pledged, 20);
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 20);
    });

    db_test!(
        a_fund_needs_crowdfund_and_its_kind_on,
        |pool: PgPool| async {
            let off = fund_data(&pool, |s| {
                s.config.prices.set(
                    ActionKind::Crowdfund,
                    ActionPrice {
                        price: 10,
                        on: false,
                    },
                );
            })
            .await;
            assert_eq!(
                start(&off, clean(), fund_request(A, ActionKind::Mute, None)).await,
                Err(BuyError::Refused(Refusal::NotForSale))
            );
            let off = fund_data(&pool, |s| {
                s.config.prices.set(
                    ActionKind::Mute,
                    ActionPrice {
                        price: 90,
                        on: false,
                    },
                );
            })
            .await;
            assert_eq!(
                start(&off, clean(), fund_request(A, ActionKind::Mute, None)).await,
                Err(BuyError::Refused(Refusal::NotForSale))
            );
            // A pledge needs only Crowdfund.
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, B, 50).await;
            let f = start(&data, clean(), fund_request(A, ActionKind::Mute, None))
                .await
                .unwrap();
            let off = fund_data(&pool, |s| {
                s.config.prices.set(
                    ActionKind::Crowdfund,
                    ActionPrice {
                        price: 10,
                        on: false,
                    },
                );
            })
            .await;
            assert_eq!(
                pledge(&off, pledge_request(B, f.action_id, 20, false)).await,
                Err(BuyError::Refused(Refusal::NotForSale))
            );
            let off = fund_data(&pool, |s| s.enabled = false).await;
            assert_eq!(
                pledge(&off, pledge_request(B, f.action_id, 20, false)).await,
                Err(BuyError::Refused(Refusal::EconomyOff))
            );
        }
    );

    db_test!(
        pledging_checks_who_what_and_how_much,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 100).await;
            fund_wallet(&pool, B, 40).await;
            fund_wallet(&pool, C, 5).await;
            fund_wallet(&pool, T, 100).await;
            // 90 to fill, 45 each at most.
            let f = start(&data, clean(), fund_request(A, ActionKind::Mute, Some(20)))
                .await
                .unwrap();
            let id = f.action_id;
            // The target may not pledge to their own fund.
            assert_eq!(
                pledge(&data, pledge_request(T, id, 20, false)).await,
                Err(BuyError::Refused(Refusal::PledgeOnSelf))
            );
            // Below the minimum is refused while more is needed.
            assert_eq!(
                pledge(&data, pledge_request(B, id, 9, false)).await,
                Err(BuyError::Refused(Refusal::PledgeTooSmall { min: 10 }))
            );
            // Too poor: 60 is capped to the 45 share, and B has 40.
            assert_eq!(
                pledge(&data, pledge_request(B, id, 60, false)).await,
                Err(BuyError::Refused(Refusal::CantAfford { short: 5 }))
            );
            // An unknown fund, and one from another server.
            assert_eq!(
                pledge(&data, pledge_request(B, id + 99, 20, false)).await,
                Err(BuyError::Refused(Refusal::FundClosed))
            );
            let mut elsewhere = pledge_request(B, id, 20, false);
            elsewhere.guild = GuildId::new(2);
            let other = Data(Arc::new(DataInner {
                database_pool: Some(pool.clone()),
                ..Default::default()
            }));
            db::save_settings(
                &pool,
                &EconomySettings {
                    enabled: true,
                    ..EconomySettings::off(GuildId::new(2))
                },
            )
            .await
            .unwrap();
            assert!(crate::economy::load_settings(&other, GuildId::new(2)).await);
            assert_eq!(
                pledge(&other, elsewhere).await,
                Err(BuyError::Refused(Refusal::FundClosed))
            );
            // A pledger who opted out is refused.
            data.economy.set_opted_out(G, [B].into_iter().collect());
            assert_eq!(
                pledge(&data, pledge_request(B, id, 20, false)).await,
                Err(BuyError::Refused(Refusal::BuyerOptedOut))
            );
            data.economy.set_opted_out(G, Default::default());
            assert_eq!(db::balance(&pool, G, B).await.unwrap(), 40);
            // B puts in 40 of the 70 still needed.
            let p = pledge(&data, pledge_request(B, id, 40, true))
                .await
                .unwrap();
            assert_eq!((p.pledged.taken, p.pledged.filled), (40, false));
            assert_eq!(p.fund.id, id);
            assert_eq!(p.fund.target, Some(T));
            // A has 20 in, so only 25 more of their 45 share, though 30 is
            // still needed and 30 was asked: the share caps it.
            let p = pledge(&data, pledge_request(A, id, 30, false))
                .await
                .unwrap();
            assert_eq!(
                (p.pledged.taken, p.pledged.total, p.pledged.filled),
                (25, 85, false)
            );
            // A's share is used up, whatever the fund still needs.
            assert_eq!(
                pledge(&data, pledge_request(A, id, 10, false)).await,
                Err(BuyError::Refused(Refusal::PledgeOverShare { max: 45 }))
            );
            // C covers the 5 left, below the minimum, and fills it.
            let last = pledge(&data, pledge_request(C, id, 5, false))
                .await
                .unwrap();
            assert!(last.pledged.filled);
            // And it is closed to the next one.
            assert_eq!(
                pledge(&data, pledge_request(C, id, 5, false)).await,
                Err(BuyError::Refused(Refusal::FundClosed))
            );
            assert_ledger_agrees(&pool, &[(A, 100), (B, 40), (C, 5)]).await;
        }
    );

    /// Every wallet equals what it started with plus its ledger.
    async fn assert_ledger_agrees(pool: &PgPool, given: &[(UserId, i64)]) {
        for (user, start) in given {
            let delta: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(delta), 0)::bigint FROM economy_ledger WHERE user_id = $1",
            )
            .bind(user.get() as i64)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(
                db::balance(pool, G, *user).await.unwrap(),
                start + delta,
                "wallet of {user} disagrees with its ledger"
            );
        }
    }

    db_test!(
        a_solo_starter_cannot_fill_a_fund_but_two_members_can,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 200).await;
            fund_wallet(&pool, B, 200).await;
            // Asking for the whole 90 takes only A's half, 45.
            let opened = start(&data, clean(), fund_request(A, ActionKind::Mute, Some(90)))
                .await
                .unwrap();
            assert_eq!((opened.pledged, opened.balance), (45, Some(155)));
            let id = opened.action_id;
            assert_eq!(
                row_of(&pool, id).await.state,
                crate::economy::wallet::ActionState::Funding
            );
            // A cannot top it up.
            assert_eq!(
                pledge(&data, pledge_request(A, id, 45, true)).await,
                Err(BuyError::Refused(Refusal::PledgeOverShare { max: 45 }))
            );
            assert_eq!(db::balance(&pool, G, A).await.unwrap(), 155);
            // Another member's half fills it, whatever they ask.
            let p = pledge(&data, pledge_request(B, id, 90, false))
                .await
                .unwrap();
            assert_eq!((p.pledged.taken, p.pledged.filled), (45, true));
            assert_ledger_agrees(&pool, &[(A, 200), (B, 200)]).await;
        }
    );

    db_test!(
        a_fund_the_starter_did_not_pledge_to_still_names_them,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, B, 100).await;
            fund_wallet(&pool, C, 100).await;
            // A signed fund the starter put nothing into.
            let signed = start(&data, clean(), fund_request(A, ActionKind::Mute, None))
                .await
                .unwrap();
            pledge(&data, pledge_request(B, signed.action_id, 45, true))
                .await
                .unwrap();
            let p = pledge(&data, pledge_request(C, signed.action_id, 45, false))
                .await
                .unwrap();
            assert!(p.pledged.filled);
            let row = row_of(&pool, signed.action_id).await;
            let fake = FakeEconomyDiscord::default();
            let done = run(&data, &fake, clean(), &row).await.unwrap();
            assert_eq!(done.contributors, vec![A, B, C]);
            // The starter signed, B signed, C did not.
            assert_eq!(done.signers, vec![A, B]);
            // An unsigned fund: the starter is a contributor, never a signer.
            let mut r = fund_request(A, ActionKind::Deafen, None);
            r.signed = false;
            let unsigned = start(&data, clean(), r).await.unwrap();
            for who in [B, C] {
                pledge(&data, pledge_request(who, unsigned.action_id, 45, who == B))
                    .await
                    .unwrap();
            }
            let row = row_of(&pool, unsigned.action_id).await;
            let done = run(&data, &fake, clean(), &row).await.unwrap();
            assert_eq!(done.contributors, vec![A, B, C]);
            assert_eq!(done.signers, vec![B]);
        }
    );

    db_test!(
        a_pledge_is_judged_against_the_funds_own_price,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 100).await;
            fund_wallet(&pool, B, 100).await;
            let f = start(&data, clean(), fund_request(A, ActionKind::Mute, Some(20)))
                .await
                .unwrap();
            // The owner reprices mutes to 30 while the 90 fund is open.
            let cheaper = fund_data(&pool, |s| {
                s.config.prices.set(
                    ActionKind::Mute,
                    ActionPrice {
                        price: 30,
                        on: true,
                    },
                );
            })
            .await;
            let p = pledge(&cheaper, pledge_request(B, f.action_id, 45, false))
                .await
                .unwrap();
            assert_eq!((p.pledged.taken, p.pledged.price), (45, 90));
            assert!(!p.pledged.filled);
        }
    );

    db_test!(an_opted_out_starter_cannot_start, |pool: PgPool| async {
        let data = fund_data(&pool, |_| {}).await;
        fund_wallet(&pool, A, 100).await;
        data.economy.set_opted_out(G, [A].into_iter().collect());
        assert_eq!(
            start(&data, clean(), fund_request(A, ActionKind::Mute, Some(20))).await,
            Err(BuyError::Refused(Refusal::BuyerOptedOut))
        );
        assert_eq!(db::balance(&pool, G, A).await.unwrap(), 100);
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            0
        );
    });

    db_test!(
        a_prank_switched_off_before_the_fill_refunds_everyone,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            // The owner turns mutes off while the fund fills.
            let mut cfg = data.economy.settings(G).config;
            cfg.prices.set(
                ActionKind::Mute,
                ActionPrice {
                    price: 90,
                    on: false,
                },
            );
            let fake = FakeEconomyDiscord::default();
            let out = execute(
                &data,
                &fake,
                FillFacts {
                    guild: G,
                    target: clean(),
                    preflight: Ok(()),
                },
                &row,
                &cfg,
                t0(),
            )
            .await;
            assert_eq!(out, Err(NotLanded::Refused(Refusal::NotForSale)));
            assert!(fake.calls().is_empty());
            assert_everyone_refunded(&pool, row.id, "refused").await;
        }
    );

    db_test!(
        a_member_who_left_during_the_act_refunds_as_left,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            let fake = FakeEconomyDiscord::default();
            fake.fail_next(CallKind::Mute, EconDiscordError::UnknownMember);
            assert_eq!(
                run(&data, &fake, clean(), &row).await,
                Err(NotLanded::Discord(EconDiscordError::UnknownMember))
            );
            assert_everyone_refunded(&pool, row.id, "left").await;
        }
    );

    db_test!(
        a_filled_fund_is_prepared_before_it_acts,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            let row = filled_mute(&pool, &data).await;
            // A fill leaves no end: that is what marks it unprepared.
            assert_eq!(row.expires_at, None);
            let fake = FakeEconomyDiscord::default();
            run(&data, &fake, clean(), &row).await.unwrap();
            let after = row_of(&pool, row.id).await;
            assert_eq!(after.expires_at, Some(t0() + Duration::minutes(5)));
        }
    );

    db_test!(
        a_last_top_up_below_the_minimum_fills_the_fund,
        |pool: PgPool| async {
            let data = fund_data(&pool, |_| {}).await;
            fund_wallet(&pool, A, 100).await;
            fund_wallet(&pool, B, 100).await;
            let f = start(&data, clean(), fund_request(A, ActionKind::Mute, Some(45)))
                .await
                .unwrap();
            pledge(&data, pledge_request(B, f.action_id, 40, false))
                .await
                .unwrap();
            // 5 left, minimum 10: 5 fills, 4 does not.
            assert_eq!(
                pledge(&data, pledge_request(B, f.action_id, 4, false)).await,
                Err(BuyError::Refused(Refusal::PledgeTooSmall { min: 10 }))
            );
            let p = pledge(&data, pledge_request(B, f.action_id, 5, false))
                .await
                .unwrap();
            assert_eq!((p.pledged.taken, p.pledged.filled), (5, true));
        }
    );
}
