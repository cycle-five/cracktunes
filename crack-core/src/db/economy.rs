//! Economy settings as a row per server, and the opted-out set. See `economy`.

use crate::economy::immunity::{Grant, GrantSource};
use crate::economy::settings::{ActionKind, EconomyConfig, EconomySettings};
use crate::economy::wallet::{ActionPayload, ActionState, LedgerKind, MoveError};
use chrono::{DateTime, Utc};
use serenity::all::{GuildId, UserId};
use sqlx::{types::Json, PgPool};
use std::collections::HashSet;

pub async fn load_settings(pool: &PgPool, guild: GuildId) -> sqlx::Result<Option<EconomySettings>> {
    let row = sqlx::query!(
        r#"SELECT enabled, config AS "config: Json<EconomyConfig>"
           FROM economy_settings WHERE guild_id = $1"#,
        guild.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| EconomySettings {
        guild,
        enabled: r.enabled,
        config: r.config.0,
    }))
}

pub async fn save_settings(pool: &PgPool, s: &EconomySettings) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO economy_settings (guild_id, enabled, config)
           VALUES ($1, $2, $3)
           ON CONFLICT (guild_id) DO UPDATE SET
               enabled = EXCLUDED.enabled,
               config = EXCLUDED.config,
               updated_at = now()"#,
        s.guild.get() as i64,
        s.enabled,
        Json(&s.config) as Json<&EconomyConfig>,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The members of a server whose wallet is opted out.
pub async fn load_opted_out(pool: &PgPool, guild: GuildId) -> sqlx::Result<HashSet<UserId>> {
    let rows = sqlx::query!(
        r#"SELECT user_id FROM economy_wallets
           WHERE guild_id = $1 AND opted_out_at IS NOT NULL"#,
        guild.get() as i64
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| u64::try_from(r.user_id).ok().filter(|v| *v != 0))
        .map(UserId::new)
        .collect())
}

/// A member's earnings over one tick, in credits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarnRow {
    pub user: UserId,
    pub voice: i64,
    pub messages: i64,
}

fn uid(raw: i64) -> Option<UserId> {
    u64::try_from(raw)
        .ok()
        .filter(|v| *v != 0 && *v != u64::MAX)
        .map(UserId::new)
}

/// Add earnings to wallets in one statement, creating the missing ones. The
/// credits go to the balance and to the counters, never to the ledger.
pub async fn credit_earnings(pool: &PgPool, guild: GuildId, rows: &[EarnRow]) -> sqlx::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let users: Vec<i64> = rows.iter().map(|r| r.user.get() as i64).collect();
    let voice: Vec<i64> = rows.iter().map(|r| r.voice).collect();
    let messages: Vec<i64> = rows.iter().map(|r| r.messages).collect();
    sqlx::query!(
        r#"INSERT INTO economy_wallets AS w (guild_id, user_id, balance, earned_voice, earned_messages)
           SELECT $1, t.u, sum(t.v + t.m), sum(t.v), sum(t.m)
           FROM UNNEST($2::bigint[], $3::bigint[], $4::bigint[]) AS t(u, v, m)
           GROUP BY t.u
           ORDER BY t.u
           ON CONFLICT (guild_id, user_id) DO UPDATE SET
               balance = w.balance + EXCLUDED.balance,
               earned_voice = w.earned_voice + EXCLUDED.earned_voice,
               earned_messages = w.earned_messages + EXCLUDED.earned_messages"#,
        guild.get() as i64,
        &users,
        &voice,
        &messages,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// What a member holds; 0 when there is no wallet.
pub async fn balance(pool: &PgPool, guild: GuildId, user: UserId) -> sqlx::Result<i64> {
    let row = sqlx::query!(
        "SELECT balance FROM economy_wallets WHERE guild_id = $1 AND user_id = $2",
        guild.get() as i64,
        user.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or(0, |r| r.balance))
}

/// A member's balance and what they earned, by source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletSummary {
    pub balance: i64,
    pub earned_voice: i64,
    pub earned_messages: i64,
}

/// A member's balance and earning counters; all zeros when there is no wallet.
pub async fn wallet_summary(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
) -> sqlx::Result<WalletSummary> {
    let row = sqlx::query!(
        "SELECT balance, earned_voice, earned_messages FROM economy_wallets
         WHERE guild_id = $1 AND user_id = $2",
        guild.get() as i64,
        user.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or(
        WalletSummary {
            balance: 0,
            earned_voice: 0,
            earned_messages: 0,
        },
        |r| WalletSummary {
            balance: r.balance,
            earned_voice: r.earned_voice,
            earned_messages: r.earned_messages,
        },
    ))
}

/// Proof that a wallet row is locked in the current transaction. Only
/// [`lock_wallet`] makes one, and [`set_balance`] needs one, so a balance
/// cannot be written without the row having been locked first.
struct Locked {
    guild: GuildId,
    user: UserId,
    balance: i64,
}

/// Create the wallet if missing, then lock it. The lock is held until the
/// transaction ends, which is what serialises debits.
async fn lock_wallet(
    tx: &mut sqlx::PgConnection,
    guild: GuildId,
    user: UserId,
) -> sqlx::Result<Locked> {
    sqlx::query!(
        "INSERT INTO economy_wallets (guild_id, user_id) VALUES ($1, $2)
         ON CONFLICT (guild_id, user_id) DO NOTHING",
        guild.get() as i64,
        user.get() as i64
    )
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query!(
        "SELECT balance FROM economy_wallets WHERE guild_id = $1 AND user_id = $2 FOR UPDATE",
        guild.get() as i64,
        user.get() as i64
    )
    .fetch_one(&mut *tx)
    .await?;
    Ok(Locked {
        guild,
        user,
        balance: row.balance,
    })
}

async fn set_balance(
    tx: &mut sqlx::PgConnection,
    wallet: &Locked,
    new_balance: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE economy_wallets SET balance = $3 WHERE guild_id = $1 AND user_id = $2",
        wallet.guild.get() as i64,
        wallet.user.get() as i64,
        new_balance
    )
    .execute(tx)
    .await?;
    Ok(())
}

async fn write_ledger(
    tx: &mut sqlx::PgConnection,
    guild: GuildId,
    user: UserId,
    delta: i64,
    kind: LedgerKind,
    action_id: Option<i64>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO economy_ledger (guild_id, user_id, delta, kind, action_id)
         VALUES ($1, $2, $3, $4, $5)",
        guild.get() as i64,
        user.get() as i64,
        delta,
        kind.as_str(),
        action_id
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// A purchase to charge for.
pub struct NewAction<'a> {
    pub guild: GuildId,
    pub kind: ActionKind,
    pub buyer: UserId,
    pub target: Option<UserId>,
    pub price: i64,
    pub note: Option<&'a str>,
    pub signed: bool,
    pub payload: &'a ActionPayload,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charged {
    pub action_id: i64,
    pub balance: i64,
}

/// Debit the buyer and record the purchase as `pending`, in one transaction.
pub async fn charge_for_action(pool: &PgPool, a: &NewAction<'_>) -> Result<Charged, MoveError> {
    charge_as(pool, a, ActionState::Pending, LedgerKind::Purchase).await
}

/// Debit the buyer and record the purchase directly as `active`, in one
/// transaction, ending at `a.expires_at`. A theme uses it with
/// [`LedgerKind::Purchase`], a bounty with [`LedgerKind::BountyPlaced`]: both
/// are charged and standing in one step, so neither is ever `pending`.
pub async fn charge_active(
    pool: &PgPool,
    a: &NewAction<'_>,
    ledger: LedgerKind,
) -> Result<Charged, MoveError> {
    // Escrow must expire, or nothing would ever refund it.
    if a.expires_at.is_none() {
        return Err(MoveError::Invalid);
    }
    charge_as(pool, a, ActionState::Active, ledger).await
}

/// The one debit-and-insert transaction behind the two charges above.
async fn charge_as(
    pool: &PgPool,
    a: &NewAction<'_>,
    state: ActionState,
    ledger: LedgerKind,
) -> Result<Charged, MoveError> {
    if a.price <= 0 {
        return Err(MoveError::Invalid);
    }
    let mut tx = pool.begin().await?;
    let wallet = lock_wallet(&mut tx, a.guild, a.buyer).await?;
    let held = wallet.balance;
    if held < a.price {
        return Err(MoveError::Insufficient { balance: held });
    }
    let left = held - a.price;
    set_balance(&mut tx, &wallet, left).await?;
    let row = sqlx::query!(
        "INSERT INTO economy_actions
             (guild_id, kind, buyer_id, target_id, price, note, signed, payload, state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id",
        a.guild.get() as i64,
        a.kind.key(),
        a.buyer.get() as i64,
        a.target.map(|t| t.get() as i64),
        a.price,
        a.note,
        a.signed,
        Json(a.payload) as Json<&ActionPayload>,
        state.as_str(),
        a.expires_at,
    )
    .fetch_one(&mut *tx)
    .await?;
    write_ledger(&mut tx, a.guild, a.buyer, -a.price, ledger, Some(row.id)).await?;
    tx.commit().await?;
    Ok(Charged {
        action_id: row.id,
        balance: left,
    })
}

/// Move an action to a new state, stamping `ended_at` when it is final. A
/// row in a final state never moves again. True when the row moved: false
/// for one already final (a theme refunded as it played).
pub async fn finish_action(
    pool: &PgPool,
    action_id: i64,
    state: ActionState,
) -> sqlx::Result<bool> {
    let moved = sqlx::query!(
        "UPDATE economy_actions
         SET state = $2, ended_at = CASE WHEN $3 THEN now() ELSE ended_at END
         WHERE id = $1 AND state NOT IN ('done', 'ended', 'failed')",
        action_id,
        state.as_str(),
        state.is_final()
    )
    .execute(pool)
    .await?;
    Ok(moved.rows_affected() > 0)
}

/// A bought action as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRow {
    pub id: i64,
    pub kind: ActionKind,
    pub buyer: UserId,
    /// A mirror's row names its owner here, so it is found like any other
    /// action aimed at that member.
    pub target: Option<UserId>,
    pub payload: ActionPayload,
    pub state: ActionState,
    pub expires_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
    pub signed: bool,
    pub revealed_at: Option<DateTime<Utc>>,
}

/// One action by id, if it belongs to `guild`. A button on a card reads the
/// action it was drawn for; an id from another server is no action here.
pub async fn action_by_id(
    pool: &PgPool,
    guild: GuildId,
    action_id: i64,
) -> sqlx::Result<Option<ActionRow>> {
    let keys = known_kinds();
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE id = $1 AND guild_id = $2 AND kind = ANY($3)"#,
        action_id,
        guild.get() as i64,
        &keys
    )
    .fetch_optional(pool)
    .await?;
    Ok(raw.and_then(RawAction::into_row).map(|(_, r)| r))
}

/// Bring an `active` row's end forward to `now` and record why, so the
/// sweeper retries its undo every tick (an early end whose undo failed). The
/// end never moves later. False when the row is not `active`.
pub async fn schedule_early_end(
    pool: &PgPool,
    action_id: i64,
    reason: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions
         SET expires_at = LEAST(COALESCE(expires_at, $3), $3), end_reason = $2
         WHERE id = $1 AND state = 'active'",
        action_id,
        reason,
        now
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// The running (`active` or `ending`) actions of these kinds aimed at
/// `target`, oldest first.
pub async fn active_of_kind(
    pool: &PgPool,
    guild: GuildId,
    target: UserId,
    kinds: &[ActionKind],
) -> sqlx::Result<Vec<ActionRow>> {
    let keys: Vec<String> = kinds.iter().map(|k| k.key().to_string()).collect();
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND target_id = $2 AND kind = ANY($3)
             AND state IN ('active', 'ending')
           ORDER BY id"#,
        guild.get() as i64,
        target.get() as i64,
        &keys
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// A prank that could be unmasked, with when it was bought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealCandidate {
    pub row: ActionRow,
    pub bought_at: DateTime<Utc>,
}

/// [`RawAction`] plus the purchase time.
struct RawCandidate {
    id: i64,
    guild_id: i64,
    kind: String,
    buyer_id: i64,
    target_id: Option<i64>,
    payload: Json<ActionPayload>,
    state: String,
    expires_at: Option<DateTime<Utc>>,
    note: Option<String>,
    signed: bool,
    revealed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

/// The revealable kinds (spec 4.1 and P4: the pranks, themes, dedications
/// and bounties) other people bought against `target` that could be
/// unmasked, as far as the target could see them: unsigned, never a `funding`
/// row, never a theme that ended without playing (a played theme is `done`)
/// and never a fund that did not fill (`unfunded`). A bounty that was
/// refunded or expired stays revealable, since its card was public. Not
/// refunded (a blocked prank was still aimed at the mirror's owner), newest
/// first. A bounced prank (its buyer is its own target) and a mirror (its
/// target is its owner) never qualify. `only` narrows to one action id, and
/// `unrevealed_only` drops the ones already unmasked.
pub async fn reveal_candidates(
    pool: &PgPool,
    guild: GuildId,
    target: UserId,
    only: Option<i64>,
    unrevealed_only: bool,
    limit: i64,
) -> sqlx::Result<Vec<RevealCandidate>> {
    let keys: Vec<String> = ActionKind::REVEALABLE
        .iter()
        .map(|k| k.key().to_string())
        .collect();
    let raw = sqlx::query_as!(
        RawCandidate,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at, created_at
           FROM economy_actions
           WHERE guild_id = $1 AND target_id = $2 AND buyer_id <> $2
             AND kind = ANY($3) AND signed = false
             AND state IN ('active', 'ending', 'ended', 'done')
             AND NOT (kind = 'theme' AND state = 'ended')
             AND end_reason IS DISTINCT FROM 'unfunded'
             AND ($4::bigint IS NULL OR id = $4)
             AND (NOT $5 OR revealed_at IS NULL)
           ORDER BY id DESC
           LIMIT $6"#,
        guild.get() as i64,
        target.get() as i64,
        &keys,
        only,
        unrevealed_only,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(raw
        .into_iter()
        .filter_map(|c| {
            let bought_at = c.created_at;
            RawAction {
                id: c.id,
                guild_id: c.guild_id,
                kind: c.kind,
                buyer_id: c.buyer_id,
                target_id: c.target_id,
                payload: c.payload,
                state: c.state,
                expires_at: c.expires_at,
                note: c.note,
                signed: c.signed,
                revealed_at: c.revealed_at,
            }
            .into_row()
            .map(|(_, row)| RevealCandidate { row, bought_at })
        })
        .collect())
}

/// An action for `/economy status`: the row, why it ended, and when it was
/// bought.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentAction {
    pub row: ActionRow,
    pub end_reason: Option<String>,
    pub bought_at: DateTime<Utc>,
}

/// [`RawAction`] plus why it ended and when it was bought.
struct RawRecent {
    id: i64,
    guild_id: i64,
    kind: String,
    buyer_id: i64,
    target_id: Option<i64>,
    payload: Json<ActionPayload>,
    state: String,
    expires_at: Option<DateTime<Utc>>,
    note: Option<String>,
    signed: bool,
    revealed_at: Option<DateTime<Utc>>,
    end_reason: Option<String>,
    created_at: DateTime<Utc>,
}

/// A server's last `limit` actions of every kind this build knows, newest
/// first, whatever their state: the record moderators read when the mod
/// log is off (spec 5.4).
pub async fn recent_actions(
    pool: &PgPool,
    guild: GuildId,
    limit: i64,
) -> sqlx::Result<Vec<RecentAction>> {
    let raw = sqlx::query_as!(
        RawRecent,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at, end_reason, created_at
           FROM economy_actions
           WHERE guild_id = $1 AND kind = ANY($2)
           ORDER BY id DESC
           LIMIT $3"#,
        guild.get() as i64,
        &known_kinds(),
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(raw.into_iter().filter_map(RawRecent::into_recent).collect())
}

impl RawRecent {
    /// As [`RawAction::into_row`], keeping why it ended and when it was bought.
    fn into_recent(self) -> Option<RecentAction> {
        let (end_reason, bought_at) = (self.end_reason, self.created_at);
        RawAction {
            id: self.id,
            guild_id: self.guild_id,
            kind: self.kind,
            buyer_id: self.buyer_id,
            target_id: self.target_id,
            payload: self.payload,
            state: self.state,
            expires_at: self.expires_at,
            note: self.note,
            signed: self.signed,
            revealed_at: self.revealed_at,
        }
        .into_row()
        .map(|(_, row)| RecentAction {
            row,
            end_reason,
            bought_at,
        })
    }
}

/// Stamp a prank as unmasked by `by`. False when it already was (nothing
/// changed), so a racing second reveal is told so and refunded.
pub async fn mark_revealed(
    conn: &mut sqlx::PgConnection,
    action_id: i64,
    by: UserId,
) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions SET revealed_at = now(), revealed_by = $2
         WHERE id = $1 AND revealed_at IS NULL",
        action_id,
        by.get() as i64
    )
    .execute(conn)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Unmask `prank` and mark the paid `reveal` purchase `done`, in one
/// transaction: a reveal left `pending` always means nothing was unmasked, so
/// reconcile's refund stays right. False (nothing written) when the prank was
/// already unmasked.
pub async fn reveal_action(
    pool: &PgPool,
    reveal_id: i64,
    prank_id: i64,
    by: UserId,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    if !mark_revealed(&mut tx, prank_id, by).await? {
        return Ok(false);
    }
    sqlx::query!(
        "UPDATE economy_actions SET state = 'done', ended_at = now()
         WHERE id = $1 AND state = 'pending'",
        reveal_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Move a `pending` action to what it becomes once the effect has landed:
/// `active` for a kind that [`ActionKind::stays_active`], else `done`.
/// False when the row was not pending (nothing changed).
pub async fn activate_action(
    pool: &PgPool,
    action_id: i64,
    kind: ActionKind,
    expires_at: Option<DateTime<Utc>>,
) -> sqlx::Result<bool> {
    let state = if kind.stays_active() {
        ActionState::Active
    } else {
        ActionState::Done
    };
    let done = sqlx::query!(
        "UPDATE economy_actions
         SET state = $2, expires_at = $3,
             ended_at = CASE WHEN $4 THEN now() ELSE ended_at END
         WHERE id = $1 AND state = 'pending'",
        action_id,
        state.as_str(),
        expires_at,
        state.is_final()
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// End a still-`pending` purchase that was paid for but could not land (a
/// mirror bounced it into a wall): `done`, `end_reason = 'blocked'`, and no
/// refund. False when the row was not pending.
pub async fn finish_blocked(pool: &PgPool, action_id: i64) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions
         SET state = 'done', ended_at = now(), end_reason = 'blocked'
         WHERE id = $1 AND state = 'pending'",
        action_id
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Spend `owner`'s standing mirror: `ended`, `end_reason = 'fired'`. True
/// for exactly one caller when several race for it.
pub async fn consume_mirror(pool: &PgPool, guild: GuildId, owner: UserId) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions
         SET state = 'ended', ended_at = now(), end_reason = 'fired'
         WHERE state = 'active'
           AND id = (SELECT id FROM economy_actions
                     WHERE guild_id = $1 AND target_id = $2 AND kind = 'mirror'
                       AND state = 'active'
                     ORDER BY id LIMIT 1)",
        guild.get() as i64,
        owner.get() as i64
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Give a still-`pending` purchase's price back. `None` (and nothing
/// changed) when it is not pending, so a second call does nothing.
///
/// A pending row with pledges is a filled crowdfund: its pledgers are
/// refunded exactly as [`refund_fund`] does, and the starter's `price` is
/// never credited (the starter paid only their own pledge). The returned
/// balance is then the starter's, unchanged unless they pledged too.
///
/// Lock order: the action row, then the pledgers' wallets in user-id order.
pub async fn refund_action(pool: &PgPool, action_id: i64) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "UPDATE economy_actions SET state = $2, ended_at = now()
         WHERE id = $1 AND state = $3
         RETURNING guild_id, buyer_id, price",
        action_id,
        ActionState::Failed.as_str(),
        ActionState::Pending.as_str()
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let (Some(guild), Some(buyer)) = (
        u64::try_from(row.guild_id).ok().filter(|g| *g != 0),
        uid(row.buyer_id),
    ) else {
        return Err(sqlx::Error::Decode("action has an invalid id".into()));
    };
    let guild = GuildId::new(guild);
    // A row that has any pledge, refunded or not, is a crowdfund: its price
    // is never credited to the starter.
    let is_fund = sqlx::query!(
        r#"SELECT EXISTS(SELECT 1 FROM economy_pledges WHERE action_id = $1) AS "yes!""#,
        action_id
    )
    .fetch_one(&mut *tx)
    .await?
    .yes;
    if is_fund {
        refund_pledges(&mut tx, guild, action_id).await?;
        let held = sqlx::query!(
            "SELECT balance FROM economy_wallets WHERE guild_id = $1 AND user_id = $2",
            guild.get() as i64,
            buyer.get() as i64
        )
        .fetch_optional(&mut *tx)
        .await?
        .map_or(0, |w| w.balance);
        tx.commit().await?;
        return Ok(Some(held));
    }
    let wallet = lock_wallet(&mut tx, guild, buyer).await?;
    let back = wallet
        .balance
        .checked_add(row.price)
        .ok_or_else(|| sqlx::Error::Decode("refund overflows the balance".into()))?;
    set_balance(&mut tx, &wallet, back).await?;
    write_ledger(
        &mut tx,
        guild,
        buyer,
        row.price,
        LedgerKind::Refund,
        Some(action_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Some(back))
}

/// Credit every unrefunded pledge on `fund` back, one `PledgeRefund` ledger
/// row per pledge (a member who pledged twice gets two), and stamp
/// `refunded_at`. The caller holds the fund row's lock. Wallets are locked in
/// user-id order. True when there was at least one pledge to refund.
async fn refund_pledges(
    tx: &mut sqlx::PgConnection,
    guild: GuildId,
    fund: i64,
) -> sqlx::Result<bool> {
    let pledges = sqlx::query!(
        "SELECT id, user_id, amount FROM economy_pledges
         WHERE action_id = $1 AND refunded_at IS NULL
         ORDER BY user_id, id
         FOR UPDATE",
        fund
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut wallet: Option<Locked> = None;
    for p in &pledges {
        let user = uid(p.user_id)
            .ok_or_else(|| sqlx::Error::Decode("pledge has an invalid user".into()))?;
        let mut w = match wallet.take() {
            Some(w) if w.user == user => w,
            _ => lock_wallet(tx, guild, user).await?,
        };
        let back = w
            .balance
            .checked_add(p.amount)
            .ok_or_else(|| sqlx::Error::Decode("refund overflows the balance".into()))?;
        set_balance(tx, &w, back).await?;
        w.balance = back;
        write_ledger(
            tx,
            guild,
            user,
            p.amount,
            LedgerKind::PledgeRefund,
            Some(fund),
        )
        .await?;
        sqlx::query!(
            "UPDATE economy_pledges SET refunded_at = now() WHERE id = $1",
            p.id
        )
        .execute(&mut *tx)
        .await?;
        wallet = Some(w);
    }
    Ok(!pledges.is_empty())
}

/// End an `active` row that never paid out (a theme that never played, a
/// bounty nobody collected): `ended` with `reason`, and the buyer's `price`
/// back as a [`LedgerKind::Refund`]. Returns the buyer's new balance, `None`
/// (nothing changed) when the row was not `active`, so a second call does
/// nothing. Only an escrow kind (a theme, a bounty) is refunded this way.
///
/// Lock order: the action row, then the buyer's wallet.
pub async fn refund_active(
    pool: &PgPool,
    action_id: i64,
    reason: &str,
) -> sqlx::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "UPDATE economy_actions SET state = $2, ended_at = now(), end_reason = $4
         WHERE id = $1 AND state = $3 AND kind = ANY($5)
         RETURNING guild_id, buyer_id, price",
        action_id,
        ActionState::Ended.as_str(),
        ActionState::Active.as_str(),
        reason,
        &escrow_kinds()
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let (Some(guild), Some(buyer)) = (
        u64::try_from(row.guild_id).ok().filter(|g| *g != 0),
        uid(row.buyer_id),
    ) else {
        return Err(sqlx::Error::Decode("action has an invalid id".into()));
    };
    let guild = GuildId::new(guild);
    let wallet = lock_wallet(&mut tx, guild, buyer).await?;
    let back = wallet
        .balance
        .checked_add(row.price)
        .ok_or_else(|| sqlx::Error::Decode("refund overflows the balance".into()))?;
    set_balance(&mut tx, &wallet, back).await?;
    write_ledger(
        &mut tx,
        guild,
        buyer,
        row.price,
        LedgerKind::Refund,
        Some(action_id),
    )
    .await?;
    tx.commit().await?;
    Ok(Some(back))
}

/// Pay `claimer` every standing bounty on `target` that somebody else
/// placed, in one transaction: each row ends `ended` with `claimed`, and its
/// price is credited with one [`LedgerKind::BountyWon`] row carrying that
/// bounty's id. Bounties `claimer` placed stand, and an expired bounty
/// (not yet swept) is skipped: it is refunded, not paid out. Returns the
/// total, 0 when there was none.
///
/// Lock order: the bounty rows in id order (`FOR UPDATE`), then the
/// claimer's wallet. Two racing claims never both pay a bounty: the second
/// waits on the row lock and finds it no longer `active`.
pub async fn claim_bounties(
    pool: &PgPool,
    guild: GuildId,
    target: UserId,
    claimer: UserId,
) -> sqlx::Result<i64> {
    let mut tx = pool.begin().await?;
    let bounties = sqlx::query!(
        "SELECT id, price FROM economy_actions
         WHERE guild_id = $1 AND target_id = $2 AND kind = $4
           AND state = 'active' AND buyer_id <> $3
           AND expires_at IS NOT NULL AND expires_at > now()
         ORDER BY id
         FOR UPDATE",
        guild.get() as i64,
        target.get() as i64,
        claimer.get() as i64,
        ActionKind::Bounty.key()
    )
    .fetch_all(&mut *tx)
    .await?;
    if bounties.is_empty() {
        return Ok(0);
    }
    let mut wallet = lock_wallet(&mut tx, guild, claimer).await?;
    let mut total = 0_i64;
    for b in &bounties {
        let back = wallet
            .balance
            .checked_add(b.price)
            .ok_or_else(|| sqlx::Error::Decode("bounty overflows the balance".into()))?;
        set_balance(&mut tx, &wallet, back).await?;
        wallet.balance = back;
        total += b.price;
        sqlx::query!(
            "UPDATE economy_actions SET state = $2, ended_at = now(), end_reason = 'claimed'
             WHERE id = $1",
            b.id,
            ActionState::Ended.as_str()
        )
        .execute(&mut *tx)
        .await?;
        write_ledger(
            &mut tx,
            guild,
            claimer,
            b.price,
            LedgerKind::BountyWon,
            Some(b.id),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(total)
}

/// Why a fund operation did not happen.
#[derive(Debug)]
pub enum FundError {
    /// A fund for this target and kind is already open.
    AlreadyOpen,
    /// The fund is not `funding`, or its time is up.
    Closed,
    /// The member has already put in their share of this fund: at most
    /// `max` (half the price, rounded up) each, so a fund takes two people.
    OverShare {
        max: i64,
    },
    Money(MoveError),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for FundError {
    fn from(e: sqlx::Error) -> Self {
        FundError::Db(e)
    }
}

impl From<MoveError> for FundError {
    fn from(e: MoveError) -> Self {
        FundError::Money(e)
    }
}

impl std::fmt::Display for FundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FundError::AlreadyOpen => write!(f, "a fund is already open"),
            FundError::Closed => write!(f, "the fund is closed"),
            FundError::OverShare { max } => write!(f, "a member may pledge at most {max}"),
            FundError::Money(e) => write!(f, "{e}"),
            FundError::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

/// A fund that was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenedFund {
    pub action_id: i64,
    /// What the first pledge took (0 when there was none).
    pub pledged: i64,
    /// The starter's balance after that pledge, `None` without one.
    pub balance: Option<i64>,
}

/// What a pledge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pledged {
    /// What was taken: the ask, capped at what the fund still needed.
    pub taken: i64,
    /// The pledges on the fund now.
    pub total: i64,
    pub price: i64,
    /// This pledge completed the fund, which is now `pending` (its
    /// `created_at` restarted at the fill, so reconcile's grace guard counts
    /// from the moment the row became runnable).
    pub filled: bool,
    /// The pledger's balance after.
    pub balance: i64,
}

/// The most one member may put into a fund of `price`: half, rounded up.
pub fn fund_share(price: i64) -> i64 {
    price / 2 + price % 2
}

/// One member's pledge, in pledge order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PledgeRow {
    pub user: UserId,
    pub amount: i64,
    pub signed: bool,
}

/// Open a crowdfund: the row goes in as `funding` with `a.expires_at` (the
/// caller computes now + 24 h) and `a.price` as the prank's price, then the
/// optional first pledge `(amount, signed)` from `a.buyer` is taken as
/// [`pledge`] takes one, in the same transaction. A second open fund for the
/// same target and kind is [`FundError::AlreadyOpen`], by the partial unique
/// index `economy_actions_one_fund`.
///
/// Lock order: the new action row, then the starter's wallet.
pub async fn open_fund(
    pool: &PgPool,
    a: &NewAction<'_>,
    first: Option<(i64, bool)>,
) -> Result<OpenedFund, FundError> {
    if a.price <= 0 || a.expires_at.is_none() {
        return Err(MoveError::Invalid.into());
    }
    let mut tx = pool.begin().await?;
    let inserted = sqlx::query!(
        "INSERT INTO economy_actions
             (guild_id, kind, buyer_id, target_id, price, note, signed, payload, state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id",
        a.guild.get() as i64,
        a.kind.key(),
        a.buyer.get() as i64,
        a.target.map(|t| t.get() as i64),
        a.price,
        a.note,
        a.signed,
        Json(a.payload) as Json<&ActionPayload>,
        ActionState::Funding.as_str(),
        a.expires_at,
    )
    .fetch_one(&mut *tx)
    .await;
    let row = match inserted {
        Ok(r) => r,
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("economy_actions_one_fund") => {
            return Err(FundError::AlreadyOpen);
        },
        Err(e) => return Err(e.into()),
    };
    let opened = match first {
        None => OpenedFund {
            action_id: row.id,
            pledged: 0,
            balance: None,
        },
        Some((amount, signed)) => {
            let p = take_pledge(&mut tx, row.id, a.guild, a.buyer, amount, signed, a.price).await?;
            OpenedFund {
                action_id: row.id,
                pledged: p.taken,
                balance: Some(p.balance),
            }
        },
    };
    tx.commit().await?;
    Ok(opened)
}

/// Pledge `amount` to a fund, in one transaction:
/// 1. lock the fund row `FOR UPDATE`; [`FundError::Closed`] unless it belongs
///    to `guild`, is `funding`, and is unexpired on the database clock;
/// 2. take `min(amount, price - pledged)`;
/// 3. debit the pledger (`Pledge`) and insert the pledge;
/// 4. if the pledges now equal the price, set the row `pending`.
///
/// Lock order: the fund row, then the pledger's wallet. Racing pledges
/// serialise on the fund row, so exactly one of them fills it and the
/// others find it closed.
pub async fn pledge(
    pool: &PgPool,
    fund: i64,
    guild: GuildId,
    user: UserId,
    amount: i64,
    signed: bool,
) -> Result<Pledged, FundError> {
    if amount <= 0 {
        return Err(MoveError::Invalid.into());
    }
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        r#"SELECT price, (expires_at IS NULL OR expires_at > now()) AS "live!"
           FROM economy_actions
           WHERE id = $1 AND guild_id = $2 AND state = 'funding'
           FOR UPDATE"#,
        fund,
        guild.get() as i64
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(FundError::Closed);
    };
    if !row.live {
        return Err(FundError::Closed);
    }
    let done = take_pledge(&mut tx, fund, guild, user, amount, signed, row.price).await?;
    tx.commit().await?;
    Ok(done)
}

/// The pledge itself, with the fund row already locked by the caller.
async fn take_pledge(
    tx: &mut sqlx::PgConnection,
    fund: i64,
    guild: GuildId,
    user: UserId,
    amount: i64,
    signed: bool,
    price: i64,
) -> Result<Pledged, FundError> {
    if amount <= 0 {
        return Err(MoveError::Invalid.into());
    }
    let pledged = sqlx::query!(
        r#"SELECT COALESCE(SUM(amount), 0)::bigint AS "total!"
           FROM economy_pledges WHERE action_id = $1 AND refunded_at IS NULL"#,
        fund
    )
    .fetch_one(&mut *tx)
    .await?
    .total;
    let left = price - pledged;
    if left <= 0 {
        return Err(FundError::Closed);
    }
    // One member may not fill a fund alone: at most half the price (rounded
    // up) each, across all their pledges to it.
    let max = fund_share(price);
    let mine = sqlx::query!(
        r#"SELECT COALESCE(SUM(amount), 0)::bigint AS "total!"
           FROM economy_pledges WHERE action_id = $1 AND user_id = $2"#,
        fund,
        user.get() as i64
    )
    .fetch_one(&mut *tx)
    .await?
    .total;
    let share_left = max - mine;
    if share_left <= 0 {
        return Err(FundError::OverShare { max });
    }
    let taken = amount.min(left).min(share_left);
    let wallet = lock_wallet(tx, guild, user).await?;
    if wallet.balance < taken {
        return Err(MoveError::Insufficient {
            balance: wallet.balance,
        }
        .into());
    }
    let left = wallet.balance - taken;
    set_balance(tx, &wallet, left).await?;
    sqlx::query!(
        "INSERT INTO economy_pledges (action_id, guild_id, user_id, amount, signed)
         VALUES ($1, $2, $3, $4, $5)",
        fund,
        guild.get() as i64,
        user.get() as i64,
        taken,
        signed
    )
    .execute(&mut *tx)
    .await?;
    write_ledger(tx, guild, user, -taken, LedgerKind::Pledge, Some(fund)).await?;
    let total = pledged + taken;
    let filled = total == price;
    if filled {
        sqlx::query!(
            "UPDATE economy_actions SET state = $2, created_at = now(), expires_at = NULL
             WHERE id = $1",
            fund,
            ActionState::Pending.as_str()
        )
        .execute(&mut *tx)
        .await?;
    }
    Ok(Pledged {
        taken,
        total,
        price,
        filled,
        balance: left,
    })
}

/// The fund's pledges, oldest first.
pub async fn fund_pledges(pool: &PgPool, fund: i64) -> sqlx::Result<Vec<PledgeRow>> {
    let rows = sqlx::query!(
        "SELECT user_id, amount, signed FROM economy_pledges
         WHERE action_id = $1 ORDER BY id",
        fund
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(PledgeRow {
                user: uid(r.user_id)?,
                amount: r.amount,
                signed: r.signed,
            })
        })
        .collect())
}

/// Give a fund's pledges back and end it: `ended` from `funding`, `failed`
/// from `pending`, with `reason`. Every unrefunded pledge is credited with a
/// `PledgeRefund` and stamped. False (nothing changed) when the row is
/// neither, or is `pending` with no pledges (a plain purchase: use
/// [`refund_action`]).
///
/// Lock order: the fund row, then the pledgers' wallets in user-id order.
pub async fn refund_fund(pool: &PgPool, id: i64, reason: &str) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "SELECT guild_id, state FROM economy_actions
         WHERE id = $1 AND state IN ('funding', 'pending')
         FOR UPDATE",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(false);
    };
    let guild = u64::try_from(row.guild_id)
        .ok()
        .filter(|g| *g != 0)
        .map(GuildId::new)
        .ok_or_else(|| sqlx::Error::Decode("action has an invalid id".into()))?;
    let next = if row.state == ActionState::Funding.as_str() {
        ActionState::Ended
    } else {
        ActionState::Failed
    };
    let refunded = refund_pledges(&mut tx, guild, id).await?;
    if !refunded && next == ActionState::Failed {
        return Ok(false);
    }
    sqlx::query!(
        "UPDATE economy_actions SET state = $2, ended_at = now(), end_reason = $3
         WHERE id = $1",
        id,
        next.as_str(),
        reason
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// The sweeper's end for a fund whose 24 hours are up: refund every pledge
/// and end the row `ended` with reason `unfunded`, only while it is still
/// `funding` and past `expires_at`, checked under the row lock. A fund that
/// filled between the sweeper's read and this call is left alone (false).
///
/// Lock order: the fund row, then the pledgers' wallets in user-id order.
pub async fn expire_fund(pool: &PgPool, id: i64) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let Some(row) = sqlx::query!(
        "SELECT guild_id FROM economy_actions
         WHERE id = $1 AND state = 'funding'
           AND expires_at IS NOT NULL AND expires_at <= now()
         FOR UPDATE",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(false);
    };
    let guild = u64::try_from(row.guild_id)
        .ok()
        .filter(|g| *g != 0)
        .map(GuildId::new)
        .ok_or_else(|| sqlx::Error::Decode("action has an invalid id".into()))?;
    refund_pledges(&mut tx, guild, id).await?;
    sqlx::query!(
        "UPDATE economy_actions
         SET state = $2, ended_at = now(), end_reason = 'unfunded'
         WHERE id = $1",
        id,
        ActionState::Ended.as_str()
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Set a `pending` row's payload. False when the row is not `pending`.
pub async fn set_payload(pool: &PgPool, id: i64, payload: &ActionPayload) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions SET payload = $2 WHERE id = $1 AND state = 'pending'",
        id,
        Json(payload) as Json<&ActionPayload>
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Prepare a filled (`pending`) fund to run: write the payload (with what to
/// restore) and the end of its effect in one statement. A fill leaves
/// `expires_at` empty, which reconcile reads as "never prepared, never acted".
/// False when the row is not `pending`.
pub async fn prepare_fund(
    pool: &PgPool,
    id: i64,
    payload: &ActionPayload,
    expires_at: Option<DateTime<Utc>>,
) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "UPDATE economy_actions SET payload = $2, expires_at = $3
         WHERE id = $1 AND state = 'pending'",
        id,
        Json(payload) as Json<&ActionPayload>,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Across all guilds, `funding` rows whose 24 hours are up, oldest first.
pub async fn due_funds(
    pool: &PgPool,
    now: DateTime<Utc>,
    limit: i64,
) -> sqlx::Result<Vec<(GuildId, ActionRow)>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE state = 'funding' AND expires_at IS NOT NULL AND expires_at <= $1
             AND kind = ANY($3)
           ORDER BY expires_at, id
           LIMIT $2"#,
        now,
        limit,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw))
}

/// [`RawAction`] plus the pledged total.
struct RawFund {
    id: i64,
    guild_id: i64,
    kind: String,
    buyer_id: i64,
    target_id: Option<i64>,
    payload: Json<ActionPayload>,
    state: String,
    expires_at: Option<DateTime<Utc>>,
    note: Option<String>,
    signed: bool,
    revealed_at: Option<DateTime<Utc>>,
    pledged: i64,
    price: i64,
}

/// One server's `funding` rows, each with what has been pledged and the
/// row's own price, newest first.
pub async fn open_funds(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<(ActionRow, i64, i64)>> {
    let raw = sqlx::query_as!(
        RawFund,
        r#"SELECT a.id, a.guild_id, a.kind, a.buyer_id, a.target_id,
                  a.payload AS "payload: Json<ActionPayload>",
                  a.state, a.expires_at, a.note, a.signed, a.revealed_at, a.price,
                  COALESCE((SELECT SUM(p.amount) FROM economy_pledges p
                            WHERE p.action_id = a.id AND p.refunded_at IS NULL), 0)::bigint
                      AS "pledged!"
           FROM economy_actions a
           WHERE a.guild_id = $1 AND a.state = 'funding' AND a.kind = ANY($2)
           ORDER BY a.id DESC"#,
        guild.get() as i64,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(raw
        .into_iter()
        .filter_map(|f| {
            let (pledged, price) = (f.pledged, f.price);
            RawAction {
                id: f.id,
                guild_id: f.guild_id,
                kind: f.kind,
                buyer_id: f.buyer_id,
                target_id: f.target_id,
                payload: f.payload,
                state: f.state,
                expires_at: f.expires_at,
                note: f.note,
                signed: f.signed,
                revealed_at: f.revealed_at,
            }
            .into_row()
            .map(|(_, row)| (row, pledged, price))
        })
        .collect())
}

/// One open fund as a pledger sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundView {
    pub row: ActionRow,
    /// The row's own price (never the config's, which may have moved).
    pub price: i64,
    /// What has been pledged to it, refunded pledges excluded.
    pub pledged: i64,
    /// What this member has put in.
    pub mine: i64,
}

/// The `funding` row `fund` of `guild`, with what `user` has put in. `None`
/// when it is not there, belongs to another server, or is no longer
/// `funding`.
pub async fn fund_view(
    pool: &PgPool,
    guild: GuildId,
    fund: i64,
    user: UserId,
) -> sqlx::Result<Option<FundView>> {
    let raw = sqlx::query!(
        r#"SELECT a.id, a.guild_id, a.kind, a.buyer_id, a.target_id,
                  a.payload AS "payload: Json<ActionPayload>",
                  a.state, a.expires_at, a.note, a.signed, a.revealed_at, a.price,
                  COALESCE((SELECT SUM(p.amount) FROM economy_pledges p
                            WHERE p.action_id = a.id AND p.refunded_at IS NULL), 0)::bigint
                      AS "pledged!",
                  COALESCE((SELECT SUM(p.amount) FROM economy_pledges p
                            WHERE p.action_id = a.id AND p.user_id = $3), 0)::bigint
                      AS "mine!"
           FROM economy_actions a
           WHERE a.id = $1 AND a.guild_id = $2 AND a.state = 'funding' AND a.kind = ANY($4)"#,
        fund,
        guild.get() as i64,
        user.get() as i64,
        &known_kinds()
    )
    .fetch_optional(pool)
    .await?;
    Ok(raw.and_then(|f| {
        let (price, pledged, mine) = (f.price, f.pledged, f.mine);
        RawAction {
            id: f.id,
            guild_id: f.guild_id,
            kind: f.kind,
            buyer_id: f.buyer_id,
            target_id: f.target_id,
            payload: f.payload,
            state: f.state,
            expires_at: f.expires_at,
            note: f.note,
            signed: f.signed,
            revealed_at: f.revealed_at,
        }
        .into_row()
        .map(|(_, row)| FundView {
            row,
            price,
            pledged,
            mine,
        })
    }))
}

/// One server's standing bounties, oldest first.
pub async fn active_bounties(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND kind = $2 AND state = 'active'
           ORDER BY id"#,
        guild.get() as i64,
        ActionKind::Bounty.key()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// One standing bounty as `/bounty list` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BountyStake {
    pub target: UserId,
    pub placer: UserId,
    pub amount: i64,
    pub signed: bool,
    pub expires_at: DateTime<Utc>,
}

/// One server's live bounties (`active` and not yet expired), oldest first,
/// with what was staked. A row with an id this build can't read is skipped.
pub async fn live_bounty_stakes(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<BountyStake>> {
    let rows = sqlx::query!(
        r#"SELECT buyer_id, target_id AS "target_id!", price, signed,
                  expires_at AS "expires_at!"
           FROM economy_actions
           WHERE guild_id = $1 AND kind = $2 AND state = 'active'
             AND target_id IS NOT NULL AND expires_at > now()
           ORDER BY id"#,
        guild.get() as i64,
        ActionKind::Bounty.key()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(BountyStake {
                target: uid(r.target_id)?,
                placer: uid(r.buyer_id)?,
                amount: r.price,
                signed: r.signed,
                expires_at: r.expires_at,
            })
        })
        .collect())
}

/// An `economy_actions` row as the sweep queries read it.
struct RawAction {
    id: i64,
    guild_id: i64,
    kind: String,
    buyer_id: i64,
    target_id: Option<i64>,
    payload: Json<ActionPayload>,
    state: String,
    expires_at: Option<DateTime<Utc>>,
    note: Option<String>,
    signed: bool,
    revealed_at: Option<DateTime<Utc>>,
}

impl RawAction {
    /// A row with a bad id or an unreadable state is skipped. An unknown
    /// kind never gets here: the queries select only [`known_kinds`], because
    /// a payload this build cannot decode would fail the whole fetch (an
    /// image-only rollback below a version that added kinds).
    fn into_row(self) -> Option<(GuildId, ActionRow)> {
        let guild = u64::try_from(self.guild_id)
            .ok()
            .filter(|g| *g != 0 && *g != u64::MAX)
            .map(GuildId::new)?;
        Some((
            guild,
            ActionRow {
                id: self.id,
                kind: ActionKind::from_key(&self.kind)?,
                buyer: uid(self.buyer_id)?,
                target: self.target_id.and_then(uid),
                payload: self.payload.0,
                state: ActionState::parse(&self.state)?,
                expires_at: self.expires_at,
                note: self.note,
                signed: self.signed,
                revealed_at: self.revealed_at,
            },
        ))
    }
}

/// The kinds whose end refunds the buyer ([`ActionKind::is_escrow`]).
fn escrow_kinds() -> Vec<String> {
    ActionKind::ALL
        .iter()
        .filter(|k| k.is_escrow())
        .map(|k| k.key().to_string())
        .collect()
}

/// Every kind this build knows, as stored.
fn known_kinds() -> Vec<String> {
    ActionKind::ALL
        .iter()
        .map(|k| k.key().to_string())
        .collect()
}

fn rows_of(raw: Vec<RawAction>) -> Vec<(GuildId, ActionRow)> {
    raw.into_iter().filter_map(RawAction::into_row).collect()
}

/// Across all guilds, `active` rows whose time is up, oldest first. A row
/// with no `expires_at` (a mirror) is never due.
pub async fn due_actions(
    pool: &PgPool,
    now: DateTime<Utc>,
    limit: i64,
) -> sqlx::Result<Vec<(GuildId, ActionRow)>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE state = 'active' AND expires_at IS NOT NULL AND expires_at <= $1
             AND kind = ANY($3)
           ORDER BY expires_at, id
           LIMIT $2"#,
        now,
        limit,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw))
}

/// One server's `active` rows whose time is up.
pub async fn overdue_for_guild(
    pool: &PgPool,
    guild: GuildId,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND state = 'active'
             AND expires_at IS NOT NULL AND expires_at <= $2
             AND kind = ANY($3)
           ORDER BY expires_at, id"#,
        guild.get() as i64,
        now,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// One server's `active` rows of these kinds, whatever their end, each with
/// when it was bought (the database's clock).
pub async fn active_for_guild(
    pool: &PgPool,
    guild: GuildId,
    kinds: &[ActionKind],
) -> sqlx::Result<Vec<(ActionRow, DateTime<Utc>)>> {
    let keys: Vec<String> = kinds.iter().map(|k| k.key().to_string()).collect();
    let raw = sqlx::query_as!(
        RawRecent,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at, end_reason, created_at
           FROM economy_actions
           WHERE guild_id = $1 AND state = 'active' AND kind = ANY($2)
           ORDER BY id"#,
        guild.get() as i64,
        &keys
    )
    .fetch_all(pool)
    .await?;
    Ok(raw
        .into_iter()
        .filter_map(|r| {
            let created_at = r.created_at;
            r.into_recent().map(|a| (a.row, created_at))
        })
        .collect())
}

/// The `ending` rows (undone on the member's next voice join) aimed at one
/// member.
pub async fn ending_for(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND target_id = $2 AND state = 'ending'
             AND kind = ANY($3)
           ORDER BY id"#,
        guild.get() as i64,
        user.get() as i64,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// All of one server's `ending` rows.
pub async fn ending_for_guild(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND state = 'ending' AND kind = ANY($2)
           ORDER BY id"#,
        guild.get() as i64,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// The database's clock now.
pub async fn db_now(pool: &PgPool) -> sqlx::Result<DateTime<Utc>> {
    sqlx::query_scalar!(r#"SELECT now() AS "now!""#)
        .fetch_one(pool)
        .await
}

/// One server's `pending` rows that nothing is still working on: every row
/// charged before `process_start` (a previous process left it, whatever its
/// age), and any row older than `grace_secs` on the database's clock. A
/// purchase of this process between its charge and its activate is younger
/// than both and is left alone. Without a recorded start only the grace
/// counts.
pub async fn pending_for_guild(
    pool: &PgPool,
    guild: GuildId,
    process_start: Option<DateTime<Utc>>,
    grace_secs: f64,
) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND state = 'pending'
             AND created_at < GREATEST($2::timestamptz, now() - make_interval(secs => $3))
             AND kind = ANY($4)
           ORDER BY id"#,
        guild.get() as i64,
        process_start,
        grace_secs,
        &known_kinds()
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// Move a running (`active` or `ending`) row to `Ended` (final, stamped) or
/// `Ending` (waits for the member's next voice join), with the reason. The
/// reason `expired` is the generic one and never overwrites a stored reason
/// (a cleanse set aside or scheduled stays "cleansed" after the retry); any
/// specific reason (`left`, `overridden`, `cleansed`, ...) wins. False when
/// the row was not running, or `state` is neither of those: nothing changed.
pub async fn end_action(
    pool: &PgPool,
    action_id: i64,
    state: ActionState,
    end_reason: &str,
) -> sqlx::Result<bool> {
    if !matches!(state, ActionState::Ended | ActionState::Ending) {
        return Ok(false);
    }
    let done = sqlx::query!(
        "UPDATE economy_actions
         SET state = $2,
             end_reason = CASE WHEN $3 = 'expired' THEN COALESCE(end_reason, $3) ELSE $3 END,
             ended_at = CASE WHEN $4 THEN now() ELSE ended_at END
         WHERE id = $1 AND state IN ('active', 'ending')",
        action_id,
        state.as_str(),
        end_reason,
        state.is_final()
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// The row's state now, `None` when there is no such row.
pub async fn action_state(pool: &PgPool, action_id: i64) -> sqlx::Result<Option<ActionState>> {
    let row = sqlx::query!("SELECT state FROM economy_actions WHERE id = $1", action_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.and_then(|r| ActionState::parse(&r.state)))
}

/// Gift credits. Both wallets are locked in ascending user-id order so two
/// opposite gifts cannot deadlock. Returns (sender, receiver) balances.
pub async fn transfer(
    pool: &PgPool,
    guild: GuildId,
    from: UserId,
    to: UserId,
    amount: i64,
) -> Result<(i64, i64), MoveError> {
    if amount <= 0 || from == to {
        return Err(MoveError::Invalid);
    }
    let mut tx = pool.begin().await?;
    let (first, second) = if from < to { (from, to) } else { (to, from) };
    let w1 = lock_wallet(&mut tx, guild, first).await?;
    let w2 = lock_wallet(&mut tx, guild, second).await?;
    let (sender, receiver) = if first == from { (w1, w2) } else { (w2, w1) };
    if sender.balance < amount {
        return Err(MoveError::Insufficient {
            balance: sender.balance,
        });
    }
    let from_bal = sender.balance - amount;
    let to_bal = receiver
        .balance
        .checked_add(amount)
        .ok_or(MoveError::Invalid)?;
    set_balance(&mut tx, &sender, from_bal).await?;
    set_balance(&mut tx, &receiver, to_bal).await?;
    write_ledger(&mut tx, guild, from, -amount, LedgerKind::GiftSent, None).await?;
    write_ledger(&mut tx, guild, to, amount, LedgerKind::GiftReceived, None).await?;
    tx.commit().await?;
    Ok((from_bal, to_bal))
}

/// Settle a coinflip: the stake is taken, and a win pays it back twice over.
/// Returns the new balance.
pub async fn settle_coinflip(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    bet: i64,
    won: bool,
) -> Result<i64, MoveError> {
    if bet <= 0 {
        return Err(MoveError::Invalid);
    }
    let mut tx = pool.begin().await?;
    let wallet = lock_wallet(&mut tx, guild, user).await?;
    let held = wallet.balance;
    if held < bet {
        return Err(MoveError::Insufficient { balance: held });
    }
    write_ledger(&mut tx, guild, user, -bet, LedgerKind::CoinflipStake, None).await?;
    let mut now = held - bet;
    if won {
        write_ledger(
            &mut tx,
            guild,
            user,
            bet.checked_mul(2).ok_or(MoveError::Invalid)?,
            LedgerKind::CoinflipWin,
            None,
        )
        .await?;
        now = now.checked_add(2 * bet).ok_or(MoveError::Invalid)?;
    }
    set_balance(&mut tx, &wallet, now).await?;
    tx.commit().await?;
    Ok(now)
}

/// A moderator's change to a balance. Going below zero is refused.
pub async fn adjust(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    amount: i64,
    by: UserId,
    reason: &str,
) -> Result<i64, MoveError> {
    if amount == 0 {
        return Err(MoveError::Invalid);
    }
    let mut tx = pool.begin().await?;
    let wallet = lock_wallet(&mut tx, guild, user).await?;
    let held = wallet.balance;
    let now = held.checked_add(amount).ok_or(MoveError::Invalid)?;
    if now < 0 {
        return Err(MoveError::Insufficient { balance: held });
    }
    set_balance(&mut tx, &wallet, now).await?;
    sqlx::query!(
        "INSERT INTO economy_ledger (guild_id, user_id, delta, kind, by_user_id, reason)
         VALUES ($1, $2, $3, $4, $5, $6)",
        guild.get() as i64,
        user.get() as i64,
        amount,
        LedgerKind::Adjust.as_str(),
        by.get() as i64,
        reason
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(now)
}

/// The richest members, balances above 0 only; ties by user id.
pub async fn top(pool: &PgPool, guild: GuildId, limit: i64) -> sqlx::Result<Vec<(UserId, i64)>> {
    let rows = sqlx::query!(
        "SELECT user_id, balance FROM economy_wallets
         WHERE guild_id = $1 AND balance > 0
         ORDER BY balance DESC, user_id ASC LIMIT $2",
        guild.get() as i64,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| uid(r.user_id).map(|u| (u, r.balance)))
        .collect())
}

/// How many wallets a server has and the sum of their balances (0 when none).
pub async fn stats(pool: &PgPool, guild: GuildId) -> sqlx::Result<(i64, i64)> {
    let row = sqlx::query!(
        r#"SELECT COUNT(*) AS "wallets!", COALESCE(SUM(balance), 0)::BIGINT AS "credits!"
           FROM economy_wallets WHERE guild_id = $1"#,
        guild.get() as i64
    )
    .fetch_one(pool)
    .await?;
    Ok((row.wallets, row.credits))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub delta: i64,
    pub kind: String,
    pub created_at: DateTime<Utc>,
}

/// A member's ledger, newest first.
pub async fn history(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    limit: i64,
) -> sqlx::Result<Vec<LedgerEntry>> {
    let rows = sqlx::query!(
        "SELECT delta, kind, created_at FROM economy_ledger
         WHERE guild_id = $1 AND user_id = $2
         ORDER BY created_at DESC, id DESC LIMIT $3",
        guild.get() as i64,
        user.get() as i64,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| LedgerEntry {
            delta: r.delta,
            kind: r.kind,
            created_at: r.created_at,
        })
        .collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptOutState {
    pub out: bool,
    pub changed_at: Option<DateTime<Utc>>,
}

pub async fn optout_state(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
) -> sqlx::Result<OptOutState> {
    let row = sqlx::query!(
        "SELECT opted_out_at, optout_changed_at FROM economy_wallets
         WHERE guild_id = $1 AND user_id = $2",
        guild.get() as i64,
        user.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map_or(
        OptOutState {
            out: false,
            changed_at: None,
        },
        |r| OptOutState {
            out: r.opted_out_at.is_some(),
            changed_at: r.optout_changed_at,
        },
    ))
}

/// Switch a member in or out, creating the wallet if needed.
pub async fn set_opted_out(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    out: bool,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO economy_wallets (guild_id, user_id, opted_out_at, optout_changed_at)
         VALUES ($1, $2, CASE WHEN $3 THEN $4::timestamptz END, $4)
         ON CONFLICT (guild_id, user_id) DO UPDATE SET
             opted_out_at = EXCLUDED.opted_out_at,
             optout_changed_at = EXCLUDED.optout_changed_at",
        guild.get() as i64,
        user.get() as i64,
        out,
        now
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// A member's stored immunity grants (at most one per source).
pub async fn grants(pool: &PgPool, guild: GuildId, user: UserId) -> sqlx::Result<Vec<Grant>> {
    let rows = sqlx::query!(
        "SELECT source, until, granted_by, reason FROM economy_immunity
         WHERE guild_id = $1 AND user_id = $2 ORDER BY source",
        guild.get() as i64,
        user.get() as i64
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(Grant {
                source: GrantSource::parse(&r.source)?,
                until: r.until,
                granted_by: r.granted_by.and_then(uid),
                reason: r.reason,
            })
        })
        .collect())
}

/// Extend a bought grant from the later of `now` and its current end.
/// Returns the new end.
pub async fn extend_bought(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    hours: u32,
    now: DateTime<Utc>,
) -> sqlx::Result<DateTime<Utc>> {
    let hours = i32::try_from(hours)
        .map_err(|_| sqlx::Error::Decode("immunity hours out of range".into()))?;
    let row = sqlx::query!(
        r#"INSERT INTO economy_immunity AS i (guild_id, user_id, source, until)
           VALUES ($1, $2, 'bought', $3::timestamptz + make_interval(hours => $4))
           ON CONFLICT (guild_id, user_id, source) DO UPDATE SET
               until = GREATEST(COALESCE(i.until, $3::timestamptz), $3::timestamptz)
                       + make_interval(hours => $4)
           RETURNING until AS "until!""#,
        guild.get() as i64,
        user.get() as i64,
        now,
        hours
    )
    .fetch_one(pool)
    .await?;
    Ok(row.until)
}

/// Extend a bought grant and mark its still-`pending` purchase `done` in one
/// transaction, so `pending` can only mean "not granted". Errors, granting
/// nothing, when the action is not pending (already finished or refunded).
pub async fn grant_bought_for_action(
    pool: &PgPool,
    action_id: i64,
    guild: GuildId,
    user: UserId,
    hours: u32,
    now: DateTime<Utc>,
) -> sqlx::Result<DateTime<Utc>> {
    let hours = i32::try_from(hours)
        .map_err(|_| sqlx::Error::Decode("immunity hours out of range".into()))?;
    let mut tx = pool.begin().await?;
    let done = sqlx::query!(
        "UPDATE economy_actions SET state = $2, ended_at = now()
         WHERE id = $1 AND state = $3",
        action_id,
        ActionState::Done.as_str(),
        ActionState::Pending.as_str()
    )
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() != 1 {
        // Dropping the transaction rolls it back.
        return Err(sqlx::Error::RowNotFound);
    }
    let row = sqlx::query!(
        r#"INSERT INTO economy_immunity AS i (guild_id, user_id, source, until)
           VALUES ($1, $2, 'bought', $3::timestamptz + make_interval(hours => $4))
           ON CONFLICT (guild_id, user_id, source) DO UPDATE SET
               until = GREATEST(COALESCE(i.until, $3::timestamptz), $3::timestamptz)
                       + make_interval(hours => $4)
           RETURNING until AS "until!""#,
        guild.get() as i64,
        user.get() as i64,
        now,
        hours
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.until)
}

/// Give or replace a member's moderator grant. `until: None` lasts until lifted.
pub async fn grant_mod(
    pool: &PgPool,
    guild: GuildId,
    user: UserId,
    until: Option<DateTime<Utc>>,
    by: UserId,
    reason: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "INSERT INTO economy_immunity (guild_id, user_id, source, until, granted_by, reason)
         VALUES ($1, $2, 'mod', $3, $4, $5)
         ON CONFLICT (guild_id, user_id, source) DO UPDATE SET
             until = EXCLUDED.until,
             granted_by = EXCLUDED.granted_by,
             reason = EXCLUDED.reason",
        guild.get() as i64,
        user.get() as i64,
        until,
        by.get() as i64,
        reason
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove the moderator grant only. True when there was one.
pub async fn lift_mod(pool: &PgPool, guild: GuildId, user: UserId) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        "DELETE FROM economy_immunity WHERE guild_id = $1 AND user_id = $2 AND source = 'mod'",
        guild.get() as i64,
        user.get() as i64
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn settings_survive_a_save_and_load(pool: PgPool) -> sqlx::Result<()> {
        let mut s = EconomySettings {
            guild: GuildId::new(1),
            enabled: true,
            config: EconomyConfig::default(),
        };
        s.config.currency_name = "gems".into();
        s.config.max_bet = 42;
        save_settings(&pool, &s).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(s.clone()));
        let off = EconomySettings::off(s.guild);
        save_settings(&pool, &off).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(off), "upsert");
        assert_eq!(load_settings(&pool, GuildId::new(9)).await?, None);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn load_opted_out_reads_only_opted_out_wallets(pool: PgPool) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO economy_wallets (guild_id, user_id, opted_out_at) VALUES
             (1, 10, now()), (1, 11, NULL), (2, 12, now())",
        )
        .execute(&pool)
        .await?;
        assert_eq!(
            load_opted_out(&pool, GuildId::new(1)).await?,
            HashSet::from([UserId::new(10)])
        );
        assert!(load_opted_out(&pool, GuildId::new(3)).await?.is_empty());
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn stats_counts_wallets_and_sums_balances(pool: PgPool) -> sqlx::Result<()> {
        assert_eq!(stats(&pool, GuildId::new(1)).await?, (0, 0));
        sqlx::query(
            "INSERT INTO economy_wallets (guild_id, user_id, balance) VALUES
             (1, 10, 100), (1, 11, 0), (1, 12, 250), (2, 13, 999)",
        )
        .execute(&pool)
        .await?;
        assert_eq!(stats(&pool, GuildId::new(1)).await?, (3, 350));
        assert_eq!(stats(&pool, GuildId::new(2)).await?, (1, 999));
        Ok(())
    }

    use crate::economy::immunity::GrantSource;
    use crate::economy::settings::ActionKind;
    use crate::economy::wallet::{ActionPayload, ActionState, MoveError};
    use chrono::{Duration, TimeZone};

    const G: GuildId = GuildId::new(1);
    const A: UserId = UserId::new(10);
    const B: UserId = UserId::new(11);
    const MOD: UserId = UserId::new(99);

    macro_rules! db_test {
        ($name:ident, |$pool:ident: PgPool| $body:expr) => {
            #[sqlx::test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) -> sqlx::Result<()> {
                $body.await
            }
        };
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    async fn fund(pool: &PgPool, user: UserId, amount: i64) {
        credit_earnings(
            pool,
            G,
            &[EarnRow {
                user,
                voice: amount,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    fn immunity_action<'a>(buyer: UserId, price: i64, payload: &'a ActionPayload) -> NewAction<'a> {
        NewAction {
            guild: G,
            kind: ActionKind::Immunity,
            buyer,
            target: None,
            price,
            note: None,
            signed: false,
            payload,
            expires_at: None,
        }
    }

    #[derive(sqlx::FromRow)]
    struct StoredAction {
        kind: String,
        buyer_id: i64,
        target_id: Option<i64>,
        price: i64,
        note: Option<String>,
        signed: bool,
        payload: String,
        state: String,
        expires_at: Option<DateTime<Utc>>,
    }

    async fn ended(pool: &PgPool) -> sqlx::Result<bool> {
        sqlx::query_scalar("SELECT ended_at IS NOT NULL FROM economy_actions")
            .fetch_one(pool)
            .await
    }

    async fn ledger_count(pool: &PgPool, kind: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM economy_ledger WHERE kind = $1")
            .bind(kind)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    db_test!(concurrent_debits_never_overdraw, |pool: PgPool| async {
        // Open both connections first, so the two charges overlap instead of
        // the second waiting on a connection while the first finishes.
        let (c1, c2) = (pool.acquire().await?, pool.acquire().await?);
        drop((c1, c2));
        let payload = ActionPayload::Immunity { hours: 24 };
        let act = immunity_action(A, 80, &payload);
        for round in 0..10 {
            sqlx::query("DELETE FROM economy_wallets")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_ledger")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_actions")
                .execute(&pool)
                .await?;
            fund(&pool, A, 100).await;
            let (x, y) = tokio::join!(
                charge_for_action(&pool, &act),
                charge_for_action(&pool, &act)
            );
            let (ok, refused) = match (x, y) {
                (Ok(c), Err(e)) | (Err(e), Ok(c)) => (c, e),
                (x, y) => panic!("round {round}: exactly one must succeed: {x:?} / {y:?}"),
            };
            assert_eq!(ok.balance, 20);
            assert!(
                matches!(refused, MoveError::Insufficient { balance: 20 }),
                "{refused:?}"
            );
            assert_eq!(balance(&pool, G, A).await?, 20);
            assert_eq!(ledger_count(&pool, "purchase").await, 1);
            let actions: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_actions")
                .fetch_one(&pool)
                .await?;
            assert_eq!(actions, 1);
        }
        Ok(())
    });

    db_test!(
        wallet_summary_reads_counters_and_defaults_to_zero,
        |pool: PgPool| async {
            assert_eq!(
                wallet_summary(&pool, G, A).await?,
                WalletSummary {
                    balance: 0,
                    earned_voice: 0,
                    earned_messages: 0
                }
            );
            credit_earnings(
                &pool,
                G,
                &[EarnRow {
                    user: A,
                    voice: 30,
                    messages: 12,
                }],
            )
            .await?;
            let payload = ActionPayload::Immunity { hours: 24 };
            charge_for_action(&pool, &immunity_action(A, 10, &payload))
                .await
                .unwrap();
            assert_eq!(
                wallet_summary(&pool, G, A).await?,
                WalletSummary {
                    balance: 32,
                    earned_voice: 30,
                    earned_messages: 12
                }
            );
            // Another member, another server: still zeros.
            assert_eq!(wallet_summary(&pool, G, B).await?.earned_voice, 0);
            assert_eq!(wallet_summary(&pool, GuildId::new(2), A).await?.balance, 0);
            Ok(())
        }
    );

    db_test!(
        earning_racing_a_charge_keeps_the_invariant,
        |pool: PgPool| async {
            // Open both connections first so the two writes overlap.
            let (c1, c2) = (pool.acquire().await?, pool.acquire().await?);
            drop((c1, c2));
            let payload = ActionPayload::Immunity { hours: 24 };
            let act = immunity_action(A, 5, &payload);
            let earn = [EarnRow {
                user: A,
                voice: 7,
                messages: 3,
            }];
            fund(&pool, A, 100).await;
            for round in 0..10 {
                let (e, c) = tokio::join!(
                    credit_earnings(&pool, G, &earn),
                    charge_for_action(&pool, &act)
                );
                e?;
                c.unwrap_or_else(|err| panic!("round {round}: charge failed: {err:?}"));
            }
            // fund() credited 100 to the counters without a ledger row, so the
            // invariant is: balance = ledger deltas + earned counters.
            let w = wallet_summary(&pool, G, A).await?;
            let ledger: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(delta), 0)::BIGINT FROM economy_ledger WHERE guild_id = 1 AND user_id = 10",
        )
        .fetch_one(&pool)
        .await?;
            assert_eq!(w.earned_voice, 100 + 10 * 7);
            assert_eq!(w.earned_messages, 10 * 3);
            assert_eq!(ledger, -10 * 5);
            assert_eq!(w.balance, ledger + w.earned_voice + w.earned_messages);
            Ok(())
        }
    );

    db_test!(refund_restores_once, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = ActionPayload::Immunity { hours: 24 };
        let c = charge_for_action(&pool, &immunity_action(A, 50, &payload))
            .await
            .unwrap();
        assert_eq!(c.balance, 50);
        assert_eq!(refund_action(&pool, c.action_id).await?, Some(100));
        assert_eq!(refund_action(&pool, c.action_id).await?, None);
        assert_eq!(balance(&pool, G, A).await?, 100);
        let state: String = sqlx::query_scalar("SELECT state FROM economy_actions WHERE id = $1")
            .bind(c.action_id)
            .fetch_one(&pool)
            .await?;
        assert_eq!(state, "failed");
        assert_eq!(ledger_count(&pool, "refund").await, 1);
        Ok(())
    });

    db_test!(refund_ignores_a_started_action, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = ActionPayload::Immunity { hours: 24 };
        let c = charge_for_action(&pool, &immunity_action(A, 50, &payload))
            .await
            .unwrap();
        finish_action(&pool, c.action_id, ActionState::Active).await?;
        assert!(!ended(&pool).await?, "active is not an end");
        assert_eq!(refund_action(&pool, c.action_id).await?, None);
        assert_eq!(balance(&pool, G, A).await?, 50);
        assert_eq!(ledger_count(&pool, "refund").await, 0);
        finish_action(&pool, c.action_id, ActionState::Done).await?;
        assert!(ended(&pool).await?, "done stamps ended_at");
        Ok(())
    });

    db_test!(charge_stores_the_action_as_given, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = ActionPayload::Immunity { hours: 6 };
        let act = NewAction {
            target: Some(B),
            note: Some("hi"),
            signed: true,
            expires_at: Some(now()),
            ..immunity_action(A, 30, &payload)
        };
        let c = charge_for_action(&pool, &act).await.unwrap();
        let row: StoredAction = sqlx::query_as(
            "SELECT kind, buyer_id, target_id, price, note, signed, payload::text AS payload, state, expires_at
             FROM economy_actions WHERE id = $1",
        )
        .bind(c.action_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(row.kind, "immunity");
        assert_eq!((row.buyer_id, row.target_id, row.price), (10, Some(11), 30));
        assert_eq!(row.note.as_deref(), Some("hi"));
        assert!(row.signed);
        let stored: ActionPayload = serde_json::from_str(&row.payload).unwrap();
        assert_eq!(stored, payload);
        assert_eq!(row.state, "pending");
        assert_eq!(row.expires_at, Some(now()));
        let ledger: (i64, Option<i64>) =
            sqlx::query_as("SELECT delta, action_id FROM economy_ledger WHERE kind = 'purchase'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(ledger, (-30, Some(c.action_id)));
        Ok(())
    });

    db_test!(ledger_and_wallet_agree, |pool: PgPool| async {
        credit_earnings(
            &pool,
            G,
            &[
                EarnRow {
                    user: A,
                    voice: 150,
                    messages: 50,
                },
                EarnRow {
                    user: B,
                    voice: 10,
                    messages: 5,
                },
            ],
        )
        .await?;
        let payload = ActionPayload::Immunity { hours: 24 };
        let c = charge_for_action(&pool, &immunity_action(A, 50, &payload))
            .await
            .unwrap();
        refund_action(&pool, c.action_id).await?;
        transfer(&pool, G, A, B, 30).await.unwrap();
        settle_coinflip(&pool, G, A, 20, true).await.unwrap();
        settle_coinflip(&pool, G, A, 40, false).await.unwrap();
        adjust(&pool, G, B, -7, MOD, "oops").await.unwrap();
        adjust(&pool, G, B, 3, MOD, "ok").await.unwrap();
        // A: 200 -50 +50 -30 +20 -40 = 150; B: 15 +30 -7 +3 = 41
        assert_eq!(balance(&pool, G, A).await?, 150);
        assert_eq!(balance(&pool, G, B).await?, 41);
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT w.balance,
                    w.earned_voice + w.earned_messages
                      + COALESCE((SELECT sum(delta) FROM economy_ledger l
                                  WHERE l.guild_id = w.guild_id AND l.user_id = w.user_id), 0)::bigint,
                    w.user_id
             FROM economy_wallets w ORDER BY w.user_id",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(rows.len(), 2);
        for (bal, derived, user) in rows {
            assert_eq!(bal, derived, "wallet {user}");
        }
        Ok(())
    });

    db_test!(
        coinflip_pays_the_stake_back_plus_the_same_again,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            assert_eq!(settle_coinflip(&pool, G, A, 30, true).await.unwrap(), 130);
            assert_eq!(settle_coinflip(&pool, G, A, 50, false).await.unwrap(), 80);
            let rows: Vec<(String, i64)> =
                sqlx::query_as("SELECT kind, delta FROM economy_ledger ORDER BY id")
                    .fetch_all(&pool)
                    .await?;
            assert_eq!(
                rows,
                vec![
                    ("coinflip_stake".to_string(), -30),
                    ("coinflip_win".to_string(), 60),
                    ("coinflip_stake".to_string(), -50),
                ]
            );
            let e = settle_coinflip(&pool, G, A, 81, true).await.unwrap_err();
            assert!(matches!(e, MoveError::Insufficient { balance: 80 }));
            assert_eq!(balance(&pool, G, A).await?, 80);
            Ok(())
        }
    );

    db_test!(transfer_moves_both_sides, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        assert_eq!(transfer(&pool, G, A, B, 40).await.unwrap(), (60, 40));
        assert_eq!(ledger_count(&pool, "gift_sent").await, 1);
        assert_eq!(ledger_count(&pool, "gift_received").await, 1);
        // the lower id as the sender takes the other lock order
        assert_eq!(transfer(&pool, G, B, A, 10).await.unwrap(), (30, 70));
        Ok(())
    });

    db_test!(transfer_moves_both_sides_or_neither, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let e = transfer(&pool, G, A, B, 101).await.unwrap_err();
        assert!(matches!(e, MoveError::Insufficient { balance: 100 }));
        assert_eq!(balance(&pool, G, A).await?, 100);
        assert_eq!(balance(&pool, G, B).await?, 0);
        let wallets: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_wallets")
            .fetch_one(&pool)
            .await?;
        assert_eq!(wallets, 1, "the receiver's wallet is not created either");
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_ledger")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows, 0);
        Ok(())
    });

    db_test!(adjust_below_zero_is_refused, |pool: PgPool| async {
        fund(&pool, A, 200).await;
        let e = adjust(&pool, G, A, -500, MOD, "no").await.unwrap_err();
        assert!(matches!(e, MoveError::Insufficient { balance: 200 }));
        assert_eq!(balance(&pool, G, A).await?, 200);
        assert_eq!(ledger_count(&pool, "adjust").await, 0);
        assert_eq!(adjust(&pool, G, A, -200, MOD, "all").await.unwrap(), 0);
        assert_eq!(adjust(&pool, G, B, 25, MOD, "new").await.unwrap(), 25);
        let row: (i64, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT delta, by_user_id, reason FROM economy_ledger WHERE user_id = 11",
        )
        .fetch_one(&pool)
        .await?;
        assert_eq!(row, (25, Some(99), Some("new".to_string())));
        let e = adjust(&pool, G, UserId::new(50), -1, MOD, "none")
            .await
            .unwrap_err();
        assert!(matches!(e, MoveError::Insufficient { balance: 0 }));
        Ok(())
    });

    db_test!(credit_earnings_creates_and_adds, |pool: PgPool| async {
        fund(&pool, A, 10).await;
        credit_earnings(
            &pool,
            G,
            &[
                EarnRow {
                    user: A,
                    voice: 5,
                    messages: 2,
                },
                EarnRow {
                    user: B,
                    voice: 3,
                    messages: 4,
                },
            ],
        )
        .await?;
        let rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
            "SELECT user_id, balance, earned_voice, earned_messages
             FROM economy_wallets WHERE guild_id = 1 ORDER BY user_id",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(rows, vec![(10, 17, 15, 2), (11, 7, 3, 4)]);
        let ledger: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_ledger")
            .fetch_one(&pool)
            .await?;
        assert_eq!(ledger, 0, "earning writes counters, not ledger rows");
        credit_earnings(&pool, G, &[]).await?;
        Ok(())
    });

    db_test!(top_orders_and_skips_zero, |pool: PgPool| async {
        for (u, n) in [(12, 5), (11, 9), (10, 9), (13, 0), (14, 1)] {
            fund(&pool, UserId::new(u), n).await;
        }
        assert_eq!(
            top(&pool, G, 10).await?,
            vec![
                (UserId::new(10), 9),
                (UserId::new(11), 9),
                (UserId::new(12), 5),
                (UserId::new(14), 1)
            ]
        );
        assert_eq!(top(&pool, G, 2).await?.len(), 2);
        assert!(top(&pool, GuildId::new(2), 10).await?.is_empty());
        Ok(())
    });

    db_test!(history_is_newest_first_and_limited, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        adjust(&pool, G, A, 1, MOD, "a").await.unwrap();
        adjust(&pool, G, A, 2, MOD, "b").await.unwrap();
        transfer(&pool, G, A, B, 3).await.unwrap();
        let h = history(&pool, G, A, 2).await?;
        assert_eq!(
            h.iter()
                .map(|e| (e.delta, e.kind.as_str()))
                .collect::<Vec<_>>(),
            vec![(-3, "gift_sent"), (2, "adjust")]
        );
        assert_eq!(history(&pool, G, A, 10).await?.len(), 3);
        assert!(history(&pool, G, UserId::new(77), 10).await?.is_empty());
        Ok(())
    });

    db_test!(
        extend_bought_extends_from_the_later_of_now_and_until,
        |pool: PgPool| async {
            let t = now();
            sqlx::query(
            "INSERT INTO economy_immunity (guild_id, user_id, source, until) VALUES (1, 10, 'bought', $1)",
        )
        .bind(t + Duration::hours(2))
        .execute(&pool)
        .await?;
            assert_eq!(
                extend_bought(&pool, G, A, 24, t).await?,
                t + Duration::hours(26)
            );
            sqlx::query("UPDATE economy_immunity SET until = $1")
                .bind(t - Duration::hours(5))
                .execute(&pool)
                .await?;
            assert_eq!(
                extend_bought(&pool, G, A, 24, t).await?,
                t + Duration::hours(24)
            );
            assert_eq!(
                extend_bought(&pool, G, B, 1, t).await?,
                t + Duration::hours(1)
            );
            let g = grants(&pool, G, A).await?;
            assert_eq!(g.len(), 1);
            assert_eq!(g[0].source, GrantSource::Bought);
            assert_eq!(g[0].until, Some(t + Duration::hours(24)));
            Ok(())
        }
    );

    db_test!(lift_removes_only_the_mod_grant, |pool: PgPool| async {
        let t = now();
        extend_bought(&pool, G, A, 24, t).await?;
        grant_mod(&pool, G, A, None, MOD, Some("trusted")).await?;
        let first = grants(&pool, G, A).await?;
        let m = first.iter().find(|g| g.source == GrantSource::Mod).unwrap();
        assert_eq!((m.until, m.reason.as_deref()), (None, Some("trusted")));
        grant_mod(&pool, G, A, Some(t + Duration::days(1)), MOD, None).await?;
        let g = grants(&pool, G, A).await?;
        assert_eq!(g.len(), 2, "the mod grant is upserted, not duplicated");
        let m = g.iter().find(|g| g.source == GrantSource::Mod).unwrap();
        assert_eq!(m.until, Some(t + Duration::days(1)));
        assert_eq!(m.granted_by, Some(MOD));
        assert_eq!(m.reason, None);
        assert!(lift_mod(&pool, G, A).await?);
        assert!(!lift_mod(&pool, G, A).await?);
        let g = grants(&pool, G, A).await?;
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].source, GrantSource::Bought);
        assert!(grants(&pool, G, B).await?.is_empty());
        Ok(())
    });

    db_test!(optout_round_trip, |pool: PgPool| async {
        assert_eq!(
            optout_state(&pool, G, A).await?,
            OptOutState {
                out: false,
                changed_at: None
            }
        );
        let t = now();
        set_opted_out(&pool, G, A, true, t).await?;
        assert_eq!(
            optout_state(&pool, G, A).await?,
            OptOutState {
                out: true,
                changed_at: Some(t)
            }
        );
        assert!(load_opted_out(&pool, G).await?.contains(&A));
        let later = t + Duration::days(8);
        set_opted_out(&pool, G, A, false, later).await?;
        assert_eq!(
            optout_state(&pool, G, A).await?,
            OptOutState {
                out: false,
                changed_at: Some(later)
            }
        );
        assert!(load_opted_out(&pool, G).await?.is_empty());
        Ok(())
    });

    db_test!(
        spending_the_whole_balance_is_allowed,
        |pool: PgPool| async {
            let payload = ActionPayload::Immunity { hours: 24 };
            fund(&pool, A, 50).await;
            let c = charge_for_action(&pool, &immunity_action(A, 50, &payload))
                .await
                .unwrap();
            assert_eq!(c.balance, 0);
            fund(&pool, A, 50).await;
            assert_eq!(transfer(&pool, G, A, B, 50).await.unwrap(), (0, 50));
            assert_eq!(settle_coinflip(&pool, G, B, 50, false).await.unwrap(), 0);
            Ok(())
        }
    );

    db_test!(opposite_transfers_do_not_deadlock, |pool: PgPool| async {
        fund(&pool, A, 1000).await;
        fund(&pool, B, 1000).await;
        for _ in 0..25 {
            let (x, y) = tokio::join!(transfer(&pool, G, A, B, 1), transfer(&pool, G, B, A, 1));
            x.unwrap();
            y.unwrap();
        }
        assert_eq!(
            balance(&pool, G, A).await? + balance(&pool, G, B).await?,
            2000
        );
        Ok(())
    });

    async fn nothing_written(pool: &PgPool) {
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_ledger")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "no ledger rows");
    }

    db_test!(
        transfer_refuses_non_positive_and_self,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            for (to, amount) in [(B, 0), (B, -50), (A, 10), (A, -10)] {
                let e = transfer(&pool, G, A, to, amount).await.unwrap_err();
                assert!(matches!(e, MoveError::Invalid), "{to} {amount}: {e:?}");
            }
            assert_eq!(balance(&pool, G, A).await?, 100);
            assert_eq!(balance(&pool, G, B).await?, 100);
            nothing_written(&pool).await;
            Ok(())
        }
    );

    db_test!(coinflip_refuses_non_positive_bet, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        for (bet, won) in [(0, true), (-50, false), (-50, true)] {
            let e = settle_coinflip(&pool, G, A, bet, won).await.unwrap_err();
            assert!(matches!(e, MoveError::Invalid), "{bet} {won}: {e:?}");
        }
        assert_eq!(balance(&pool, G, A).await?, 100);
        nothing_written(&pool).await;
        Ok(())
    });

    db_test!(charge_refuses_non_positive_price, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = ActionPayload::Immunity { hours: 24 };
        for price in [0, -50] {
            let e = charge_for_action(&pool, &immunity_action(A, price, &payload))
                .await
                .unwrap_err();
            assert!(matches!(e, MoveError::Invalid), "{price}: {e:?}");
        }
        assert_eq!(balance(&pool, G, A).await?, 100);
        nothing_written(&pool).await;
        let actions: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_actions")
            .fetch_one(&pool)
            .await?;
        assert_eq!(actions, 0);
        Ok(())
    });

    db_test!(adjust_refuses_zero_and_overflow, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let e = adjust(&pool, G, A, 0, MOD, "nothing").await.unwrap_err();
        assert!(matches!(e, MoveError::Invalid), "{e:?}");
        let e = adjust(&pool, G, A, i64::MAX, MOD, "huge")
            .await
            .unwrap_err();
        assert!(matches!(e, MoveError::Invalid), "{e:?}");
        assert_eq!(balance(&pool, G, A).await?, 100);
        nothing_written(&pool).await;
        assert_eq!(
            adjust(&pool, G, A, -1, MOD, "negative stays legal")
                .await
                .unwrap(),
            99
        );
        Ok(())
    });

    db_test!(
        finish_action_never_leaves_a_final_state,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            let payload = ActionPayload::Immunity { hours: 24 };
            let c = charge_for_action(&pool, &immunity_action(A, 50, &payload))
                .await
                .unwrap();
            assert_eq!(refund_action(&pool, c.action_id).await?, Some(100));
            finish_action(&pool, c.action_id, ActionState::Active).await?;
            let state: String =
                sqlx::query_scalar("SELECT state FROM economy_actions WHERE id = $1")
                    .bind(c.action_id)
                    .fetch_one(&pool)
                    .await?;
            assert_eq!(state, "failed");
            Ok(())
        }
    );

    const C: UserId = UserId::new(12);
    const T: UserId = UserId::new(20);

    fn aimed<'a>(
        kind: ActionKind,
        buyer: UserId,
        target: UserId,
        price: i64,
        payload: &'a ActionPayload,
        expires_at: Option<DateTime<Utc>>,
    ) -> NewAction<'a> {
        NewAction {
            guild: G,
            kind,
            buyer,
            target: Some(target),
            price,
            note: None,
            signed: false,
            payload,
            expires_at,
        }
    }

    /// Every wallet equals what it was given plus its ledger rows, and the
    /// ledger rows sum to nothing but what the test moved.
    async fn agree(pool: &PgPool, given: &[(UserId, i64)]) {
        for (user, start) in given {
            let delta: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(delta), 0)::bigint FROM economy_ledger WHERE user_id = $1",
            )
            .bind(user.get() as i64)
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(
                balance(pool, G, *user).await.unwrap(),
                start + delta,
                "wallet of {user} disagrees with its ledger"
            );
        }
    }

    async fn state_of(pool: &PgPool, id: i64) -> String {
        sqlx::query_scalar("SELECT state FROM economy_actions WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn reason_of(pool: &PgPool, id: i64) -> Option<String> {
        sqlx::query_scalar("SELECT end_reason FROM economy_actions WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn theme() -> ActionPayload {
        ActionPayload::Theme {
            url: "https://example.com/a".into(),
            start_secs: 0,
        }
    }

    db_test!(
        charge_active_debits_and_starts_active,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            let payload = theme();
            let end = now() + Duration::days(7);
            let a = aimed(ActionKind::Theme, A, T, 60, &payload, Some(end));
            let c = charge_active(&pool, &a, LedgerKind::Purchase)
                .await
                .unwrap();
            assert_eq!(c.balance, 40);
            assert_eq!(balance(&pool, G, A).await?, 40);
            assert_eq!(state_of(&pool, c.action_id).await, "active");
            let row = action_by_id(&pool, G, c.action_id).await?.unwrap();
            assert_eq!(row.expires_at, Some(end));
            assert_eq!(row.target, Some(T));
            assert_eq!(ledger_count(&pool, "purchase").await, 1);
            // A bounty books its own ledger kind.
            let b = aimed(
                ActionKind::Bounty,
                A,
                T,
                40,
                &ActionPayload::Bounty,
                Some(end),
            );
            charge_active(&pool, &b, LedgerKind::BountyPlaced)
                .await
                .unwrap();
            assert_eq!(ledger_count(&pool, "bounty_placed").await, 1);
            // Too poor: nothing written.
            let e = charge_active(&pool, &a, LedgerKind::Purchase)
                .await
                .unwrap_err();
            assert!(matches!(e, MoveError::Insufficient { balance: 0 }), "{e:?}");
            let e = charge_active(
                &pool,
                &aimed(ActionKind::Theme, A, T, 0, &payload, None),
                LedgerKind::Purchase,
            )
            .await
            .unwrap_err();
            assert!(matches!(e, MoveError::Invalid), "{e:?}");
            agree(&pool, &[(A, 100)]).await;
            Ok(())
        }
    );

    db_test!(refund_active_refunds_once, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = theme();
        let a = aimed(ActionKind::Theme, A, T, 60, &payload, Some(now()));
        let c = charge_active(&pool, &a, LedgerKind::Purchase)
            .await
            .unwrap();
        assert_eq!(
            refund_active(&pool, c.action_id, "unplayed").await?,
            Some(100)
        );
        assert_eq!(refund_active(&pool, c.action_id, "unplayed").await?, None);
        assert_eq!(balance(&pool, G, A).await?, 100);
        assert_eq!(state_of(&pool, c.action_id).await, "ended");
        assert_eq!(
            reason_of(&pool, c.action_id).await.as_deref(),
            Some("unplayed")
        );
        assert!(ended(&pool).await?);
        assert_eq!(ledger_count(&pool, "refund").await, 1);
        // A pending row is not active: refund_active leaves it.
        let p = charge_for_action(
            &pool,
            &immunity_action(A, 10, &ActionPayload::Immunity { hours: 1 }),
        )
        .await
        .unwrap();
        assert_eq!(refund_active(&pool, p.action_id, "x").await?, None);
        assert_eq!(state_of(&pool, p.action_id).await, "pending");
        agree(&pool, &[(A, 100)]).await;
        Ok(())
    });

    db_test!(
        finish_action_says_whether_it_moved_the_row,
        |pool: PgPool| async {
            fund(&pool, A, 200).await;
            let payload = theme();
            // The real clock: nothing here may read as past its end.
            let a = aimed(
                ActionKind::Theme,
                A,
                T,
                60,
                &payload,
                Some(Utc::now() + Duration::days(7)),
            );
            let played = charge_active(&pool, &a, LedgerKind::Purchase)
                .await
                .unwrap()
                .action_id;
            let refunded = charge_active(&pool, &a, LedgerKind::Purchase)
                .await
                .unwrap()
                .action_id;
            assert!(finish_action(&pool, played, ActionState::Done).await?);
            assert_eq!(state_of(&pool, played).await, "done");
            assert!(
                !finish_action(&pool, played, ActionState::Done).await?,
                "final: it never moves again"
            );
            // Refunded as it played: the play finds nothing to mark.
            assert_eq!(refund_active(&pool, refunded, "cleansed").await?, Some(140));
            assert!(!finish_action(&pool, refunded, ActionState::Done).await?);
            assert_eq!(state_of(&pool, refunded).await, "ended");
            // A played theme is spent: its end refunds nothing.
            assert_eq!(refund_active(&pool, played, "expired").await?, None);
            assert_eq!(balance(&pool, G, A).await?, 140);
            agree(&pool, &[(A, 200)]).await;
            Ok(())
        }
    );

    async fn place_bounty(pool: &PgPool, buyer: UserId, price: i64) -> i64 {
        let a = aimed(
            ActionKind::Bounty,
            buyer,
            T,
            price,
            &ActionPayload::Bounty,
            Some(Utc::now() + Duration::days(7)),
        );
        charge_active(pool, &a, LedgerKind::BountyPlaced)
            .await
            .unwrap()
            .action_id
    }

    db_test!(claim_skips_the_placers_own_bounty, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let mine = place_bounty(&pool, A, 30).await;
        let theirs = place_bounty(&pool, B, 50).await;
        let total = claim_bounties(&pool, G, T, A).await?;
        assert_eq!(total, 50);
        assert_eq!(state_of(&pool, mine).await, "active");
        assert_eq!(state_of(&pool, theirs).await, "ended");
        assert_eq!(reason_of(&pool, theirs).await.as_deref(), Some("claimed"));
        assert_eq!(balance(&pool, G, A).await?, 70 + 50);
        // One BountyWon row, naming the bounty that paid.
        let won: Vec<(i64, Option<i64>)> =
            sqlx::query_as("SELECT delta, action_id FROM economy_ledger WHERE kind = 'bounty_won'")
                .fetch_all(&pool)
                .await?;
        assert_eq!(won, vec![(50, Some(theirs))]);
        // Another member's bounties on someone else are untouched.
        assert_eq!(claim_bounties(&pool, G, B, C).await?, 0);
        assert_eq!(active_bounties(&pool, G).await?.len(), 1);
        agree(&pool, &[(A, 100), (B, 100)]).await;
        Ok(())
    });

    db_test!(
        claim_pays_every_bounty_one_ledger_row_each,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let one = place_bounty(&pool, A, 30).await;
            let two = place_bounty(&pool, B, 50).await;
            assert_eq!(claim_bounties(&pool, G, T, C).await?, 80);
            assert_eq!(balance(&pool, G, C).await?, 80);
            let ids: Vec<Option<i64>> = sqlx::query_scalar(
                "SELECT action_id FROM economy_ledger WHERE kind = 'bounty_won' ORDER BY id",
            )
            .fetch_all(&pool)
            .await?;
            assert_eq!(ids, vec![Some(one), Some(two)]);
            agree(&pool, &[(A, 100), (B, 100), (C, 0)]).await;
            Ok(())
        }
    );

    db_test!(a_bounty_is_claimed_once, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        place_bounty(&pool, A, 50).await;
        assert_eq!(claim_bounties(&pool, G, T, B).await?, 50);
        assert_eq!(claim_bounties(&pool, G, T, C).await?, 0);
        assert_eq!(balance(&pool, G, B).await?, 50);
        assert_eq!(balance(&pool, G, C).await?, 0);
        assert_eq!(ledger_count(&pool, "bounty_won").await, 1);
        agree(&pool, &[(A, 100), (B, 0), (C, 0)]).await;
        Ok(())
    });

    fn fund_action<'a>(price: i64, payload: &'a ActionPayload) -> NewAction<'a> {
        aimed(
            ActionKind::Rename,
            A,
            T,
            price,
            payload,
            // The database judges "live" by its own clock, so this follows
            // the real one: a fixed date goes stale and the fund reads closed.
            Some(Utc::now() + Duration::hours(24)),
        )
    }

    fn rename() -> ActionPayload {
        ActionPayload::Rename {
            name: "x".into(),
            restore: None,
        }
    }

    db_test!(pledges_cap_at_the_price, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let payload = rename();
        // 20 each at most: the opening 30 is capped to A's share.
        let f = open_fund(&pool, &fund_action(40, &payload), Some((30, true)))
            .await
            .unwrap();
        assert_eq!((f.pledged, f.balance), (20, Some(80)));
        let p = pledge(&pool, f.action_id, G, B, 25, false).await.unwrap();
        assert_eq!(
            p,
            Pledged {
                taken: 20,
                total: 40,
                price: 40,
                filled: true,
                balance: 80
            }
        );
        assert_eq!(state_of(&pool, f.action_id).await, "pending");
        let rows = fund_pledges(&pool, f.action_id).await?;
        assert_eq!(
            rows,
            vec![
                PledgeRow {
                    user: A,
                    amount: 20,
                    signed: true
                },
                PledgeRow {
                    user: B,
                    amount: 20,
                    signed: false
                },
            ]
        );
        // A filled fund takes no more.
        let e = pledge(&pool, f.action_id, G, C, 5, false)
            .await
            .unwrap_err();
        assert!(matches!(e, FundError::Closed), "{e:?}");
        // The ledger names the fund.
        let ids: Vec<Option<i64>> =
            sqlx::query_scalar("SELECT action_id FROM economy_ledger WHERE kind = 'pledge'")
                .fetch_all(&pool)
                .await?;
        assert_eq!(ids, vec![Some(f.action_id); 2]);
        agree(&pool, &[(A, 100), (B, 100)]).await;
        Ok(())
    });

    db_test!(a_short_pledge_leaves_the_fund_open, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = rename();
        let f = open_fund(&pool, &fund_action(40, &payload), None)
            .await
            .unwrap();
        assert_eq!((f.pledged, f.balance), (0, None));
        let p = pledge(&pool, f.action_id, G, A, 15, false).await.unwrap();
        assert_eq!((p.taken, p.total, p.filled), (15, 15, false));
        assert_eq!(state_of(&pool, f.action_id).await, "funding");
        // Too poor to pledge: refused, nothing written.
        let e = pledge(&pool, f.action_id, G, B, 15, false)
            .await
            .unwrap_err();
        assert!(
            matches!(e, FundError::Money(MoveError::Insufficient { balance: 0 })),
            "{e:?}"
        );
        for amount in [0, -5] {
            let e = pledge(&pool, f.action_id, G, A, amount, false)
                .await
                .unwrap_err();
            assert!(matches!(e, FundError::Money(MoveError::Invalid)), "{e:?}");
        }
        // Another server's id is no fund here.
        let e = pledge(&pool, f.action_id, GuildId::new(2), A, 5, false)
            .await
            .unwrap_err();
        assert!(matches!(e, FundError::Closed), "{e:?}");
        assert_eq!(fund_pledges(&pool, f.action_id).await?.len(), 1);
        agree(&pool, &[(A, 100), (B, 0)]).await;
        Ok(())
    });

    db_test!(a_starter_cannot_fill_it_alone, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = rename();
        let f = open_fund(&pool, &fund_action(40, &payload), Some((500, false)))
            .await
            .unwrap();
        // Capped at half the price: it takes a second member.
        assert_eq!(f.pledged, 20);
        assert_eq!(state_of(&pool, f.action_id).await, "funding");
        agree(&pool, &[(A, 100)]).await;
        Ok(())
    });

    db_test!(an_expired_fund_takes_no_pledge, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = rename();
        let mut a = fund_action(40, &payload);
        a.expires_at = Some(Utc::now() - Duration::minutes(1));
        let f = open_fund(&pool, &a, None).await.unwrap();
        let e = pledge(&pool, f.action_id, G, A, 5, false)
            .await
            .unwrap_err();
        assert!(matches!(e, FundError::Closed), "{e:?}");
        assert_eq!(balance(&pool, G, A).await?, 100);
        nothing_written(&pool).await;
        Ok(())
    });

    db_test!(racing_pledges_fill_a_fund_once, |pool: PgPool| async {
        // Open the connections first so the two pledges really overlap.
        let (c1, c2) = (pool.acquire().await?, pool.acquire().await?);
        drop((c1, c2));
        for round in 0..8_u64 {
            let target = UserId::new(100 + round);
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            fund(&pool, C, 100).await;
            let payload = rename();
            let mut a = fund_action(40, &payload);
            a.target = Some(target);
            let f = open_fund(&pool, &a, Some((20, false))).await.unwrap();
            // 20 are missing; each of these alone would fill the fund.
            let (x, y) = tokio::join!(
                pledge(&pool, f.action_id, G, B, 25, false),
                pledge(&pool, f.action_id, G, C, 40, false)
            );
            let (won, lost) = match (x, y) {
                (Ok(w), Err(l)) | (Err(l), Ok(w)) => (w, l),
                (x, y) => panic!("round {round}: exactly one must win: {x:?} / {y:?}"),
            };
            assert!(won.filled, "round {round}");
            assert_eq!((won.taken, won.total, won.price), (20, 40, 40));
            assert!(matches!(lost, FundError::Closed), "{lost:?}");
            let total: i64 = fund_pledges(&pool, f.action_id)
                .await?
                .iter()
                .map(|p| p.amount)
                .sum();
            assert_eq!(total, 40, "round {round}");
            assert_eq!(state_of(&pool, f.action_id).await, "pending");
            // Only the winner paid.
            assert_eq!(
                balance(&pool, G, B).await? + balance(&pool, G, C).await?,
                180
            );
            agree(&pool, &[(A, 100), (B, 100), (C, 100)]).await;
            sqlx::query("DELETE FROM economy_wallets WHERE user_id <> 0")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_ledger")
                .execute(&pool)
                .await?;
        }
        Ok(())
    });

    db_test!(one_open_fund_per_target_and_kind, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let payload = rename();
        let first = open_fund(&pool, &fund_action(40, &payload), Some((10, false)))
            .await
            .unwrap();
        // The same kind on the same target: refused, and nothing is taken.
        let e = open_fund(&pool, &fund_action(40, &payload), Some((10, false)))
            .await
            .unwrap_err();
        assert!(matches!(e, FundError::AlreadyOpen), "{e:?}");
        assert_eq!(balance(&pool, G, A).await?, 90);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_actions")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows, 1);
        // Another kind opens; so does another target.
        let mut other = fund_action(40, &payload);
        other.kind = ActionKind::Clown;
        open_fund(&pool, &other, None).await.unwrap();
        let mut elsewhere = fund_action(40, &payload);
        elsewhere.target = Some(B);
        open_fund(&pool, &elsewhere, None).await.unwrap();
        // Once the first fund ends, the slot is free again.
        assert!(refund_fund(&pool, first.action_id, "expired").await?);
        open_fund(&pool, &fund_action(40, &payload), None)
            .await
            .unwrap();
        agree(&pool, &[(A, 100)]).await;
        Ok(())
    });

    db_test!(refund_fund_pays_back_every_pledger, |pool: PgPool| async {
        for from_pending in [false, true] {
            sqlx::query("DELETE FROM economy_wallets")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_ledger")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_pledges")
                .execute(&pool)
                .await?;
            sqlx::query("DELETE FROM economy_actions")
                .execute(&pool)
                .await?;
            let given = [(A, 100), (B, 100), (C, 100)];
            for (u, n) in given {
                fund(&pool, u, n).await;
            }
            let payload = rename();
            let f = open_fund(&pool, &fund_action(60, &payload), Some((10, false)))
                .await
                .unwrap();
            pledge(&pool, f.action_id, G, B, 20, true).await.unwrap();
            // B pledges twice: two refund rows.
            pledge(&pool, f.action_id, G, B, 5, true).await.unwrap();
            if from_pending {
                pledge(&pool, f.action_id, G, C, 99, false).await.unwrap();
                assert_eq!(state_of(&pool, f.action_id).await, "pending");
            }
            assert!(refund_fund(&pool, f.action_id, "expired").await?);
            assert_eq!(
                state_of(&pool, f.action_id).await,
                if from_pending { "failed" } else { "ended" }
            );
            assert_eq!(
                reason_of(&pool, f.action_id).await.as_deref(),
                Some("expired")
            );
            for (u, n) in given {
                assert_eq!(balance(&pool, G, u).await?, n, "pending={from_pending}");
            }
            let refunds = ledger_count(&pool, "pledge_refund").await;
            assert_eq!(refunds, if from_pending { 4 } else { 3 });
            let stamped: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM economy_pledges WHERE refunded_at IS NOT NULL",
            )
            .fetch_one(&pool)
            .await?;
            assert_eq!(stamped, refunds);
            // Once only.
            assert!(!refund_fund(&pool, f.action_id, "again").await?);
            for (u, n) in given {
                assert_eq!(balance(&pool, G, u).await?, n);
            }
            agree(&pool, &given).await;
        }
        Ok(())
    });

    db_test!(
        a_pending_funded_row_refunds_pledgers_not_the_starter,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let f = open_fund(&pool, &fund_action(40, &payload), Some((10, false)))
                .await
                .unwrap();
            pledge(&pool, f.action_id, G, B, 30, false).await.unwrap();
            // B's share is 20; A tops up their own to fill it.
            pledge(&pool, f.action_id, G, A, 10, false).await.unwrap();
            assert_eq!(state_of(&pool, f.action_id).await, "pending");
            // A (the starter) holds 80, B 80; the row's price is 40.
            let back = refund_action(&pool, f.action_id).await?;
            assert_eq!(
                back,
                Some(100),
                "the starter's balance, with only their own pledges back"
            );
            assert_eq!(balance(&pool, G, A).await?, 100);
            assert_eq!(balance(&pool, G, B).await?, 100);
            assert_eq!(ledger_count(&pool, "refund").await, 0, "no price refund");
            assert_eq!(ledger_count(&pool, "pledge_refund").await, 3);
            assert_eq!(state_of(&pool, f.action_id).await, "failed");
            assert_eq!(refund_action(&pool, f.action_id).await?, None);
            agree(&pool, &[(A, 100), (B, 100)]).await;
            Ok(())
        }
    );

    db_test!(
        refund_fund_leaves_a_plain_pending_row_alone,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            let p = charge_for_action(
                &pool,
                &immunity_action(A, 10, &ActionPayload::Immunity { hours: 1 }),
            )
            .await
            .unwrap();
            assert!(!refund_fund(&pool, p.action_id, "x").await?);
            assert_eq!(state_of(&pool, p.action_id).await, "pending");
            assert_eq!(balance(&pool, G, A).await?, 90);
            Ok(())
        }
    );

    db_test!(
        due_funds_finds_expired_funding_rows_only,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let at = now();
            let mut old = fund_action(40, &payload);
            old.expires_at = Some(at - Duration::hours(1));
            let due = open_fund(&pool, &old, None).await.unwrap();
            let mut fresh = fund_action(40, &payload);
            fresh.target = Some(B);
            fresh.expires_at = Some(at + Duration::hours(1));
            open_fund(&pool, &fresh, None).await.unwrap();
            // An expired row in another state is not a fund.
            let theme = theme();
            let a = aimed(
                ActionKind::Theme,
                A,
                C,
                10,
                &theme,
                Some(at - Duration::hours(1)),
            );
            charge_active(&pool, &a, LedgerKind::Purchase)
                .await
                .unwrap();
            let mut filled = fund_action(10, &payload);
            filled.target = Some(C);
            filled.expires_at = Some(Utc::now() + Duration::hours(1));
            let pend = open_fund(&pool, &filled, Some((5, false))).await.unwrap();
            pledge(&pool, pend.action_id, G, B, 5, false).await.unwrap();
            // Whatever it holds, a filled fund is not due.
            sqlx::query("UPDATE economy_actions SET expires_at = $2 WHERE id = $1")
                .bind(pend.action_id)
                .bind(at - Duration::hours(1))
                .execute(&pool)
                .await?;
            assert_eq!(state_of(&pool, pend.action_id).await, "pending");
            let got = due_funds(&pool, at, 10).await?;
            assert_eq!(got.len(), 1);
            assert_eq!((got[0].0, got[0].1.id), (G, due.action_id));
            assert_eq!(got[0].1.state, ActionState::Funding);
            assert!(due_funds(&pool, at - Duration::days(1), 10)
                .await?
                .is_empty());
            // An unknown kind is skipped, not an error.
            sqlx::query("UPDATE economy_actions SET kind = 'from_the_future', payload = '{\"kind\":\"zzz\"}' WHERE id = $1")
            .bind(due.action_id)
            .execute(&pool)
            .await?;
            assert!(due_funds(&pool, at, 10).await?.is_empty());
            agree(&pool, &[(A, 100)]).await;
            Ok(())
        }
    );

    db_test!(
        open_funds_lists_newest_first_with_totals,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let first = open_fund(&pool, &fund_action(40, &payload), Some((10, false)))
                .await
                .unwrap();
            pledge(&pool, first.action_id, G, B, 5, false)
                .await
                .unwrap();
            let mut other = fund_action(40, &payload);
            other.target = Some(B);
            let second = open_fund(&pool, &other, None).await.unwrap();
            let got = open_funds(&pool, G).await?;
            let shape: Vec<(i64, i64, i64)> = got.iter().map(|(r, n, p)| (r.id, *n, *p)).collect();
            assert_eq!(
                shape,
                vec![(second.action_id, 0, 40), (first.action_id, 15, 40)]
            );
            assert!(open_funds(&pool, GuildId::new(2)).await?.is_empty());
            agree(&pool, &[(A, 100), (B, 100)]).await;
            Ok(())
        }
    );

    db_test!(a_member_cannot_fill_a_fund_alone, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let payload = rename();
        // Price 40: 20 each at most. The opening ask of 30 is capped to 20.
        let f = open_fund(&pool, &fund_action(40, &payload), Some((30, true)))
            .await
            .unwrap();
        assert_eq!((f.pledged, f.balance), (20, Some(80)));
        assert!(matches!(
            pledge(&pool, f.action_id, G, A, 5, false).await,
            Err(FundError::OverShare { max: 20 })
        ));
        assert_eq!(balance(&pool, G, A).await?, 80);
        // The share is across all of a member's pledges: B puts in 8, then
        // 12 more is all that is left of theirs, though the fund needs 12.
        let p = pledge(&pool, f.action_id, G, B, 8, false).await.unwrap();
        assert_eq!((p.taken, p.total, p.filled), (8, 28, false));
        let p = pledge(&pool, f.action_id, G, B, 50, false).await.unwrap();
        assert_eq!((p.taken, p.total, p.filled), (12, 40, true));
        assert!(matches!(
            pledge(&pool, f.action_id, G, B, 1, false).await,
            Err(FundError::Closed)
        ));
        // A fill leaves the row unprepared: pending, with no end.
        let row = action_by_id(&pool, G, f.action_id).await?.unwrap();
        assert_eq!(row.state, ActionState::Pending);
        assert_eq!(row.expires_at, None);
        agree(&pool, &[(A, 100), (B, 100)]).await;
        Ok(())
    });

    db_test!(
        prepare_fund_writes_payload_and_end_together,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let f = open_fund(&pool, &fund_action(40, &payload), Some((20, false)))
                .await
                .unwrap();
            let ready = ActionPayload::Rename {
                name: "y".into(),
                restore: Some("z".into()),
            };
            let end = Utc::now() + Duration::hours(2);
            assert!(
                !prepare_fund(&pool, f.action_id, &ready, Some(end)).await?,
                "a fund that is still filling is not prepared"
            );
            pledge(&pool, f.action_id, G, B, 20, false).await.unwrap();
            assert!(prepare_fund(&pool, f.action_id, &ready, Some(end)).await?);
            let row = action_by_id(&pool, G, f.action_id).await?.unwrap();
            assert_eq!(row.payload, ready);
            assert_eq!(
                row.expires_at.map(|t| t.timestamp_micros()),
                Some(end.timestamp_micros())
            );
            activate_action(&pool, f.action_id, ActionKind::Rename, Some(end)).await?;
            assert!(!prepare_fund(&pool, f.action_id, &payload, None).await?);
            agree(&pool, &[(A, 100), (B, 100)]).await;
            Ok(())
        }
    );

    db_test!(fund_view_reads_one_open_fund, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let payload = rename();
        let f = open_fund(&pool, &fund_action(40, &payload), Some((15, false)))
            .await
            .unwrap();
        pledge(&pool, f.action_id, G, B, 10, false).await.unwrap();
        let v = fund_view(&pool, G, f.action_id, B).await?.unwrap();
        assert_eq!((v.price, v.pledged, v.mine), (40, 25, 10));
        assert_eq!(v.row.id, f.action_id);
        assert_eq!(fund_view(&pool, G, f.action_id, A).await?.unwrap().mine, 15);
        // Not in another server, not a missing id.
        assert!(fund_view(&pool, GuildId::new(2), f.action_id, B)
            .await?
            .is_none());
        assert!(fund_view(&pool, G, f.action_id + 1, B).await?.is_none());
        // Not once it is no longer funding: B's share caps this at 10, and
        // A's last 5 fills it.
        let p = pledge(&pool, f.action_id, G, B, 15, false).await.unwrap();
        assert_eq!((p.taken, p.filled), (10, false));
        let p = pledge(&pool, f.action_id, G, A, 5, false).await.unwrap();
        assert!(p.filled);
        assert!(fund_view(&pool, G, f.action_id, B).await?.is_none());
        Ok(())
    });

    db_test!(set_payload_only_while_pending, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let payload = rename();
        let f = open_fund(&pool, &fund_action(40, &payload), None)
            .await
            .unwrap();
        let new = ActionPayload::Rename {
            name: "y".into(),
            restore: Some("z".into()),
        };
        assert!(
            !set_payload(&pool, f.action_id, &new).await?,
            "funding is not pending"
        );
        pledge(&pool, f.action_id, G, A, 20, false).await.unwrap();
        pledge(&pool, f.action_id, G, B, 20, false).await.unwrap();
        assert!(set_payload(&pool, f.action_id, &new).await?);
        assert_eq!(
            action_by_id(&pool, G, f.action_id).await?.unwrap().payload,
            new
        );
        activate_action(&pool, f.action_id, ActionKind::Rename, None).await?;
        assert!(!set_payload(&pool, f.action_id, &payload).await?);
        assert_eq!(
            action_by_id(&pool, G, f.action_id).await?.unwrap().payload,
            new
        );
        agree(&pool, &[(A, 100), (B, 100)]).await;
        Ok(())
    });

    db_test!(
        active_bounties_lists_only_standing_bounties,
        |pool: PgPool| async {
            fund(&pool, A, 200).await;
            let one = place_bounty(&pool, A, 30).await;
            let two = place_bounty(&pool, A, 40).await;
            let t = theme();
            charge_active(
                &pool,
                &aimed(
                    ActionKind::Theme,
                    A,
                    T,
                    10,
                    &t,
                    Some(Utc::now() + Duration::days(7)),
                ),
                LedgerKind::Purchase,
            )
            .await
            .unwrap();
            refund_active(&pool, one, "expired").await?;
            let got: Vec<i64> = active_bounties(&pool, G)
                .await?
                .iter()
                .map(|r| r.id)
                .collect();
            assert_eq!(got, vec![two]);
            agree(&pool, &[(A, 200)]).await;
            Ok(())
        }
    );

    db_test!(
        reveal_now_covers_themes_dedications_and_bounties,
        |pool: PgPool| async {
            fund(&pool, A, 500).await;
            let end = Some(now() + Duration::days(1));
            let t = theme();
            let theme_id = charge_active(
                &pool,
                &aimed(ActionKind::Theme, A, T, 10, &t, end),
                LedgerKind::Purchase,
            )
            .await
            .unwrap()
            .action_id;
            let bounty_id = place_bounty(&pool, A, 20).await;
            let d = ActionPayload::Dedicate { song: "s".into() };
            let ded = charge_for_action(&pool, &aimed(ActionKind::Dedicate, A, T, 30, &d, None))
                .await
                .unwrap();
            activate_action(&pool, ded.action_id, ActionKind::Dedicate, None).await?;
            // A jump is aimed at nobody; a funding row cannot be revealed yet.
            let payload = rename();
            let f = open_fund(&pool, &fund_action(40, &payload), Some((10, false)))
                .await
                .unwrap();
            assert_eq!(state_of(&pool, f.action_id).await, "funding");
            let got = reveal_candidates(&pool, G, T, None, false, 10).await?;
            let mut ids: Vec<i64> = got.iter().map(|c| c.row.id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![theme_id, bounty_id, ded.action_id]);
            assert!(!ids.contains(&f.action_id));
            agree(&pool, &[(A, 500)]).await;
            Ok(())
        }
    );

    db_test!(
        a_filled_funds_created_at_is_the_fill_time,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let f = open_fund(&pool, &fund_action(40, &payload), Some((20, false)))
                .await
                .unwrap();
            sqlx::query(
                "UPDATE economy_actions SET created_at = now() - interval '1 hour' WHERE id = $1",
            )
            .bind(f.action_id)
            .execute(&pool)
            .await?;
            let p = pledge(&pool, f.action_id, G, B, 30, false).await.unwrap();
            assert!(p.filled);
            let fresh: bool = sqlx::query_scalar(
            "SELECT created_at > now() - interval '30 seconds' FROM economy_actions WHERE id = $1",
        )
        .bind(f.action_id)
        .fetch_one(&pool)
        .await?;
            assert!(fresh, "created_at must restart when the fund fills");
            // A fund that has not filled keeps the time it opened.
            Ok(())
        }
    );

    db_test!(
        expire_fund_leaves_a_filled_fund_alone,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            // Due and unfilled: refunded and ended `unfunded`.
            let mut due = fund_action(40, &payload);
            due.expires_at = Some(Utc::now() + Duration::seconds(1));
            let f = open_fund(&pool, &due, Some((10, false))).await.unwrap();
            pledge(&pool, f.action_id, G, B, 5, false).await.unwrap();
            // Not due yet: left alone.
            assert!(!expire_fund(&pool, f.action_id).await?);
            assert_eq!(state_of(&pool, f.action_id).await, "funding");
            sqlx::query(
                "UPDATE economy_actions SET expires_at = now() - interval '1 minute' WHERE id = $1",
            )
            .bind(f.action_id)
            .execute(&pool)
            .await?;
            assert!(expire_fund(&pool, f.action_id).await?);
            assert_eq!(state_of(&pool, f.action_id).await, "ended");
            assert_eq!(
                reason_of(&pool, f.action_id).await.as_deref(),
                Some("unfunded")
            );
            assert_eq!(ledger_count(&pool, "pledge_refund").await, 2);
            assert!(!expire_fund(&pool, f.action_id).await?);
            // Filled between the sweeper's read and the refund: left alone.
            let mut other = fund_action(40, &payload);
            other.target = Some(B);
            other.expires_at = Some(Utc::now() + Duration::hours(1));
            let g = open_fund(&pool, &other, Some((20, false))).await.unwrap();
            pledge(&pool, g.action_id, G, B, 20, false).await.unwrap();
            assert_eq!(state_of(&pool, g.action_id).await, "pending");
            sqlx::query(
                "UPDATE economy_actions SET expires_at = now() - interval '1 minute' WHERE id = $1",
            )
            .bind(g.action_id)
            .execute(&pool)
            .await?;
            assert!(!expire_fund(&pool, g.action_id).await?);
            assert_eq!(state_of(&pool, g.action_id).await, "pending");
            assert_eq!(balance(&pool, G, A).await?, 80);
            agree(&pool, &[(A, 100), (B, 100)]).await;
            Ok(())
        }
    );

    db_test!(
        refund_active_refuses_a_non_escrow_row,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            let payload = rename();
            let c = charge_for_action(&pool, &aimed(ActionKind::Rename, A, T, 40, &payload, None))
                .await
                .unwrap();
            activate_action(&pool, c.action_id, ActionKind::Rename, Some(now())).await?;
            assert_eq!(state_of(&pool, c.action_id).await, "active");
            assert_eq!(refund_active(&pool, c.action_id, "x").await?, None);
            assert_eq!(state_of(&pool, c.action_id).await, "active");
            assert_eq!(balance(&pool, G, A).await?, 60);
            assert_eq!(ledger_count(&pool, "refund").await, 0);
            agree(&pool, &[(A, 100)]).await;
            Ok(())
        }
    );

    db_test!(
        a_fund_with_every_pledge_refunded_never_refunds_the_price,
        |pool: PgPool| async {
            fund(&pool, A, 100).await;
            fund(&pool, B, 100).await;
            let payload = rename();
            let f = open_fund(&pool, &fund_action(40, &payload), Some((20, false)))
                .await
                .unwrap();
            pledge(&pool, f.action_id, G, B, 20, false).await.unwrap();
            assert_eq!(state_of(&pool, f.action_id).await, "pending");
            // The pledges are already back with their owners (as after an
            // interrupted refund): the row is still a fund, not a purchase.
            sqlx::query("UPDATE economy_pledges SET refunded_at = now()")
                .execute(&pool)
                .await?;
            sqlx::query("UPDATE economy_wallets SET balance = balance + 20")
                .execute(&pool)
                .await?;
            sqlx::query("INSERT INTO economy_ledger (guild_id, user_id, delta, kind, action_id) VALUES (1, 10, 20, 'pledge_refund', $1), (1, 11, 20, 'pledge_refund', $1)")
            .bind(f.action_id)
            .execute(&pool)
            .await?;
            assert_eq!(refund_action(&pool, f.action_id).await?, Some(100));
            assert_eq!(ledger_count(&pool, "refund").await, 0);
            assert_eq!(balance(&pool, G, A).await?, 100);
            assert_eq!(state_of(&pool, f.action_id).await, "failed");
            agree(&pool, &[(A, 100), (B, 100)]).await;
            Ok(())
        }
    );

    db_test!(escrow_without_an_end_is_refused, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        let t = theme();
        let e = charge_active(
            &pool,
            &aimed(ActionKind::Theme, A, T, 10, &t, None),
            LedgerKind::Purchase,
        )
        .await
        .unwrap_err();
        assert!(matches!(e, MoveError::Invalid), "{e:?}");
        let payload = rename();
        let mut open = fund_action(40, &payload);
        open.expires_at = None;
        let e = open_fund(&pool, &open, Some((10, false)))
            .await
            .unwrap_err();
        assert!(matches!(e, FundError::Money(MoveError::Invalid)), "{e:?}");
        let free = fund_action(0, &payload);
        let e = open_fund(&pool, &free, None).await.unwrap_err();
        assert!(matches!(e, FundError::Money(MoveError::Invalid)), "{e:?}");
        assert_eq!(balance(&pool, G, A).await?, 100);
        nothing_written(&pool).await;
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM economy_actions")
            .fetch_one(&pool)
            .await?;
        assert_eq!(rows, 0);
        Ok(())
    });

    db_test!(an_expired_bounty_is_not_collected, |pool: PgPool| async {
        fund(&pool, A, 100).await;
        fund(&pool, B, 100).await;
        let lapsed = charge_active(
            &pool,
            &aimed(
                ActionKind::Bounty,
                A,
                T,
                30,
                &ActionPayload::Bounty,
                Some(Utc::now() - Duration::minutes(1)),
            ),
            LedgerKind::BountyPlaced,
        )
        .await
        .unwrap()
        .action_id;
        let live = place_bounty(&pool, B, 20).await;
        assert_eq!(claim_bounties(&pool, G, T, C).await?, 20);
        assert_eq!(
            state_of(&pool, lapsed).await,
            "active",
            "left for the sweeper"
        );
        assert_eq!(state_of(&pool, live).await, "ended");
        assert_eq!(balance(&pool, G, C).await?, 20);
        // The sweeper then refunds it to its placer.
        assert_eq!(refund_active(&pool, lapsed, "expired").await?, Some(100));
        agree(&pool, &[(A, 100), (B, 100), (C, 0)]).await;
        Ok(())
    });

    db_test!(
        reveal_skips_unplayed_themes_and_unfunded_funds,
        |pool: PgPool| async {
            fund(&pool, A, 500).await;
            let end = Some(Utc::now() + Duration::days(1));
            let t = theme();
            let played = charge_active(
                &pool,
                &aimed(ActionKind::Theme, A, T, 10, &t, end),
                LedgerKind::Purchase,
            )
            .await
            .unwrap()
            .action_id;
            finish_action(&pool, played, ActionState::Done).await?;
            let unplayed = charge_active(
                &pool,
                &aimed(ActionKind::Theme, A, T, 10, &t, end),
                LedgerKind::Purchase,
            )
            .await
            .unwrap()
            .action_id;
            refund_active(&pool, unplayed, "unplayed").await?;
            // A refunded bounty's card was public: it stays revealable.
            let bounty = place_bounty(&pool, A, 20).await;
            refund_active(&pool, bounty, "expired").await?;
            // A fund that never filled was never seen as a prank.
            let payload = rename();
            let mut a = fund_action(40, &payload);
            a.expires_at = Some(Utc::now() + Duration::seconds(1));
            let f = open_fund(&pool, &a, Some((10, false))).await.unwrap();
            sqlx::query(
                "UPDATE economy_actions SET expires_at = now() - interval '1 minute' WHERE id = $1",
            )
            .bind(f.action_id)
            .execute(&pool)
            .await?;
            assert!(expire_fund(&pool, f.action_id).await?);
            let got = reveal_candidates(&pool, G, T, None, false, 10).await?;
            let mut ids: Vec<i64> = got.iter().map(|c| c.row.id).collect();
            ids.sort_unstable();
            assert_eq!(ids, vec![played, bounty]);
            agree(&pool, &[(A, 500)]).await;
            Ok(())
        }
    );
}
