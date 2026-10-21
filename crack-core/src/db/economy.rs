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
        ActionState::Pending.as_str(),
        a.expires_at,
    )
    .fetch_one(&mut *tx)
    .await?;
    write_ledger(
        &mut tx,
        a.guild,
        a.buyer,
        -a.price,
        LedgerKind::Purchase,
        Some(row.id),
    )
    .await?;
    tx.commit().await?;
    Ok(Charged {
        action_id: row.id,
        balance: left,
    })
}

/// Move an action to a new state, stamping `ended_at` when it is final. A
/// row in a final state never moves again.
pub async fn finish_action(pool: &PgPool, action_id: i64, state: ActionState) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE economy_actions
         SET state = $2, ended_at = CASE WHEN $3 THEN now() ELSE ended_at END
         WHERE id = $1 AND state NOT IN ('done', 'ended', 'failed')",
        action_id,
        state.as_str(),
        state.is_final()
    )
    .execute(pool)
    .await?;
    Ok(())
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

/// The running (`active` or `ending`) actions of these kinds aimed at
/// `target`, oldest first. A row this build cannot read is skipped.
pub async fn active_of_kind(
    pool: &PgPool,
    guild: GuildId,
    target: UserId,
    kinds: &[ActionKind],
) -> sqlx::Result<Vec<ActionRow>> {
    let keys: Vec<String> = kinds.iter().map(|k| k.key().to_string()).collect();
    let rows = sqlx::query!(
        r#"SELECT id, kind, buyer_id, target_id, payload AS "payload: Json<ActionPayload>",
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
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(ActionRow {
                id: r.id,
                kind: ActionKind::from_key(&r.kind)?,
                buyer: uid(r.buyer_id)?,
                target: r.target_id.and_then(uid),
                payload: r.payload.0,
                state: ActionState::parse(&r.state)?,
                expires_at: r.expires_at,
                note: r.note,
                signed: r.signed,
                revealed_at: r.revealed_at,
            })
        })
        .collect())
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
    /// A row this build cannot read (unknown kind, bad id) is skipped.
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
           ORDER BY expires_at, id
           LIMIT $2"#,
        now,
        limit
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
           ORDER BY expires_at, id"#,
        guild.get() as i64,
        now
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
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
           ORDER BY id"#,
        guild.get() as i64,
        user.get() as i64
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
           WHERE guild_id = $1 AND state = 'ending'
           ORDER BY id"#,
        guild.get() as i64
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// One server's `pending` rows charged before `cutoff`: a purchase between
/// its charge and its activate is younger than that and is left alone.
pub async fn pending_for_guild(
    pool: &PgPool,
    guild: GuildId,
    cutoff: DateTime<Utc>,
) -> sqlx::Result<Vec<ActionRow>> {
    let raw = sqlx::query_as!(
        RawAction,
        r#"SELECT id, guild_id, kind, buyer_id, target_id,
                  payload AS "payload: Json<ActionPayload>",
                  state, expires_at, note, signed, revealed_at
           FROM economy_actions
           WHERE guild_id = $1 AND state = 'pending' AND created_at < $2
           ORDER BY id"#,
        guild.get() as i64,
        cutoff
    )
    .fetch_all(pool)
    .await?;
    Ok(rows_of(raw).into_iter().map(|(_, r)| r).collect())
}

/// Move a running (`active` or `ending`) row to `Ended` (final, stamped) or
/// `Ending` (waits for the member's next voice join), with the reason. False
/// when the row was not running, or `state` is neither of those: nothing
/// changed.
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
         SET state = $2, end_reason = $3,
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
}
