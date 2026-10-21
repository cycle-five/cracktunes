//! Buying things with credits: the checks, the charge, the grant, and the
//! refund when the grant fails. Immunity is the first item for sale.

use super::discord::{EconDiscordError, EconomyDiscord};
use super::effects::end_running_pranks;
use super::immunity::Protection;
use super::preflight::{self, BotPerms};
use super::settings::{clown_name, ActionKind, ActionPrice, EconomyConfig};
use super::target::{check_target, TargetFacts, TargetOutcome};
use super::wallet::{ActionPayload, ActionState, MoveError};
use crate::messaging::messages::{
    ECONOMY_ALREADY_DEAFENED, ECONOMY_ALREADY_IMMUNE, ECONOMY_ALREADY_MUTED,
    ECONOMY_ALREADY_REVEALED, ECONOMY_ALREADY_RUNNING_PREFIX, ECONOMY_ALREADY_THERE,
    ECONOMY_BOUNTY_TOO_SMALL_PREFIX, ECONOMY_BUYER_NOT_IN_VOICE, ECONOMY_BUYER_OPTED_OUT,
    ECONOMY_CANNOT_JOIN_DESTINATION, ECONOMY_CANT_AFFORD_MIDDLE, ECONOMY_CANT_AFFORD_PREFIX,
    ECONOMY_CANT_TELL, ECONOMY_FUND_ALREADY_OPEN, ECONOMY_FUND_CLOSED, ECONOMY_MIRROR_ALREADY_UP,
    ECONOMY_NOTHING_TO_CLEANSE, ECONOMY_NOTHING_TO_REVEAL, ECONOMY_NOT_FOR_SALE,
    ECONOMY_NO_DATABASE, ECONOMY_NO_JAIL_CHANNEL, ECONOMY_OFF, ECONOMY_OUT_OF_REACH,
    ECONOMY_PLEDGE_ON_SELF, ECONOMY_PLEDGE_OVER_SHARE_PREFIX, ECONOMY_PLEDGE_OVER_SHARE_SUFFIX,
    ECONOMY_PLEDGE_TOO_SMALL_PREFIX, ECONOMY_PLEDGE_TOO_SMALL_SUFFIX, ECONOMY_PURCHASE_FAILED,
    ECONOMY_REVEAL_PICK_AGO, ECONOMY_REVEAL_PICK_DAY, ECONOMY_REVEAL_PICK_HOUR,
    ECONOMY_REVEAL_PICK_JUST_NOW, ECONOMY_REVEAL_PICK_MINUTE, ECONOMY_TARGET_IMMUNE_INDEFINITELY,
    ECONOMY_TARGET_IMMUNE_UNTIL, ECONOMY_TARGET_IS_BOT, ECONOMY_TARGET_IS_SELF,
    ECONOMY_TARGET_NOT_IN_VOICE, ECONOMY_TARGET_OPTED_OUT, ECONOMY_UNAVAILABLE_PREFIX,
    ECONOMY_UNAVAILABLE_SUFFIX,
};
use crate::Data;
use chrono::{DateTime, Utc};
use serenity::all::{ChannelId, GuildId, Permissions, UserId};

/// Everything [`check_purchase`] needs, read up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurchaseFacts {
    pub enabled: bool,
    pub price: ActionPrice,
    /// The action's own switch (for immunity, `immunity_shop`).
    pub source_on: bool,
    pub buyer_opted_out: bool,
    /// The buyer holds a moderator grant that lasts until lifted, so a
    /// purchase would add nothing.
    pub already_immune_indefinitely: bool,
    pub balance: i64,
}

/// Why a purchase was refused before any credits moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    EconomyOff,
    NotForSale,
    BuyerOptedOut,
    AlreadyImmune,
    CantAfford {
        short: i64,
    },
    TargetIsBot,
    TargetIsSelf,
    OutOfReach,
    TargetNotInVoice,
    BuyerNotInVoice,
    TargetOptedOut,
    TargetImmune(Protection),
    AlreadyRunning {
        until: DateTime<Utc>,
    },
    CannotJoinDestination,
    NoJailChannel,
    /// A mirror already stands for this buyer.
    MirrorAlreadyUp,
    /// Preflight: CrackTunes lacks these permissions for the action.
    Unavailable {
        missing: Permissions,
    },
    /// Cleanse with no prank running on the buyer.
    NothingToCleanse,
    /// Reveal with no unsigned prank to unmask.
    NothingToReveal,
    /// The picked prank was unmasked already.
    AlreadyRevealed,
    /// A mute on a member who is already server-muted: whoever muted them
    /// owns it, and the shop's undo would lift it.
    AlreadyMuted,
    /// The same for a deafen.
    AlreadyDeafened,
    /// A drag whose target is already in the buyer's channel.
    AlreadyThere,
    /// A bounty below the minimum stake (or not above zero).
    BountyTooSmall {
        min: i64,
    },
    /// A crowdfund for this target and prank is already open.
    FundAlreadyOpen,
    /// The crowdfund is closed, full, expired, or not in this server.
    FundClosed,
    /// A pledge to a crowdfund aimed at the pledger.
    PledgeOnSelf,
    /// A pledge below the minimum that is not what the fund still needs.
    PledgeTooSmall {
        min: i64,
    },
    /// A pledge from a member who has put in their share of the fund (half
    /// its price, rounded up): a fund takes at least two people.
    PledgeOverShare {
        max: i64,
    },
    /// A role position nobody could read (the cache can't place one of the
    /// member's roles, or the bot's): not the hierarchy, so not out of reach.
    CantTell,
}

/// Check a purchase, refusing for the first reason that applies, in order:
/// economy off, not for sale, buyer opted out, already immune indefinitely,
/// can't afford.
pub fn check_purchase(f: &PurchaseFacts) -> Result<(), Refusal> {
    if !f.enabled {
        return Err(Refusal::EconomyOff);
    }
    if !f.source_on || !f.price.on {
        return Err(Refusal::NotForSale);
    }
    if f.buyer_opted_out {
        return Err(Refusal::BuyerOptedOut);
    }
    if f.already_immune_indefinitely {
        return Err(Refusal::AlreadyImmune);
    }
    let price = i64::from(f.price.price);
    if f.balance < price {
        return Err(cant_afford(price, f.balance));
    }
    Ok(())
}

/// The refusal for a buyer holding `balance` against a `price`.
pub fn cant_afford(price: i64, balance: i64) -> Refusal {
    Refusal::CantAfford {
        short: price - balance,
    }
}

/// The words for a refusal, naming the server's currency.
pub fn refusal_text(r: &Refusal, currency: &str) -> String {
    match r {
        Refusal::EconomyOff => ECONOMY_OFF.to_string(),
        Refusal::NotForSale => ECONOMY_NOT_FOR_SALE.to_string(),
        Refusal::BuyerOptedOut => ECONOMY_BUYER_OPTED_OUT.to_string(),
        Refusal::AlreadyImmune => ECONOMY_ALREADY_IMMUNE.to_string(),
        Refusal::CantAfford { short } => {
            format!("{ECONOMY_CANT_AFFORD_PREFIX} {short} {ECONOMY_CANT_AFFORD_MIDDLE} {currency}.")
        },
        Refusal::TargetIsBot => ECONOMY_TARGET_IS_BOT.to_string(),
        Refusal::TargetIsSelf => ECONOMY_TARGET_IS_SELF.to_string(),
        Refusal::OutOfReach => ECONOMY_OUT_OF_REACH.to_string(),
        Refusal::TargetNotInVoice => ECONOMY_TARGET_NOT_IN_VOICE.to_string(),
        Refusal::BuyerNotInVoice => ECONOMY_BUYER_NOT_IN_VOICE.to_string(),
        Refusal::TargetOptedOut => ECONOMY_TARGET_OPTED_OUT.to_string(),
        Refusal::TargetImmune(Protection::Until(t)) => {
            format!("{ECONOMY_TARGET_IMMUNE_UNTIL} <t:{}:f>.", t.timestamp())
        },
        Refusal::TargetImmune(Protection::Indefinitely) => {
            ECONOMY_TARGET_IMMUNE_INDEFINITELY.to_string()
        },
        Refusal::TargetImmune(Protection::OptedOut) => ECONOMY_TARGET_OPTED_OUT.to_string(),
        Refusal::AlreadyRunning { until } => {
            format!(
                "{ECONOMY_ALREADY_RUNNING_PREFIX} <t:{}:R>.",
                until.timestamp()
            )
        },
        Refusal::CannotJoinDestination => ECONOMY_CANNOT_JOIN_DESTINATION.to_string(),
        Refusal::NoJailChannel => ECONOMY_NO_JAIL_CHANNEL.to_string(),
        Refusal::MirrorAlreadyUp => ECONOMY_MIRROR_ALREADY_UP.to_string(),
        Refusal::Unavailable { missing } => format!(
            "{ECONOMY_UNAVAILABLE_PREFIX} {} {ECONOMY_UNAVAILABLE_SUFFIX}",
            crate::rooms::card::perm_names(*missing, true)
        ),
        Refusal::NothingToCleanse => ECONOMY_NOTHING_TO_CLEANSE.to_string(),
        Refusal::NothingToReveal => ECONOMY_NOTHING_TO_REVEAL.to_string(),
        Refusal::AlreadyRevealed => ECONOMY_ALREADY_REVEALED.to_string(),
        Refusal::AlreadyMuted => ECONOMY_ALREADY_MUTED.to_string(),
        Refusal::AlreadyDeafened => ECONOMY_ALREADY_DEAFENED.to_string(),
        Refusal::CantTell => ECONOMY_CANT_TELL.to_string(),
        Refusal::AlreadyThere => ECONOMY_ALREADY_THERE.to_string(),
        Refusal::BountyTooSmall { min } => {
            format!("{ECONOMY_BOUNTY_TOO_SMALL_PREFIX} {min} {currency}.")
        },
        Refusal::FundAlreadyOpen => ECONOMY_FUND_ALREADY_OPEN.to_string(),
        Refusal::FundClosed => ECONOMY_FUND_CLOSED.to_string(),
        Refusal::PledgeOnSelf => ECONOMY_PLEDGE_ON_SELF.to_string(),
        Refusal::PledgeOverShare { max } => {
            format!(
                "{ECONOMY_PLEDGE_OVER_SHARE_PREFIX} {max} {currency} {ECONOMY_PLEDGE_OVER_SHARE_SUFFIX}"
            )
        },
        Refusal::PledgeTooSmall { min } => {
            format!(
                "{ECONOMY_PLEDGE_TOO_SMALL_PREFIX} {min} {currency}, {ECONOMY_PLEDGE_TOO_SMALL_SUFFIX}"
            )
        },
    }
}

/// The words for a purchase that did not happen.
pub fn buy_error_text(err: &BuyError, currency: &str) -> String {
    match err {
        BuyError::Refused(r) => refusal_text(r, currency),
        BuyError::NoDatabase => ECONOMY_NO_DATABASE.to_string(),
        BuyError::Db | BuyError::Discord(_) | BuyError::Invalid => {
            ECONOMY_PURCHASE_FAILED.to_string()
        },
    }
}

/// Why a purchase did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuyError {
    Refused(Refusal),
    NoDatabase,
    Db,
    /// The action could not be done in Discord; the price was refunded.
    Discord(EconDiscordError),
    /// The request is malformed (a prank without a target, a rename without
    /// a name, or a kind `buy` does not sell). Nothing was charged.
    Invalid,
}

/// A completed immunity purchase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImmunityBought {
    pub until: DateTime<Utc>,
    pub balance: i64,
}

/// Buy immunity: check, charge (recorded `pending`), then grant and mark the
/// action `done` in one transaction. A failed grant is refunded.
///
/// A row left `pending` was never granted: the grant and `done` commit
/// together. Reconcile (PR 2) may refund any pending immunity row.
pub async fn buy_immunity(
    data: &Data,
    g: GuildId,
    buyer: UserId,
    now: DateTime<Utc>,
) -> Result<ImmunityBought, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let settings = data.economy.settings(g);
    let cfg = &settings.config;
    let price = cfg.prices.get(ActionKind::Immunity);
    let balance = db::balance(pool, g, buyer).await.map_err(|err| {
        tracing::warn!("economy: reading balance of {buyer} in {g} failed: {err}");
        BuyError::Db
    })?;
    let grants = db::grants(pool, g, buyer).await.map_err(|err| {
        tracing::warn!("economy: reading grants of {buyer} in {g} failed: {err}");
        BuyError::Db
    })?;
    let facts = PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price,
        source_on: cfg.immunity_shop,
        buyer_opted_out: data.economy.is_opted_out(g, buyer),
        already_immune_indefinitely: grants.iter().any(|gr| gr.until.is_none()),
        balance,
    };
    check_purchase(&facts).map_err(BuyError::Refused)?;

    let hours = cfg.bought_immunity_hours;
    let payload = ActionPayload::Immunity { hours };
    let amount = i64::from(price.price);
    let charged = db::charge_for_action(
        pool,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Immunity,
            buyer,
            target: None,
            price: amount,
            note: None,
            signed: false,
            payload: &payload,
            expires_at: None,
        },
    )
    .await
    .map_err(|err| match err {
        // The balance moved between the check and the charge.
        MoveError::Insufficient { balance } => BuyError::Refused(cant_afford(amount, balance)),
        MoveError::Invalid => BuyError::Db,
        MoveError::Db(err) => {
            tracing::warn!("economy: charging {buyer} in {g} failed: {err}");
            BuyError::Db
        },
    })?;

    let until =
        match db::grant_bought_for_action(pool, charged.action_id, g, buyer, hours, now).await {
            Ok(until) => until,
            Err(err) => {
                tracing::warn!("economy: granting immunity to {buyer} in {g} failed: {err}");
                // Refund moves the action to failed and restores the credits.
                if let Err(err) = db::refund_action(pool, charged.action_id).await {
                    tracing::error!(
                        "economy: refunding action {} failed: {err}",
                        charged.action_id
                    );
                }
                return Err(BuyError::Db);
            },
        };
    Ok(ImmunityBought {
        until,
        balance: charged.balance,
    })
}

/// What happened to a bought prank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuyOutcome {
    /// It landed on the target (or, for a mirror, the shield is up).
    Landed,
    /// The target's mirror sent it back: it landed on the buyer.
    Bounced,
    /// The target's mirror sent it back and it could not land on the buyer.
    /// The buyer was charged all the same.
    Blocked,
}

/// A completed purchase of a prank or a mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bought {
    pub action_id: i64,
    pub balance: i64,
    pub outcome: BuyOutcome,
    /// When the effect ends, for a timed one that landed.
    pub until: Option<DateTime<Utc>>,
}

/// What to buy. `target` is `None` for a mirror, which protects the buyer.
#[derive(Debug, Clone)]
pub struct BuyRequest {
    pub guild: GuildId,
    pub buyer: UserId,
    pub kind: ActionKind,
    pub target: Option<UserId>,
    /// The new nickname, for a rename.
    pub nick: Option<String>,
    pub note: Option<String>,
    pub signed: bool,
    pub now: DateTime<Utc>,
}

pub(crate) fn payload_for(
    kind: ActionKind,
    on: &TargetFacts,
    buyer_voice: Option<ChannelId>,
    cfg: &EconomyConfig,
    nick: Option<&str>,
    restore: Option<String>,
    locked: Option<String>,
) -> Result<ActionPayload, BuyError> {
    Ok(match kind {
        ActionKind::Rename => ActionPayload::Rename {
            name: nick.ok_or(BuyError::Invalid)?.to_string(),
            restore,
        },
        ActionKind::Clown => ActionPayload::Clown {
            applied: clown_name(&on.display_name),
            restore,
        },
        ActionKind::Lock => ActionPayload::Lock { locked },
        ActionKind::Drag => ActionPayload::Drag {
            from: on.in_voice.ok_or(BuyError::Invalid)?,
            to: buyer_voice.ok_or(BuyError::Invalid)?,
        },
        ActionKind::Mute => ActionPayload::Mute,
        ActionKind::Deafen => ActionPayload::Deafen,
        ActionKind::Jail => ActionPayload::Jail {
            jail: cfg.jail_channel_id.ok_or(BuyError::Invalid)?,
        },
        ActionKind::Mirror => ActionPayload::Mirror,
        // Not sold through `buy`.
        ActionKind::Immunity
        | ActionKind::Cleanse
        | ActionKind::Reveal
        | ActionKind::Theme
        | ActionKind::Dedicate
        | ActionKind::Jump
        | ActionKind::Bounty
        | ActionKind::Crowdfund => return Err(BuyError::Invalid),
    })
}

/// Do the Discord half of a prank. Lock and mirror change nobody.
pub(crate) async fn act(
    discord: &dyn EconomyDiscord,
    g: GuildId,
    on: UserId,
    kind: ActionKind,
    payload: &ActionPayload,
) -> Result<(), EconDiscordError> {
    match payload {
        ActionPayload::Rename { name, .. } => discord.set_nick(g, on, Some(name), kind).await,
        ActionPayload::Clown { applied, .. } => discord.set_nick(g, on, Some(applied), kind).await,
        ActionPayload::Mute => discord.set_mute(g, on, true, kind).await,
        ActionPayload::Deafen => discord.set_deaf(g, on, true, kind).await,
        ActionPayload::Drag { to, .. } => discord.move_member(g, on, *to, kind).await,
        ActionPayload::Jail { jail } => discord.move_member(g, on, *jail, kind).await,
        ActionPayload::Lock { .. }
        | ActionPayload::Mirror
        | ActionPayload::Immunity { .. }
        | ActionPayload::Cleanse
        | ActionPayload::Reveal { .. }
        | ActionPayload::Theme { .. }
        | ActionPayload::Dedicate { .. }
        | ActionPayload::Jump { .. }
        | ActionPayload::Bounty => Ok(()),
    }
}

/// The name pranks running on `victim`, read only for the kinds that care
/// (rename, clown and lock): they set what a new one restores, and when a
/// lock ends.
pub(crate) async fn running_name_pranks(
    pool: &sqlx::PgPool,
    g: GuildId,
    victim: UserId,
    kind: ActionKind,
) -> Result<Vec<crate::db::economy::ActionRow>, BuyError> {
    if !matches!(
        kind,
        ActionKind::Rename | ActionKind::Clown | ActionKind::Lock
    ) {
        return Ok(Vec::new());
    }
    crate::db::economy::active_of_kind(pool, g, victim, &[ActionKind::Rename, ActionKind::Clown])
        .await
        .map_err(|e| db_failed(&format!("reading name pranks on {victim} in {g}"), &e))
}

/// The name a new name prank restores: the running one's own capture, else
/// the member's nickname now (`None` = no nickname).
pub(crate) fn captured_restore(
    running: &[crate::db::economy::ActionRow],
    current_nick: &Option<String>,
) -> Option<String> {
    let running_restore = running.iter().find_map(|r| match &r.payload {
        ActionPayload::Rename { restore, .. } | ActionPayload::Clown { restore, .. } => {
            Some(restore.clone())
        },
        _ => None,
    });
    running_restore.unwrap_or_else(|| current_nick.clone())
}

/// When a prank of `kind` bought at `now` ends. A lock follows the running
/// name prank's end (only an `active` one, not one winding down), else
/// `rename_hours`.
pub(crate) fn prank_expiry(
    kind: ActionKind,
    running: &[crate::db::economy::ActionRow],
    now: DateTime<Utc>,
    cfg: &EconomyConfig,
) -> Option<DateTime<Utc>> {
    match kind {
        ActionKind::Rename | ActionKind::Clown => {
            Some(now + chrono::Duration::hours(i64::from(cfg.rename_hours)))
        },
        ActionKind::Lock => running
            .iter()
            .filter(|r| r.state == ActionState::Active)
            .find_map(|r| r.expires_at)
            .or_else(|| Some(now + chrono::Duration::hours(i64::from(cfg.rename_hours)))),
        ActionKind::Mute | ActionKind::Deafen | ActionKind::Jail => {
            Some(now + chrono::Duration::minutes(i64::from(cfg.short_effect_minutes)))
        },
        _ => None,
    }
}

pub(crate) fn db_failed(what: &str, err: &sqlx::Error) -> BuyError {
    tracing::warn!("economy: {what} failed: {err}");
    BuyError::Db
}

/// End a charged purchase as blocked (no refund) and say so.
async fn blocked(pool: &sqlx::PgPool, id: i64, balance: i64) -> Bought {
    if let Err(err) = crate::db::economy::finish_blocked(pool, id).await {
        tracing::error!("economy: ending blocked action {id} failed: {err}");
    }
    Bought {
        action_id: id,
        balance,
        outcome: BuyOutcome::Blocked,
        until: None,
    }
}

/// Buy a prank (rename, lock, clown, drag, mute, deafen, jail) or a mirror:
/// check, charge, act in Discord, then activate. `facts` describes the target;
/// `buyer_facts` the buyer seen as a target, for a mirror's bounce.
///
/// Cleanse, reveal and immunity are not sold here (`BuyError::Invalid`, from
/// `payload_for`, before anything is charged).
/// The caller posts the card and the mod log event.
///
/// A target's mirror is spent whether the prank bounces onto the buyer or is
/// blocked, and a blocked prank is not refunded. A bounce never bounces again
/// and never uses up the buyer's own mirror; a drag is always blocked, as it
/// would move the buyer into their own channel.
pub async fn buy(
    data: &Data,
    discord: &dyn EconomyDiscord,
    facts: TargetFacts,
    buyer_facts: TargetFacts,
    buyer_voice: Option<ChannelId>,
    req: BuyRequest,
) -> Result<Bought, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let (g, buyer, kind) = (req.guild, req.buyer, req.kind);
    let settings = data.economy.settings(g);
    let cfg = &settings.config;
    let price = cfg.prices.get(kind);
    let balance = db::balance(pool, g, buyer)
        .await
        .map_err(|e| db_failed(&format!("reading balance of {buyer} in {g}"), &e))?;
    check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price,
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(g, buyer),
        already_immune_indefinitely: false,
        balance,
    })
    .map_err(BuyError::Refused)?;
    if kind == ActionKind::Mirror {
        let standing = db::active_of_kind(pool, g, buyer, &[ActionKind::Mirror])
            .await
            .map_err(|e| db_failed(&format!("reading the mirror of {buyer} in {g}"), &e))?;
        if !standing.is_empty() {
            return Err(BuyError::Refused(Refusal::MirrorAlreadyUp));
        }
    }
    if kind == ActionKind::Jail && cfg.jail_channel_id.is_none() {
        return Err(BuyError::Refused(Refusal::NoJailChannel));
    }

    // Who it lands on, and whether a mirror is in the way.
    // The third item is whose mirror was hit, when one was.
    let (victim, outcome, mirror_owner) = if kind == ActionKind::Mirror {
        (buyer, BuyOutcome::Landed, None)
    } else {
        let target = req.target.ok_or(BuyError::Invalid)?;
        match check_target(kind, &facts, buyer_voice).map_err(BuyError::Refused)? {
            TargetOutcome::Lands => (target, BuyOutcome::Landed, None),
            TargetOutcome::Mirrored => {
                let bounce = TargetFacts {
                    is_self: false,
                    has_mirror: false,
                    ..buyer_facts.clone()
                };
                if kind != ActionKind::Drag && check_target(kind, &bounce, buyer_voice).is_ok() {
                    (buyer, BuyOutcome::Bounced, Some(target))
                } else {
                    (target, BuyOutcome::Blocked, Some(target))
                }
            },
        }
    };
    let on = match outcome {
        BuyOutcome::Bounced => &buyer_facts,
        _ => &facts,
    };

    // A name prank running on the one it lands on: its end, and what to restore.
    let running = running_name_pranks(pool, g, victim, kind).await?;
    let restore = captured_restore(&running, &on.current_nick);
    let payload = payload_for(
        kind,
        on,
        buyer_voice,
        cfg,
        req.nick.as_deref(),
        restore,
        on.current_nick.clone(),
    )?;
    let expires = prank_expiry(kind, &running, req.now, cfg);

    let amount = i64::from(price.price);
    let charged = charge(
        pool,
        g,
        buyer,
        amount,
        &db::NewAction {
            guild: g,
            kind,
            buyer,
            target: Some(victim),
            price: amount,
            note: req.note.as_deref(),
            signed: req.signed,
            payload: &payload,
            expires_at: if outcome == BuyOutcome::Blocked {
                None
            } else {
                expires
            },
        },
    )
    .await?;
    let id = charged.action_id;
    let refund = || async {
        if let Err(err) = db::refund_action(pool, id).await {
            tracing::error!("economy: refunding action {id} failed: {err}");
        }
    };

    if let Some(owner) = mirror_owner {
        // The mirror is spent after the charge, so a failed charge keeps it.
        match db::consume_mirror(pool, g, owner).await {
            Ok(true) => {},
            Ok(false) => {
                // Another buyer spent it first, so the facts are stale.
                tracing::info!("economy: {owner}'s mirror in {g} was already spent; refunding");
                refund().await;
                return Err(BuyError::Db);
            },
            Err(err) => {
                refund().await;
                return Err(db_failed(
                    &format!("spending the mirror of {owner} in {g}"),
                    &err,
                ));
            },
        }
        if outcome == BuyOutcome::Blocked {
            return Ok(blocked(pool, id, charged.balance).await);
        }
    }

    if let Err(err) = act(discord, g, victim, kind, &payload).await {
        tracing::warn!("economy: {kind:?} on {victim} in {g} failed in Discord: {err:?}");
        if outcome == BuyOutcome::Bounced {
            // The defender's mirror is spent and the bounce could not land on
            // the attacker: blocked, and the attacker is charged all the same.
            return Ok(blocked(pool, id, charged.balance).await);
        }
        refund().await;
        return Err(BuyError::Discord(err));
    }
    // The effect has landed. If this write fails the row stays `pending` and
    // reconcile resolves it; the buyer still gets their card.
    match db::activate_action(pool, id, kind, expires).await {
        Ok(true) => {},
        Ok(false) => tracing::warn!("economy: action {id} was no longer pending at activation"),
        Err(err) => tracing::error!("economy: activating action {id} failed: {err}"),
    }
    Ok(Bought {
        action_id: id,
        balance: charged.balance,
        outcome,
        until: expires,
    })
}

/// Refuse a purchase before charging when CrackTunes lacks what the action
/// needs (spec 7: each action is sold only while its needs are met).
/// `channels` are the ones involved: the target's voice and the buyer's for a
/// drag, the jail channel for jail. `perms` of `None` (the cache cannot say)
/// fails closed: everything counts as missing.
pub fn preflight_refusal(
    kind: ActionKind,
    perms: Option<&BotPerms>,
    channels: &[ChannelId],
) -> Result<(), Refusal> {
    let none = BotPerms::default();
    let gaps = preflight::missing(kind, perms.unwrap_or(&none), channels);
    let missing = gaps
        .into_iter()
        .fold(Permissions::empty(), |all, (_, gap)| all | gap);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Refusal::Unavailable { missing })
    }
}

/// How long ago, in words (autocomplete labels are plain text, so no
/// `<t:..:R>`): "just now", "5 minutes ago", "1 hour ago", "2 days ago".
pub fn ago_words(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let minutes = now.signed_duration_since(then).num_minutes().max(0);
    let (n, unit) = if minutes < 1 {
        return ECONOMY_REVEAL_PICK_JUST_NOW.to_string();
    } else if minutes < 60 {
        (minutes, ECONOMY_REVEAL_PICK_MINUTE)
    } else if minutes < 60 * 24 {
        (minutes / 60, ECONOMY_REVEAL_PICK_HOUR)
    } else {
        (minutes / (60 * 24), ECONOMY_REVEAL_PICK_DAY)
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural} {ECONOMY_REVEAL_PICK_AGO}")
}

/// Charge a purchase and map the ways it can fail. The caller has already
/// checked affordability; this covers the balance moving in between.
async fn charge(
    pool: &sqlx::PgPool,
    g: GuildId,
    buyer: UserId,
    price: i64,
    action: &crate::db::economy::NewAction<'_>,
) -> Result<crate::db::economy::Charged, BuyError> {
    crate::db::economy::charge_for_action(pool, action)
        .await
        .map_err(|err| match err {
            MoveError::Insufficient { balance } => BuyError::Refused(cant_afford(price, balance)),
            MoveError::Invalid => BuyError::Db,
            MoveError::Db(err) => db_failed(&format!("charging {buyer} in {g}"), &err),
        })
}

/// A completed cleanse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cleansed {
    /// How many running pranks this call ended, set aside or scheduled.
    pub ended: usize,
    pub balance: i64,
}

/// Buy a cleanse: refuse before charging when nothing is running (an
/// `ending` row is already on its way out and doesn't count), check, charge,
/// end every running prank on the buyer (`cleansed`), then mark the purchase
/// `done`. A prank whose undo fails is scheduled for the sweeper to retry
/// (nothing is refunded for it). A standing mirror is kept. If the charge
/// went through but this call ended no `active` row (a race with the
/// sweeper, or a database error), the charge is refunded.
pub async fn cleanse(
    data: &Data,
    discord: &dyn EconomyDiscord,
    g: GuildId,
    user: UserId,
    now: DateTime<Utc>,
) -> Result<Cleansed, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let running = db::active_of_kind(pool, g, user, ActionKind::PRANKS)
        .await
        .map_err(|e| db_failed(&format!("reading pranks on {user} in {g}"), &e))?;
    if !running.iter().any(|r| r.state == ActionState::Active) {
        return Err(BuyError::Refused(Refusal::NothingToCleanse));
    }
    let settings = data.economy.settings(g);
    let price = settings.config.prices.get(ActionKind::Cleanse);
    let balance = db::balance(pool, g, user)
        .await
        .map_err(|e| db_failed(&format!("reading balance of {user} in {g}"), &e))?;
    check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price,
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(g, user),
        already_immune_indefinitely: false,
        balance,
    })
    .map_err(BuyError::Refused)?;
    charge_and_cleanse(data, discord, g, user, i64::from(price.price), now).await
}

/// The charged half of [`cleanse`], split out so the refund when nothing is
/// left to end can be driven directly.
async fn charge_and_cleanse(
    data: &Data,
    discord: &dyn EconomyDiscord,
    g: GuildId,
    user: UserId,
    amount: i64,
    now: DateTime<Utc>,
) -> Result<Cleansed, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let charged = charge(
        pool,
        g,
        user,
        amount,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Cleanse,
            buyer: user,
            target: Some(user),
            price: amount,
            note: None,
            signed: false,
            payload: &ActionPayload::Cleanse,
            expires_at: None,
        },
    )
    .await?;
    let ended = match end_running_pranks(data, discord, g, user, "cleansed", now).await {
        Ok(n) => n.were_active,
        Err(err) => {
            tracing::warn!("economy: ending the pranks on {user} in {g} failed: {err}");
            0
        },
    };
    if ended == 0 {
        // Never charge for nothing: something ended them first, or the read failed.
        if let Err(err) = db::refund_action(pool, charged.action_id).await {
            tracing::error!(
                "economy: refunding cleanse {} failed: {err}",
                charged.action_id
            );
        }
        return Err(BuyError::Refused(Refusal::NothingToCleanse));
    }
    match db::activate_action(pool, charged.action_id, ActionKind::Cleanse, None).await {
        Ok(true) => {},
        Ok(false) => tracing::warn!(
            "economy: cleanse {} was no longer pending at activation",
            charged.action_id
        ),
        Err(err) => tracing::error!(
            "economy: activating cleanse {} failed: {err}",
            charged.action_id
        ),
    }
    Ok(Cleansed {
        ended,
        balance: charged.balance,
    })
}

/// A completed reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revealed {
    /// Who bought the unmasked prank.
    pub buyer: UserId,
    pub kind: ActionKind,
    pub balance: i64,
}

/// The pranks `user` could unmask, newest first, at most `limit`: unsigned,
/// unrevealed, aimed at them by someone else (spec 4.3). Autocomplete reads
/// these too.
pub async fn reveal_choices(
    data: &Data,
    g: GuildId,
    user: UserId,
    limit: i64,
) -> Vec<crate::db::economy::RevealCandidate> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Vec::new();
    };
    crate::db::economy::reveal_candidates(pool, g, user, None, true, limit)
        .await
        .unwrap_or_else(|err| {
            tracing::warn!("economy: listing reveals for {user} in {g} failed: {err}");
            Vec::new()
        })
}

/// Buy a reveal: name the buyer of an unsigned prank aimed at `user`.
/// `action_id` of `None` picks the most recent unrevealed one; a picked id
/// that is not an unsigned prank aimed at `user` is `NothingToReveal`
/// (nobody unmasks other people's pranks), and one already unmasked is
/// `AlreadyRevealed`. Both refuse before charging, and before affordability
/// is looked at. A prank a mirror blocked counts: it was aimed at the mirror's
/// owner.
pub async fn reveal(
    data: &Data,
    g: GuildId,
    user: UserId,
    action_id: Option<i64>,
) -> Result<Revealed, BuyError> {
    use crate::db::economy as db;
    let Some(pool) = data.database_pool.as_ref() else {
        return Err(BuyError::NoDatabase);
    };
    let found = db::reveal_candidates(pool, g, user, action_id, action_id.is_none(), 1)
        .await
        .map_err(|e| db_failed(&format!("reading pranks on {user} in {g}"), &e))?;
    let Some(prank) = found.into_iter().next().map(|c| c.row) else {
        return Err(BuyError::Refused(Refusal::NothingToReveal));
    };
    if prank.revealed_at.is_some() {
        return Err(BuyError::Refused(Refusal::AlreadyRevealed));
    }
    let settings = data.economy.settings(g);
    let price = settings.config.prices.get(ActionKind::Reveal);
    let balance = db::balance(pool, g, user)
        .await
        .map_err(|e| db_failed(&format!("reading balance of {user} in {g}"), &e))?;
    check_purchase(&PurchaseFacts {
        enabled: data.economy.is_enabled(g),
        price,
        source_on: true,
        buyer_opted_out: data.economy.is_opted_out(g, user),
        already_immune_indefinitely: false,
        balance,
    })
    .map_err(BuyError::Refused)?;
    let amount = i64::from(price.price);
    let charged = charge(
        pool,
        g,
        user,
        amount,
        &db::NewAction {
            guild: g,
            kind: ActionKind::Reveal,
            buyer: user,
            target: Some(user),
            price: amount,
            note: None,
            signed: false,
            payload: &ActionPayload::Reveal {
                action_id: prank.id,
            },
            expires_at: None,
        },
    )
    .await?;
    let id = charged.action_id;
    // Unmasking and marking the purchase done commit together, so a reveal
    // left `pending` never unmasked anything.
    match db::reveal_action(pool, id, prank.id, user).await {
        Ok(true) => {},
        outcome => {
            if let Err(err) = db::refund_action(pool, id).await {
                tracing::error!("economy: refunding action {id} failed: {err}");
            }
            return match outcome {
                // Another press unmasked it between the check and now.
                Ok(_) => Err(BuyError::Refused(Refusal::AlreadyRevealed)),
                Err(err) => Err(db_failed(&format!("unmasking action {}", prank.id), &err)),
            };
        },
    }
    Ok(Revealed {
        buyer: prank.buyer,
        kind: prank.kind,
        balance: charged.balance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::discord::EconDiscordError;
    use crate::economy::immunity::Protection;
    use crate::economy::settings::EconomySettings;
    use crate::DataInner;
    use sqlx::PgPool;
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(10);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn ok() -> PurchaseFacts {
        PurchaseFacts {
            enabled: true,
            price: ActionPrice {
                price: 200,
                on: true,
            },
            source_on: true,
            buyer_opted_out: false,
            already_immune_indefinitely: false,
            balance: 1000,
        }
    }

    #[test]
    fn check_purchase_order() {
        assert_eq!(check_purchase(&ok()), Ok(()));
        // Everything wrong at once: economy off wins, then each falls away.
        let mut f = PurchaseFacts {
            enabled: false,
            source_on: false,
            buyer_opted_out: true,
            already_immune_indefinitely: true,
            balance: 0,
            ..ok()
        };
        assert_eq!(check_purchase(&f), Err(Refusal::EconomyOff));
        f.enabled = true;
        assert_eq!(check_purchase(&f), Err(Refusal::NotForSale));
        f.source_on = true;
        f.price.on = false;
        assert_eq!(check_purchase(&f), Err(Refusal::NotForSale));
        f.price.on = true;
        assert_eq!(check_purchase(&f), Err(Refusal::BuyerOptedOut));
        f.buyer_opted_out = false;
        assert_eq!(check_purchase(&f), Err(Refusal::AlreadyImmune));
        f.already_immune_indefinitely = false;
        assert_eq!(check_purchase(&f), Err(Refusal::CantAfford { short: 200 }));
        f.balance = 200;
        assert_eq!(check_purchase(&f), Ok(()));
    }

    #[test]
    fn cant_afford_says_how_many_more() {
        let f = PurchaseFacts {
            balance: 160,
            ..ok()
        };
        let r = check_purchase(&f).unwrap_err();
        assert_eq!(r, Refusal::CantAfford { short: 40 });
        assert_eq!(refusal_text(&r, "credits"), "You need 40 more credits.");
    }

    #[test]
    fn cant_afford_is_price_minus_balance() {
        assert_eq!(cant_afford(200, 50), Refusal::CantAfford { short: 150 });
        assert_eq!(cant_afford(200, 0), Refusal::CantAfford { short: 200 });
    }

    #[test]
    fn every_refusal_has_its_own_text() {
        let all = [
            Refusal::EconomyOff,
            Refusal::NotForSale,
            Refusal::BuyerOptedOut,
            Refusal::AlreadyImmune,
            Refusal::CantAfford { short: 1 },
            Refusal::TargetIsBot,
            Refusal::TargetIsSelf,
            Refusal::OutOfReach,
            Refusal::TargetNotInVoice,
            Refusal::BuyerNotInVoice,
            Refusal::TargetOptedOut,
            Refusal::TargetImmune(Protection::Until(t0())),
            Refusal::TargetImmune(Protection::Indefinitely),
            Refusal::AlreadyRunning { until: t0() },
            Refusal::CannotJoinDestination,
            Refusal::NoJailChannel,
            Refusal::MirrorAlreadyUp,
            Refusal::Unavailable {
                missing: Permissions::MOVE_MEMBERS,
            },
            Refusal::NothingToCleanse,
            Refusal::NothingToReveal,
            Refusal::AlreadyRevealed,
            Refusal::AlreadyMuted,
            Refusal::AlreadyDeafened,
            Refusal::CantTell,
            Refusal::AlreadyThere,
            Refusal::BountyTooSmall { min: 50 },
            Refusal::FundAlreadyOpen,
            Refusal::FundClosed,
            Refusal::PledgeOnSelf,
            Refusal::PledgeTooSmall { min: 10 },
            Refusal::PledgeOverShare { max: 45 },
        ];
        let texts: std::collections::HashSet<_> =
            all.iter().map(|r| refusal_text(r, "credits")).collect();
        assert_eq!(texts.len(), all.len());
        assert_eq!(refusal_text(&Refusal::EconomyOff, "x"), ECONOMY_OFF);
    }

    #[test]
    fn target_refusals_carry_their_facts() {
        let t = t0().timestamp();
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::Until(t0())), "c"),
            format!("They're immune until <t:{t}:f>.")
        );
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::Indefinitely), "c"),
            "They're immune until a moderator lifts it."
        );
        assert_eq!(
            refusal_text(&Refusal::TargetImmune(Protection::OptedOut), "c"),
            "They've opted out of the economy."
        );
        assert_eq!(
            refusal_text(&Refusal::AlreadyRunning { until: t0() }, "c"),
            format!("They're already pranked like that until <t:{t}:R>.")
        );
        let text = refusal_text(
            &Refusal::Unavailable {
                missing: Permissions::MOVE_MEMBERS | Permissions::MUTE_MEMBERS,
            },
            "c",
        );
        assert!(text.starts_with("CrackTunes is missing "), "{text}");
        assert!(text.ends_with(" for that."), "{text}");
        assert!(
            text.contains("Move Members") && text.contains("Mute Members"),
            "{text}"
        );
    }

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    async fn shop_data(pool: &PgPool, funds: i64) -> Data {
        let s = EconomySettings {
            enabled: true,
            ..EconomySettings::off(G)
        };
        crate::db::economy::save_settings(pool, &s).await.unwrap();
        if funds > 0 {
            crate::db::economy::credit_earnings(
                pool,
                G,
                &[crate::db::economy::EarnRow {
                    user: U,
                    voice: funds,
                    messages: 0,
                }],
            )
            .await
            .unwrap();
        }
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        assert!(crate::economy::load_settings(&data, G).await);
        data
    }

    async fn count(pool: &PgPool, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
    }

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

    db_test!(buying_immunity_charges_and_protects, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let now = t0();
        let bought = buy_immunity(&data, G, U, now).await.unwrap();
        assert_eq!(bought.until, now + chrono::Duration::hours(24));
        assert_eq!(bought.balance, 300);
        assert_eq!(
            crate::economy::protection(&data, G, U, now).await.unwrap(),
            Some(Protection::Until(now + chrono::Duration::hours(24)))
        );
        assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 300);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE delta = -200 AND kind <> 'refund'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'done'"
            )
            .await,
            1
        );
    });

    db_test!(buying_twice_extends, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let now = t0();
        buy_immunity(&data, G, U, now).await.unwrap();
        let again = buy_immunity(&data, G, U, now).await.unwrap();
        assert_eq!(again.until, now + chrono::Duration::hours(48));
        assert_eq!(again.balance, 100);
    });

    db_test!(
        a_broke_buyer_is_refused_and_nothing_moves,
        |pool: PgPool| async {
            let data = shop_data(&pool, 160).await;
            assert_eq!(
                buy_immunity(&data, G, U, t0()).await,
                Err(BuyError::Refused(Refusal::CantAfford { short: 40 }))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 160);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions").await,
                0
            );
        }
    );

    db_test!(
        two_buys_racing_for_one_purchase_of_funds,
        |pool: PgPool| async {
            // 250 buys one immunity. Whether the loser is stopped by the check or
            // by the charge (the race), it is told the same shortfall.
            let data = shop_data(&pool, 250).await;
            let now = t0();
            let (a, b) = tokio::join!(
                buy_immunity(&data, G, U, now),
                buy_immunity(&data, G, U, now)
            );
            let (won, lost) = if a.is_ok() { (a, b) } else { (b, a) };
            assert_eq!(won.unwrap().balance, 50);
            assert_eq!(
                lost,
                Err(BuyError::Refused(Refusal::CantAfford { short: 150 }))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 50);
        }
    );

    db_test!(
        an_indefinite_mod_grant_refuses_a_purchase_before_any_charge,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            crate::db::economy::grant_mod(&pool, G, U, None, UserId::new(99), None)
                .await
                .unwrap();
            assert_eq!(
                buy_immunity(&data, G, U, t0()).await,
                Err(BuyError::Refused(Refusal::AlreadyImmune))
            );
            assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 500);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions").await,
                0
            );
        }
    );

    db_test!(grant_and_done_commit_together, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        buy_immunity(&data, G, U, t0()).await.unwrap();
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'done' AND ended_at IS NOT NULL"
            )
            .await,
            1
        );
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_immunity").await,
            1
        );
    });

    db_test!(
        granting_a_finished_action_grants_nothing,
        |pool: PgPool| async {
            use crate::db::economy as db;
            use crate::economy::wallet::ActionPayload;
            let data = shop_data(&pool, 500).await;
            let payload = ActionPayload::Immunity { hours: 24 };
            let charged = db::charge_for_action(
                &pool,
                &db::NewAction {
                    guild: G,
                    kind: ActionKind::Immunity,
                    buyer: U,
                    target: None,
                    price: 200,
                    note: None,
                    signed: false,
                    payload: &payload,
                    expires_at: None,
                },
            )
            .await
            .unwrap();
            db::refund_action(&pool, charged.action_id).await.unwrap();
            let r = db::grant_bought_for_action(&pool, charged.action_id, G, U, 24, t0()).await;
            assert!(r.is_err());
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_immunity").await,
                0
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
            let _ = data;
        }
    );

    db_test!(a_failed_grant_is_refunded, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        // Reads of grants still work (the check reads them); only the grant fails.
        sqlx::query(
            "CREATE FUNCTION refuse_grant() RETURNS trigger AS $$ BEGIN RAISE EXCEPTION 'no'; END $$ LANGUAGE plpgsql",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TRIGGER refuse_grant BEFORE INSERT ON economy_immunity FOR EACH ROW EXECUTE FUNCTION refuse_grant()",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(buy_immunity(&data, G, U, t0()).await, Err(BuyError::Db));
        assert_eq!(crate::db::economy::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'failed'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'refund'"
            )
            .await,
            1
        );
    });

    // ---- buying a prank ----

    use crate::db::economy as db;
    use crate::economy::discord::fake::{Call, CallKind, FakeEconomyDiscord};

    const VICTIM: UserId = UserId::new(20);
    const VC: ChannelId = ChannelId::new(5);
    const MY_VC: ChannelId = ChannelId::new(6);
    const JAIL: ChannelId = ChannelId::new(7);

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
            can_join_destination: Some(true),
            current_nick: None,
            display_name: "disp".to_string(),
        }
    }

    /// The buyer seen as a target.
    fn me() -> TargetFacts {
        TargetFacts {
            is_self: true,
            in_voice: Some(MY_VC),
            current_nick: Some("mine".to_string()),
            display_name: "me".to_string(),
            ..clean()
        }
    }

    fn req(kind: ActionKind) -> BuyRequest {
        BuyRequest {
            guild: G,
            buyer: U,
            kind,
            target: if kind == ActionKind::Mirror {
                None
            } else {
                Some(VICTIM)
            },
            nick: (kind == ActionKind::Rename).then(|| "Bean".to_string()),
            note: None,
            signed: false,
            now: t0(),
        }
    }

    async fn buy_it(
        data: &Data,
        fake: &FakeEconomyDiscord,
        facts: TargetFacts,
        r: BuyRequest,
    ) -> Result<Bought, BuyError> {
        buy(data, fake, facts, me(), Some(MY_VC), r).await
    }

    fn set_jail(data: &Data, jail: Option<ChannelId>) {
        let mut s = data.economy.settings(G);
        s.config.jail_channel_id = jail;
        data.economy.set_settings(s);
    }

    /// The victim buys a mirror, so it stands for them.
    async fn victim_mirrors(pool: &PgPool, data: &Data, fake: &FakeEconomyDiscord) {
        db::credit_earnings(
            pool,
            G,
            &[db::EarnRow {
                user: VICTIM,
                voice: 150,
                messages: 0,
            }],
        )
        .await
        .unwrap();
        let r = BuyRequest {
            buyer: VICTIM,
            ..req(ActionKind::Mirror)
        };
        buy(data, fake, clean(), clean(), None, r).await.unwrap();
    }

    async fn state_and_reason(pool: &PgPool, id: i64) -> (String, Option<String>) {
        sqlx::query_as("SELECT state, end_reason FROM economy_actions WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    db_test!(
        a_rename_charges_sets_the_nick_and_activates,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            let facts = TargetFacts {
                current_nick: Some("old".to_string()),
                ..clean()
            };
            let bought = buy_it(&data, &fake, facts, req(ActionKind::Rename))
                .await
                .unwrap();
            let until = t0() + chrono::Duration::hours(24);
            assert_eq!(bought.outcome, BuyOutcome::Landed);
            assert_eq!(bought.balance, 380);
            assert_eq!(bought.until, Some(until));
            assert_eq!(
                fake.calls(),
                vec![Call::Nick(VICTIM, Some("Bean".into()), ActionKind::Rename)]
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 380);
            let rows = db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Rename])
                .await
                .unwrap();
            let [r] = rows.as_slice() else {
                panic!("{rows:?}")
            };
            assert_eq!(r.id, bought.action_id);
            assert_eq!(r.state, ActionState::Active);
            assert_eq!(r.expires_at, Some(until));
            assert_eq!(r.buyer, U);
            assert_eq!(
                r.payload,
                ActionPayload::Rename {
                    name: "Bean".into(),
                    restore: Some("old".into())
                }
            );
        }
    );

    db_test!(refused_purchases_charge_nothing, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        set_jail(&data, Some(JAIL));
        let fake = FakeEconomyDiscord::default();
        let k = ActionKind::Rename;
        let cases: Vec<(&str, ActionKind, TargetFacts, Refusal)> = vec![
            (
                "bot",
                k,
                TargetFacts {
                    is_bot: true,
                    ..clean()
                },
                Refusal::TargetIsBot,
            ),
            (
                "self",
                k,
                TargetFacts {
                    is_self: true,
                    ..clean()
                },
                Refusal::TargetIsSelf,
            ),
            (
                "out of reach",
                k,
                TargetFacts {
                    top_role_position: Some(9),
                    ..clean()
                },
                Refusal::OutOfReach,
            ),
            (
                "not in voice",
                ActionKind::Mute,
                TargetFacts {
                    in_voice: None,
                    ..clean()
                },
                Refusal::TargetNotInVoice,
            ),
            (
                "opted out",
                k,
                TargetFacts {
                    opted_out: true,
                    ..clean()
                },
                Refusal::TargetOptedOut,
            ),
            (
                "immune",
                k,
                TargetFacts {
                    protection: Some(Protection::Indefinitely),
                    ..clean()
                },
                Refusal::TargetImmune(Protection::Indefinitely),
            ),
            (
                "immune until",
                k,
                TargetFacts {
                    protection: Some(Protection::Until(t0())),
                    ..clean()
                },
                Refusal::TargetImmune(Protection::Until(t0())),
            ),
            (
                "already running",
                k,
                TargetFacts {
                    running_same_kind: Some(t0()),
                    ..clean()
                },
                Refusal::AlreadyRunning { until: t0() },
            ),
            (
                "already there",
                ActionKind::Drag,
                TargetFacts {
                    in_voice: Some(MY_VC),
                    ..clean()
                },
                Refusal::AlreadyThere,
            ),
            (
                "cannot join",
                ActionKind::Drag,
                TargetFacts {
                    can_join_destination: Some(false),
                    ..clean()
                },
                Refusal::CannotJoinDestination,
            ),
        ];
        for (name, kind, facts, want) in cases {
            assert_eq!(
                buy_it(&data, &fake, facts, req(kind)).await,
                Err(BuyError::Refused(want)),
                "{name}"
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500, "{name}");
        }
        // A drag needs the buyer in voice too.
        assert_eq!(
            buy(&data, &fake, clean(), me(), None, req(ActionKind::Drag)).await,
            Err(BuyError::Refused(Refusal::BuyerNotInVoice))
        );
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
        // Jail without a channel, and a second mirror.
        set_jail(&data, None);
        assert_eq!(
            buy_it(&data, &fake, clean(), req(ActionKind::Jail)).await,
            Err(BuyError::Refused(Refusal::NoJailChannel))
        );
        buy_it(&data, &fake, clean(), req(ActionKind::Mirror))
            .await
            .unwrap();
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 350);
        assert_eq!(
            buy_it(&data, &fake, clean(), req(ActionKind::Mirror)).await,
            Err(BuyError::Refused(Refusal::MirrorAlreadyUp))
        );
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 350);
        // The only things written: the one mirror.
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            1
        );
        assert_eq!(count(&pool, "SELECT count(*) FROM economy_ledger").await, 1);
        assert_eq!(fake.calls(), vec![]);
    });

    /// A moderator already server-muted (or deafened) the target: the shop's
    /// mute is refused before any charge, so its undo can never lift theirs.
    async fn a_moderators_flag_cannot_be_bought_off(pool: PgPool, kind: ActionKind) {
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        let (flagged, want) = match kind {
            ActionKind::Mute => (
                TargetFacts {
                    server_mute: true,
                    ..clean()
                },
                Refusal::AlreadyMuted,
            ),
            _ => (
                TargetFacts {
                    server_deaf: true,
                    ..clean()
                },
                Refusal::AlreadyDeafened,
            ),
        };
        assert_eq!(
            buy_it(&data, &fake, flagged.clone(), req(kind)).await,
            Err(BuyError::Refused(want))
        );
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            0
        );
        assert_eq!(count(&pool, "SELECT count(*) FROM economy_ledger").await, 0);
        assert_eq!(fake.calls(), vec![]);
        assert_eq!(
            refusal_text(&want, "credits"),
            if kind == ActionKind::Mute {
                "They're already server-muted."
            } else {
                "They're already server-deafened."
            }
        );

        // The bounce goes through the buyer's facts: a buyer a moderator
        // muted is blocked, and their mute is not touched either.
        victim_mirrors(&pool, &data, &fake).await;
        let mirrored = TargetFacts {
            has_mirror: true,
            ..clean()
        };
        let flagged_me = TargetFacts {
            server_mute: flagged.server_mute,
            server_deaf: flagged.server_deaf,
            ..me()
        };
        let bought = buy(&data, &fake, mirrored, flagged_me, Some(MY_VC), req(kind))
            .await
            .unwrap();
        assert_eq!(bought.outcome, BuyOutcome::Blocked);
        assert_eq!(fake.calls(), vec![], "nothing is done to anyone");
    }

    db_test!(
        a_running_shop_mute_says_until_when_and_charges_nothing,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            for (kind, flagged, theirs) in [
                (
                    ActionKind::Mute,
                    TargetFacts {
                        server_mute: true,
                        ..clean()
                    },
                    Refusal::AlreadyMuted,
                ),
                (
                    ActionKind::Deafen,
                    TargetFacts {
                        server_deaf: true,
                        ..clean()
                    },
                    Refusal::AlreadyDeafened,
                ),
            ] {
                let first = buy_it(&data, &fake, clean(), req(kind)).await.unwrap();
                let until = first.until.unwrap();
                let held = db::balance(&pool, G, U).await.unwrap();
                let all_rows = "SELECT count(*) FROM economy_actions";
                let before = count(&pool, all_rows).await;
                // The shop's own mute: already running, until its end.
                let shop_muted = TargetFacts {
                    running_same_kind: Some(until),
                    ..flagged.clone()
                };
                assert_eq!(
                    buy_it(&data, &fake, shop_muted, req(kind)).await,
                    Err(BuyError::Refused(Refusal::AlreadyRunning { until })),
                    "{kind:?}"
                );
                // A moderator's, with no shop row: theirs.
                assert_eq!(
                    buy_it(&data, &fake, flagged, req(kind)).await,
                    Err(BuyError::Refused(theirs)),
                    "{kind:?}"
                );
                assert_eq!(db::balance(&pool, G, U).await.unwrap(), held, "{kind:?}");
                assert_eq!(count(&pool, all_rows).await, before, "{kind:?}");
            }
            assert_eq!(fake.calls().len(), 2, "only the two first purchases");
        }
    );

    db_test!(a_moderators_mute_cannot_be_bought_off, |pool: PgPool| {
        a_moderators_flag_cannot_be_bought_off(pool, ActionKind::Mute)
    });

    db_test!(a_moderators_deafen_cannot_be_bought_off, |pool: PgPool| {
        a_moderators_flag_cannot_be_bought_off(pool, ActionKind::Deafen)
    });

    db_test!(a_discord_failure_refunds, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
        assert_eq!(
            buy_it(&data, &fake, clean(), req(ActionKind::Rename)).await,
            Err(BuyError::Discord(EconDiscordError::MissingPermissions))
        );
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'failed'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_ledger WHERE kind = 'refund'"
            )
            .await,
            1
        );
        assert!(db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Rename])
            .await
            .unwrap()
            .is_empty());
    });

    db_test!(
        a_mirror_bounces_the_rename_onto_the_buyer,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            victim_mirrors(&pool, &data, &fake).await;
            let facts = TargetFacts {
                has_mirror: true,
                ..clean()
            };
            let bought = buy_it(&data, &fake, facts, req(ActionKind::Rename))
                .await
                .unwrap();
            assert_eq!(bought.outcome, BuyOutcome::Bounced);
            assert_eq!(bought.balance, 380);
            assert_eq!(bought.until, Some(t0() + chrono::Duration::hours(24)));
            // The buyer's chosen name went on the buyer, and nobody else.
            assert_eq!(
                fake.calls(),
                vec![Call::Nick(U, Some("Bean".into()), ActionKind::Rename)]
            );
            // The mirror is spent.
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions
                     WHERE kind = 'mirror' AND state = 'ended' AND end_reason = 'fired'"
                )
                .await,
                1
            );
            // The row is on the buyer, restoring the buyer's own name.
            let rows = db::active_of_kind(&pool, G, U, &[ActionKind::Rename])
                .await
                .unwrap();
            let [r] = rows.as_slice() else {
                panic!("{rows:?}")
            };
            assert_eq!(
                r.payload,
                ActionPayload::Rename {
                    name: "Bean".into(),
                    restore: Some("mine".into())
                }
            );
            assert!(db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Rename])
                .await
                .unwrap()
                .is_empty());
        }
    );

    db_test!(
        a_mirror_blocks_what_cannot_land_on_the_buyer_and_still_charges,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            let mirrored = TargetFacts {
                has_mirror: true,
                ..clean()
            };

            // A drag is always blocked, even with a buyer who could take it.
            victim_mirrors(&pool, &data, &fake).await;
            let bought = buy_it(&data, &fake, mirrored.clone(), req(ActionKind::Drag))
                .await
                .unwrap();
            assert_eq!(bought.outcome, BuyOutcome::Blocked);
            assert_eq!(bought.balance, 440);
            assert_eq!(bought.until, None);
            assert_eq!(
                state_and_reason(&pool, bought.action_id).await,
                ("done".to_string(), Some("blocked".to_string()))
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 440);

            // An immune buyer cannot take a bounced clown either.
            victim_mirrors(&pool, &data, &fake).await;
            let immune_me = TargetFacts {
                protection: Some(Protection::Indefinitely),
                ..me()
            };
            let bought = buy(
                &data,
                &fake,
                mirrored,
                immune_me,
                Some(MY_VC),
                req(ActionKind::Clown),
            )
            .await
            .unwrap();
            assert_eq!(bought.outcome, BuyOutcome::Blocked);
            assert_eq!(bought.balance, 410);
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 410);
            assert_eq!(
                state_and_reason(&pool, bought.action_id).await,
                ("done".to_string(), Some("blocked".to_string()))
            );
            // Nothing was done in Discord, and both mirrors fired.
            assert_eq!(fake.calls(), vec![]);
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions
                     WHERE kind = 'mirror' AND end_reason = 'fired'"
                )
                .await,
                2
            );
        }
    );

    db_test!(a_mirror_already_spent_refunds, |pool: PgPool| async {
        // The facts say a mirror stands, but another buyer spent it first.
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        let stale = TargetFacts {
            has_mirror: true,
            ..clean()
        };
        assert_eq!(
            buy_it(&data, &fake, stale, req(ActionKind::Rename)).await,
            Err(BuyError::Db)
        );
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(fake.calls(), vec![]);
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'failed'"
            )
            .await,
            1
        );
    });

    db_test!(a_lock_ends_with_the_running_rename, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        // No name prank running: a full rename_hours from now.
        let facts = TargetFacts {
            current_nick: Some("keep".to_string()),
            ..clean()
        };
        let alone = buy_it(&data, &fake, facts.clone(), req(ActionKind::Lock))
            .await
            .unwrap();
        assert_eq!(alone.until, Some(t0() + chrono::Duration::hours(24)));
        // A lock changes nobody in Discord.
        assert_eq!(fake.calls(), vec![]);
        let rows = db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Lock])
            .await
            .unwrap();
        assert_eq!(
            rows[0].payload,
            ActionPayload::Lock {
                locked: Some("keep".into())
            }
        );
        sqlx::query("UPDATE economy_actions SET state = 'ended'")
            .execute(&pool)
            .await
            .unwrap();

        // A rename started now; the lock bought an hour later ends with it.
        buy_it(&data, &fake, facts.clone(), req(ActionKind::Rename))
            .await
            .unwrap();
        let later = BuyRequest {
            now: t0() + chrono::Duration::hours(1),
            ..req(ActionKind::Lock)
        };
        let lock = buy_it(&data, &fake, facts, later).await.unwrap();
        assert_eq!(lock.until, Some(t0() + chrono::Duration::hours(24)));
    });

    db_test!(each_kind_acts_through_the_seam, |pool: PgPool| async {
        let data = shop_data(&pool, 1000).await;
        set_jail(&data, Some(JAIL));
        let fake = FakeEconomyDiscord::default();
        for kind in [
            ActionKind::Clown,
            ActionKind::Drag,
            ActionKind::Mute,
            ActionKind::Deafen,
            ActionKind::Jail,
        ] {
            buy_it(&data, &fake, clean(), req(kind)).await.unwrap();
        }
        assert_eq!(
            fake.calls(),
            vec![
                Call::Nick(VICTIM, Some("disp \u{1F921}".into()), ActionKind::Clown),
                Call::Move(VICTIM, MY_VC, ActionKind::Drag),
                Call::Mute(VICTIM, true, ActionKind::Mute),
                Call::Deaf(VICTIM, true, ActionKind::Deafen),
                Call::Move(VICTIM, JAIL, ActionKind::Jail),
            ]
        );
        // Drag is instant: done. The rest run until they expire.
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'done' AND kind = 'drag'"
            )
            .await,
            1
        );
        assert_eq!(
            count(
                &pool,
                "SELECT count(*) FROM economy_actions WHERE state = 'active' AND expires_at IS NOT NULL"
            )
            .await,
            4
        );
        let mute = db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Mute])
            .await
            .unwrap();
        assert_eq!(
            mute[0].expires_at,
            Some(t0() + chrono::Duration::minutes(5))
        );
        let jail = db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Jail])
            .await
            .unwrap();
        assert_eq!(jail[0].payload, ActionPayload::Jail { jail: JAIL });
    });

    db_test!(a_mirror_stands_until_it_fires, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        buy_it(&data, &fake, clean(), req(ActionKind::Mirror))
            .await
            .unwrap();
        let rows = db::active_of_kind(&pool, G, U, &[ActionKind::Mirror])
            .await
            .unwrap();
        let [r] = rows.as_slice() else {
            panic!("{rows:?}")
        };
        assert_eq!((r.state, r.expires_at), (ActionState::Active, None));
        assert!(db::consume_mirror(&pool, G, U).await.unwrap());
        assert!(!db::consume_mirror(&pool, G, U).await.unwrap());
    });

    db_test!(cleanse_and_reveal_are_not_sold_here, |pool: PgPool| async {
        let data = shop_data(&pool, 500).await;
        let fake = FakeEconomyDiscord::default();
        for k in [
            ActionKind::Cleanse,
            ActionKind::Reveal,
            ActionKind::Immunity,
        ] {
            assert_eq!(
                buy_it(&data, &fake, clean(), req(k)).await,
                Err(BuyError::Invalid),
                "{k:?}"
            );
        }
        // Refused before any charge: no money moved and no row was written.
        assert_eq!(db::balance(&pool, G, U).await.unwrap(), 500);
        assert_eq!(
            count(&pool, "SELECT count(*) FROM economy_actions").await,
            0
        );
        assert_eq!(count(&pool, "SELECT count(*) FROM economy_ledger").await, 0);
        assert_eq!(fake.calls(), vec![]);
    });

    db_test!(
        a_bounce_that_fails_in_discord_is_blocked_and_still_charges,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            victim_mirrors(&pool, &data, &fake).await;
            fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
            let facts = TargetFacts {
                has_mirror: true,
                ..clean()
            };
            let bought = buy_it(&data, &fake, facts, req(ActionKind::Rename))
                .await
                .unwrap();
            assert_eq!(bought.outcome, BuyOutcome::Blocked);
            assert_eq!(bought.balance, 380);
            assert_eq!(bought.until, None);
            // The attempt on the buyer was made, and failed.
            assert_eq!(
                fake.calls(),
                vec![Call::Nick(U, Some("Bean".into()), ActionKind::Rename)]
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 380);
            assert_eq!(
                state_and_reason(&pool, bought.action_id).await,
                ("done".to_string(), Some("blocked".to_string()))
            );
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_ledger WHERE kind = 'refund'"
                )
                .await,
                0
            );
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions
                     WHERE kind = 'mirror' AND end_reason = 'fired'"
                )
                .await,
                1
            );
        }
    );

    db_test!(
        a_bounce_on_a_buyer_with_their_own_mirror_and_immunity_is_blocked,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            // The buyer holds a mirror of their own (150), and so does the victim.
            buy_it(&data, &fake, clean(), req(ActionKind::Mirror))
                .await
                .unwrap();
            victim_mirrors(&pool, &data, &fake).await;
            let buyer_facts = TargetFacts {
                has_mirror: true,
                protection: Some(Protection::Indefinitely),
                ..me()
            };
            let facts = TargetFacts {
                has_mirror: true,
                ..clean()
            };
            let bought = buy(
                &data,
                &fake,
                facts,
                buyer_facts,
                Some(MY_VC),
                req(ActionKind::Clown),
            )
            .await
            .unwrap();
            // A bounce never bounces again: the immune buyer is blocked.
            assert_eq!(bought.outcome, BuyOutcome::Blocked);
            assert_eq!(fake.calls(), vec![]);
            // The victim's mirror fired; the buyer's own stands.
            let own = db::active_of_kind(&pool, G, U, &[ActionKind::Mirror])
                .await
                .unwrap();
            assert_eq!(own.len(), 1);
            assert_eq!(own[0].state, ActionState::Active);
            assert!(db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Mirror])
                .await
                .unwrap()
                .is_empty());
        }
    );

    db_test!(
        a_second_name_prank_keeps_the_first_restore,
        |pool: PgPool| async {
            let data = shop_data(&pool, 1000).await;
            let fake = FakeEconomyDiscord::default();
            for first_restore in [Some("orig".to_string()), None] {
                sqlx::query("DELETE FROM economy_actions")
                    .execute(&pool)
                    .await
                    .unwrap();
                let first = TargetFacts {
                    current_nick: first_restore.clone(),
                    ..clean()
                };
                buy_it(&data, &fake, first, req(ActionKind::Rename))
                    .await
                    .unwrap();
                // The target now shows the prank's name; a clown must not
                // capture that as the name to restore.
                let second = TargetFacts {
                    current_nick: Some("Bean".to_string()),
                    ..clean()
                };
                buy_it(&data, &fake, second, req(ActionKind::Clown))
                    .await
                    .unwrap();
                let rows = db::active_of_kind(&pool, G, VICTIM, &[ActionKind::Clown])
                    .await
                    .unwrap();
                let [r] = rows.as_slice() else {
                    panic!("{rows:?}")
                };
                assert_eq!(
                    r.payload,
                    ActionPayload::Clown {
                        applied: "disp \u{1F921}".into(),
                        restore: first_restore.clone()
                    },
                    "{first_restore:?}"
                );
            }
        }
    );

    db_test!(
        a_lock_ends_only_with_an_active_name_prank,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            let fake = FakeEconomyDiscord::default();
            buy_it(&data, &fake, clean(), req(ActionKind::Rename))
                .await
                .unwrap();
            // The rename is winding down: it no longer sets the lock's end.
            sqlx::query("UPDATE economy_actions SET state = 'ending'")
                .execute(&pool)
                .await
                .unwrap();
            let later = BuyRequest {
                now: t0() + chrono::Duration::hours(1),
                ..req(ActionKind::Lock)
            };
            let lock = buy_it(&data, &fake, clean(), later).await.unwrap();
            assert_eq!(lock.until, Some(t0() + chrono::Duration::hours(25)));
        }
    );

    #[tokio::test]
    async fn without_a_pool_buying_says_no_database() {
        let data = Data(Arc::new(DataInner::default()));
        assert_eq!(
            buy_immunity(&data, G, U, t0()).await,
            Err(BuyError::NoDatabase)
        );
    }

    // ---- preflight, wording ----

    fn perms(guild: Permissions, chans: &[(ChannelId, Permissions)]) -> BotPerms {
        BotPerms {
            guild,
            in_channel: chans.iter().copied().collect(),
        }
    }

    #[test]
    fn preflight_refuses_with_the_missing_permissions_in_words() {
        let nick = Permissions::MANAGE_NICKNAMES;
        let none = Permissions::empty();
        // Rename needs Manage Nicknames.
        assert_eq!(
            preflight_refusal(ActionKind::Rename, Some(&perms(none, &[])), &[]),
            Err(Refusal::Unavailable { missing: nick })
        );
        assert_eq!(
            preflight_refusal(ActionKind::Rename, Some(&perms(nick, &[])), &[]),
            Ok(())
        );
        // Lock also needs the audit log.
        let text = refusal_text(
            &preflight_refusal(ActionKind::Lock, Some(&perms(nick, &[])), &[]).unwrap_err(),
            "credits",
        );
        assert!(text.contains("View Audit Log"), "{text}");
        // A drag needs both channels: the one missing is named.
        let both = Permissions::VIEW_CHANNEL | Permissions::CONNECT;
        let have = perms(
            Permissions::MOVE_MEMBERS,
            &[(VC, both), (MY_VC, Permissions::VIEW_CHANNEL)],
        );
        assert_eq!(
            preflight_refusal(ActionKind::Drag, Some(&have), &[VC, MY_VC]),
            Err(Refusal::Unavailable {
                missing: Permissions::CONNECT
            })
        );
        // Jail checks the jail channel.
        assert_eq!(
            preflight_refusal(
                ActionKind::Jail,
                Some(&perms(Permissions::MOVE_MEMBERS, &[])),
                &[JAIL]
            ),
            Err(Refusal::Unavailable { missing: both })
        );
        // The cache cannot say: fail closed.
        assert_eq!(
            preflight_refusal(ActionKind::Mute, None, &[]),
            Err(Refusal::Unavailable {
                missing: Permissions::MUTE_MEMBERS
            })
        );
        // Defence and unmasking need nothing.
        for k in [ActionKind::Mirror, ActionKind::Cleanse, ActionKind::Reveal] {
            assert_eq!(preflight_refusal(k, None, &[]), Ok(()), "{k:?}");
        }
    }

    #[test]
    fn ago_words_reads_as_plain_text() {
        let now = t0();
        let ago = |secs: i64| ago_words(now, now - chrono::Duration::seconds(secs));
        assert_eq!(ago(10), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(5 * 60), "5 minutes ago");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(3 * 3600), "3 hours ago");
        assert_eq!(ago(2 * 86_400), "2 days ago");
        // A clock that ran backwards is not negative time.
        assert_eq!(
            ago_words(now, now + chrono::Duration::minutes(5)),
            "just now"
        );
    }

    // ---- cleanse and reveal ----

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

    db_test!(
        cleanse_ends_running_pranks_and_restores,
        |pool: PgPool| async {
            let data = shop_data(&pool, 1000).await;
            fund(&pool, VICTIM, 300).await;
            let fake = FakeEconomyDiscord::default();
            let facts = TargetFacts {
                current_nick: Some("old".to_string()),
                ..clean()
            };
            let rename = buy_it(&data, &fake, facts, req(ActionKind::Rename))
                .await
                .unwrap();
            let mute = buy_it(&data, &fake, clean(), req(ActionKind::Mute))
                .await
                .unwrap();
            // The victim also holds a mirror, which a cleanse keeps.
            let r = BuyRequest {
                buyer: VICTIM,
                ..req(ActionKind::Mirror)
            };
            let mirror = buy(&data, &fake, clean(), clean(), None, r).await.unwrap();
            let before = fake.calls().len();

            let cleansed = cleanse(&data, &fake, G, VICTIM, t0()).await.unwrap();
            assert_eq!(
                cleansed,
                Cleansed {
                    ended: 2,
                    balance: 70
                }
            );
            let sent: Vec<_> = fake.calls().into_iter().skip(before).collect();
            assert_eq!(sent.len(), 2, "{sent:?}");
            assert!(sent.contains(&Call::Nick(VICTIM, Some("old".into()), ActionKind::Rename)));
            assert!(sent.contains(&Call::Mute(VICTIM, false, ActionKind::Mute)));
            for id in [rename.action_id, mute.action_id] {
                assert_eq!(
                    state_and_reason(&pool, id).await,
                    ("ended".to_string(), Some("cleansed".to_string()))
                );
            }
            assert_eq!(state_and_reason(&pool, mirror.action_id).await.0, "active");
            // 80 credits left the victim's wallet, recorded against a done cleanse.
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 70);
            let (state, price): (String, i64) =
                sqlx::query_as("SELECT state, price FROM economy_actions WHERE kind = 'cleanse'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!((state.as_str(), price), ("done", 80));
        }
    );

    db_test!(
        cleanse_with_nothing_running_is_refused_before_charging,
        |pool: PgPool| async {
            let data = shop_data(&pool, 0).await;
            fund(&pool, VICTIM, 500).await;
            let fake = FakeEconomyDiscord::default();
            // A standing mirror is not a prank.
            let r = BuyRequest {
                buyer: VICTIM,
                ..req(ActionKind::Mirror)
            };
            buy(&data, &fake, clean(), clean(), None, r).await.unwrap();
            let held = db::balance(&pool, G, VICTIM).await.unwrap();
            assert_eq!(
                cleanse(&data, &fake, G, VICTIM, t0()).await,
                Err(BuyError::Refused(Refusal::NothingToCleanse))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), held);
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions WHERE kind = 'cleanse'"
                )
                .await,
                0
            );
            assert_eq!(fake.calls().len(), 0);
        }
    );

    db_test!(
        a_failed_undo_in_a_cleanse_is_retried_by_the_next_sweep_and_ends_cleansed,
        |pool: PgPool| async {
            use crate::economy::discord::fake::CallKind;
            let data = shop_data(&pool, 500).await;
            fund(&pool, VICTIM, 100).await;
            let fake = FakeEconomyDiscord::default();
            let mute = buy_it(&data, &fake, clean(), req(ActionKind::Mute))
                .await
                .unwrap();
            fake.fail_next(CallKind::Mute, EconDiscordError::MissingPermissions);
            let done = cleanse(&data, &fake, G, VICTIM, t0()).await.unwrap();
            // Charged, and the prank is scheduled rather than left to run on.
            assert_eq!((done.ended, done.balance), (1, 20));
            assert_eq!(
                state_and_reason(&pool, mute.action_id).await,
                ("active".to_string(), Some("cleansed".to_string()))
            );
            let due: Option<DateTime<Utc>> =
                sqlx::query_scalar("SELECT expires_at FROM economy_actions WHERE id = $1")
                    .bind(mute.action_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(due, Some(t0()), "the end moved up to now");
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions WHERE kind = 'cleanse' AND state = 'done'")
                    .await,
                1
            );
            // The next sweep retries the undo and ends it as cleansed.
            crate::economy::effects::sweep(&data, &fake, t0(), &|_| true).await;
            assert_eq!(
                state_and_reason(&pool, mute.action_id).await,
                ("ended".to_string(), Some("cleansed".to_string()))
            );
            let unmutes = fake
                .calls()
                .into_iter()
                .filter(|c| matches!(c, Call::Mute(_, false, _)))
                .count();
            assert_eq!(unmutes, 2, "one failed, one retried");
        }
    );

    db_test!(
        cleanse_with_only_ending_rows_is_refused_before_charging,
        |pool: PgPool| async {
            let data = shop_data(&pool, 500).await;
            fund(&pool, VICTIM, 200).await;
            let fake = FakeEconomyDiscord::default();
            // A mute already set aside for their next join (they're offline).
            buy_it(&data, &fake, clean(), req(ActionKind::Mute))
                .await
                .unwrap();
            sqlx::query("UPDATE economy_actions SET state = 'ending' WHERE kind = 'mute'")
                .execute(&pool)
                .await
                .unwrap();
            let before = fake.calls().len();
            assert_eq!(
                cleanse(&data, &fake, G, VICTIM, t0()).await,
                Err(BuyError::Refused(Refusal::NothingToCleanse))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 200);
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions WHERE kind = 'cleanse'"
                )
                .await,
                0
            );
            assert_eq!(fake.calls().len(), before);
            // Charged anyway (a race), and the member still offline: the
            // ending row stays ending, nothing active ended, so it's refunded.
            fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
            assert_eq!(
                charge_and_cleanse(&data, &fake, G, VICTIM, 80, t0()).await,
                Err(BuyError::Refused(Refusal::NothingToCleanse))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 200);
        }
    );

    db_test!(
        a_cleanse_that_finds_nothing_after_charging_is_refunded,
        |pool: PgPool| async {
            let data = shop_data(&pool, 0).await;
            fund(&pool, VICTIM, 200).await;
            let fake = FakeEconomyDiscord::default();
            // The check passed and the charge went through, but by the time the
            // pranks are ended there is nothing left (the sweeper got there).
            assert_eq!(
                charge_and_cleanse(&data, &fake, G, VICTIM, 80, t0()).await,
                Err(BuyError::Refused(Refusal::NothingToCleanse))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 200);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions WHERE kind = 'cleanse' AND state = 'failed'")
                    .await,
                1
            );
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_ledger WHERE kind = 'refund' AND delta = 80"
                )
                .await,
                1
            );
        }
    );

    /// `U` pranks `VICTIM` with a rename, a mute, then a signed clown.
    async fn three_pranks(data: &Data, fake: &FakeEconomyDiscord) -> (i64, i64, i64) {
        let rename = buy_it(data, fake, clean(), req(ActionKind::Rename))
            .await
            .unwrap();
        let mute = buy_it(data, fake, clean(), req(ActionKind::Mute))
            .await
            .unwrap();
        let signed = BuyRequest {
            signed: true,
            ..req(ActionKind::Clown)
        };
        let clown = buy_it(data, fake, clean(), signed).await.unwrap();
        (rename.action_id, mute.action_id, clown.action_id)
    }

    async fn revealed(pool: &PgPool, id: i64) -> (bool, Option<i64>) {
        sqlx::query_as(
            "SELECT revealed_at IS NOT NULL, revealed_by FROM economy_actions WHERE id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    db_test!(
        reveal_picks_the_latest_unsigned_unrevealed,
        |pool: PgPool| async {
            let data = shop_data(&pool, 1000).await;
            fund(&pool, VICTIM, 350).await;
            let fake = FakeEconomyDiscord::default();
            let (rename, mute, clown) = three_pranks(&data, &fake).await;

            // The newest is signed (the buyer is already known): the mute is next.
            let first = reveal(&data, G, VICTIM, None).await.unwrap();
            assert_eq!(
                first,
                Revealed {
                    buyer: U,
                    kind: ActionKind::Mute,
                    balance: 250
                }
            );
            assert_eq!(
                revealed(&pool, mute).await,
                (true, Some(VICTIM.get() as i64))
            );
            assert_eq!(revealed(&pool, rename).await.0, false);
            assert_eq!(revealed(&pool, clown).await.0, false);
            // Then the rename; then nothing is left to unmask.
            let second = reveal(&data, G, VICTIM, None).await.unwrap();
            assert_eq!((second.kind, second.balance), (ActionKind::Rename, 150));
            assert_eq!(revealed(&pool, rename).await.0, true);
            assert_eq!(
                reveal(&data, G, VICTIM, None).await,
                Err(BuyError::Refused(Refusal::NothingToReveal))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 150);
            // Each reveal is its own done purchase.
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions WHERE kind = 'reveal' AND state = 'done'"
                )
                .await,
                2
            );
        }
    );

    db_test!(
        reveal_refuses_before_charging_when_nothing_to_reveal,
        |pool: PgPool| async {
            let data = shop_data(&pool, 0).await;
            fund(&pool, VICTIM, 500).await;
            assert_eq!(
                reveal(&data, G, VICTIM, None).await,
                Err(BuyError::Refused(Refusal::NothingToReveal))
            );
            assert_eq!(
                reveal(&data, G, VICTIM, Some(12345)).await,
                Err(BuyError::Refused(Refusal::NothingToReveal))
            );
            // Eligibility is judged before affordability: a broke member with
            // nothing to unmask is told so, not that they need credits.
            assert_eq!(
                reveal(&data, G, UserId::new(31), None).await,
                Err(BuyError::Refused(Refusal::NothingToReveal))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 500);
            assert_eq!(
                count(&pool, "SELECT count(*) FROM economy_actions").await,
                0
            );
        }
    );

    db_test!(
        revealing_twice_is_refused_before_charging,
        |pool: PgPool| async {
            let data = shop_data(&pool, 1000).await;
            fund(&pool, VICTIM, 300).await;
            let fake = FakeEconomyDiscord::default();
            let (rename, _mute, _clown) = three_pranks(&data, &fake).await;
            let done = reveal(&data, G, VICTIM, Some(rename)).await.unwrap();
            assert_eq!(done.balance, 200);
            assert_eq!(
                reveal(&data, G, VICTIM, Some(rename)).await,
                Err(BuyError::Refused(Refusal::AlreadyRevealed))
            );
            assert_eq!(db::balance(&pool, G, VICTIM).await.unwrap(), 200);
            assert_eq!(
                count(
                    &pool,
                    "SELECT count(*) FROM economy_actions WHERE kind = 'reveal'"
                )
                .await,
                1
            );
        }
    );

    db_test!(
        reveal_refuses_an_action_aimed_at_someone_else,
        |pool: PgPool| async {
            let data = shop_data(&pool, 1000).await;
            let bystander = UserId::new(30);
            fund(&pool, bystander, 500).await;
            let fake = FakeEconomyDiscord::default();
            let (rename, _mute, clown) = three_pranks(&data, &fake).await;
            // Picking the victim's prank, signed or not, unmasks nothing.
            for id in [rename, clown] {
                assert_eq!(
                    reveal(&data, G, bystander, Some(id)).await,
                    Err(BuyError::Refused(Refusal::NothingToReveal))
                );
            }
            assert_eq!(db::balance(&pool, G, bystander).await.unwrap(), 500);
            assert_eq!(revealed(&pool, rename).await.0, false);
            // Their own latest, by default, is still nothing: no prank is aimed at them.
            assert_eq!(
                reveal(&data, G, bystander, None).await,
                Err(BuyError::Refused(Refusal::NothingToReveal))
            );
        }
    );

    db_test!(reveal_ignores_bounces_and_mirrors, |pool: PgPool| async {
        let data = shop_data(&pool, 1000).await;
        let fake = FakeEconomyDiscord::default();
        victim_mirrors(&pool, &data, &fake).await;
        // U's rename bounces onto U: a row whose buyer is its own target.
        let bounced = buy_it(
            &data,
            &fake,
            TargetFacts {
                has_mirror: true,
                ..clean()
            },
            req(ActionKind::Rename),
        )
        .await
        .unwrap();
        assert_eq!(bounced.outcome, BuyOutcome::Bounced);
        fund(&pool, VICTIM, 300).await;
        let mirror_row: i64 =
            sqlx::query_scalar("SELECT id FROM economy_actions WHERE kind = 'mirror'")
                .fetch_one(&pool)
                .await
                .unwrap();
        for (who, pick) in [
            (U, Some(bounced.action_id)),
            (U, None),
            (VICTIM, Some(mirror_row)),
            (VICTIM, None),
        ] {
            assert_eq!(
                reveal(&data, G, who, pick).await,
                Err(BuyError::Refused(Refusal::NothingToReveal)),
                "{who} {pick:?}"
            );
        }
    });

    db_test!(
        reveal_choices_lists_the_latest_ten_unsigned,
        |pool: PgPool| async {
            let data = shop_data(&pool, 5000).await;
            let fake = FakeEconomyDiscord::default();
            let mut ids = Vec::new();
            for _ in 0..12 {
                sqlx::query("UPDATE economy_actions SET state = 'ended'")
                    .execute(&pool)
                    .await
                    .unwrap();
                let b = buy_it(&data, &fake, clean(), req(ActionKind::Deafen))
                    .await
                    .unwrap();
                ids.push(b.action_id);
            }
            let signed = BuyRequest {
                signed: true,
                ..req(ActionKind::Deafen)
            };
            buy_it(&data, &fake, clean(), signed).await.unwrap();
            // The two newest unsigned ones are already unmasked.
            let mut conn = pool.acquire().await.unwrap();
            for id in &ids[10..] {
                assert!(db::mark_revealed(&mut conn, *id, VICTIM).await.unwrap());
            }
            drop(conn);
            let picks = reveal_choices(&data, G, VICTIM, 10).await;
            let got: Vec<i64> = picks.iter().map(|c| c.row.id).collect();
            let want: Vec<i64> = ids[..10].iter().rev().copied().collect();
            assert_eq!(
                got, want,
                "the ten newest unsigned, unrevealed, newest first"
            );
        }
    );

    db_test!(reveal_finds_a_blocked_prank, |pool: PgPool| async {
        let data = shop_data(&pool, 1000).await;
        let fake = FakeEconomyDiscord::default();
        victim_mirrors(&pool, &data, &fake).await;
        fund(&pool, VICTIM, 200).await;
        // The buyer is immune, so the bounce cannot land: blocked.
        let immune = TargetFacts {
            protection: Some(Protection::Indefinitely),
            ..me()
        };
        let mirrored = TargetFacts {
            has_mirror: true,
            ..clean()
        };
        let blocked = buy(
            &data,
            &fake,
            mirrored,
            immune,
            Some(MY_VC),
            req(ActionKind::Rename),
        )
        .await
        .unwrap();
        assert_eq!(blocked.outcome, BuyOutcome::Blocked);
        // It was still aimed at the mirror's owner, who can unmask it.
        let done = reveal(&data, G, VICTIM, None).await.unwrap();
        assert_eq!((done.buyer, done.kind), (U, ActionKind::Rename));
        assert_eq!(revealed(&pool, blocked.action_id).await.0, true);
    });

    db_test!(marking_a_prank_revealed_works_once, |pool: PgPool| async {
        let data = shop_data(&pool, 1000).await;
        let fake = FakeEconomyDiscord::default();
        let (rename, mute, _clown) = three_pranks(&data, &fake).await;
        let mut conn = pool.acquire().await.unwrap();
        assert!(db::mark_revealed(&mut conn, rename, VICTIM).await.unwrap());
        assert!(
            !db::mark_revealed(&mut conn, rename, UserId::new(31))
                .await
                .unwrap(),
            "a second reveal finds it already unmasked"
        );
        drop(conn);
        assert_eq!(
            revealed(&pool, rename).await,
            (true, Some(VICTIM.get() as i64))
        );
        // The whole-purchase write refuses a repeat too, and writes nothing.
        assert!(!db::reveal_action(&pool, 0, rename, VICTIM).await.unwrap());
        assert!(db::reveal_action(&pool, 0, mute, VICTIM).await.unwrap());
    });

    db_test!(
        a_balance_that_moved_before_the_charge_reads_as_cant_afford,
        |pool: PgPool| async {
            let _data = shop_data(&pool, 50).await;
            let action = db::NewAction {
                guild: G,
                kind: ActionKind::Cleanse,
                buyer: U,
                target: Some(U),
                price: 80,
                note: None,
                signed: false,
                payload: &ActionPayload::Cleanse,
                expires_at: None,
            };
            assert_eq!(
                charge(&pool, G, U, 80, &action).await.map(|c| c.balance),
                Err(BuyError::Refused(Refusal::CantAfford { short: 30 }))
            );
            assert_eq!(db::balance(&pool, G, U).await.unwrap(), 50);
        }
    );
}
