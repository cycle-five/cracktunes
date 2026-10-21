//! Timed pranks end on time: the sweeper, the undo, the restart reconcile and
//! the supervisor that makes a dead background loop loud.
//!
//! `undo_plan` is pure. `finish`, `sweep`, `on_voice_join` and `reconcile`
//! act through the [`EconomyDiscord`] seam and read the cache through
//! [`MemberView`], so tests drive them without Discord.

use super::discord::{EconDiscordError, EconomyDiscord, LiveEconomyDiscord};
use super::settings::ActionKind;
use super::wallet::{ActionPayload, ActionState};
use crate::db::economy::{self, ActionRow};
use crate::Data;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serenity::all::{Cache, ChannelId, GuildId, Http, UserId};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::MissedTickBehavior;

/// How often the sweeper looks for rows whose time is up.
pub const SWEEP_EVERY: Duration = Duration::from_secs(15);
/// How many due rows one sweep takes.
const SWEEP_BATCH: i64 = 500;
/// After a failed undo, the first retry waits this long; each further
/// failure doubles it, up to [`UNDO_BACKOFF_MAX`].
const UNDO_BACKOFF_BASE: ChronoDuration = ChronoDuration::seconds(15);
const UNDO_BACKOFF_MAX: ChronoDuration = ChronoDuration::minutes(5);
/// A row whose undo keeps failing is given up on this long after it was due.
const UNDO_GIVE_UP: ChronoDuration = ChronoDuration::hours(1);
/// A `pending` row of this process younger than this may be a purchase
/// still in flight (on the database's clock).
const PENDING_GRACE_SECS: i64 = 120;

/// What putting a prank right takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoPlan {
    /// `None` clears the nickname.
    SetNick(Option<String>),
    SetMute(bool),
    SetDeaf(bool),
    /// Drag, jail, mirror, lock (ends by itself), reveal: nothing to restore.
    Nothing,
}

/// The undo for a row: only what the bot changed, put back to what the
/// payload captured.
pub fn undo_plan(row: &ActionRow) -> UndoPlan {
    match &row.payload {
        ActionPayload::Rename { restore, .. } | ActionPayload::Clown { restore, .. } => {
            UndoPlan::SetNick(restore.clone())
        },
        ActionPayload::Mute => UndoPlan::SetMute(false),
        ActionPayload::Deafen => UndoPlan::SetDeaf(false),
        ActionPayload::Lock { .. }
        | ActionPayload::Drag { .. }
        | ActionPayload::Jail { .. }
        | ActionPayload::Mirror
        | ActionPayload::Immunity { .. }
        | ActionPayload::Cleanse
        | ActionPayload::Reveal { .. }
        | ActionPayload::Theme { .. }
        | ActionPayload::Dedicate { .. }
        | ActionPayload::Jump { .. }
        | ActionPayload::Bounty => UndoPlan::Nothing,
    }
}

async fn apply(
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    target: UserId,
    kind: ActionKind,
    plan: &UndoPlan,
) -> Result<(), EconDiscordError> {
    match plan {
        UndoPlan::SetNick(nick) => discord.set_nick(guild, target, nick.as_deref(), kind).await,
        UndoPlan::SetMute(on) => discord.set_mute(guild, target, *on, kind).await,
        UndoPlan::SetDeaf(on) => discord.set_deaf(guild, target, *on, kind).await,
        UndoPlan::Nothing => Ok(()),
    }
}

/// What [`finish`] did with a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finished {
    /// Undone (or nothing to undo) and marked `ended`.
    Ended,
    /// The member is not in voice: marked `ending`, finished at their next join.
    Ending,
    /// An early end whose undo failed: its end was brought forward to now, so
    /// the sweeper retries it every tick, and gives up an hour later.
    Scheduled,
    /// Nothing changed: the row was no longer running, or a read or write
    /// failed.
    Skipped,
    /// The undo failed in Discord and the row is left as it was, to be tried
    /// again (the sweeper backs off; an `ending` row waits for a join).
    Retry,
}

impl Finished {
    /// Whether this call ended the row or took charge of ending it.
    pub fn took_effect(self) -> bool {
        !matches!(self, Finished::Skipped | Finished::Retry)
    }
}

/// End a running row: undo what the bot did, then mark it.
///
/// A row that is no longer `active` or `ending` in the database (a mod
/// override or a cleanse got there first) is left alone and sends nothing to
/// Discord: the state is read fresh before the undo, and the final write is
/// guarded on it too.
///
/// - undone, or nothing to undo: `ended(reason)`;
/// - member not in voice: `ending`, finished at their next voice join;
/// - member left the server: `ended("left")`;
/// - anything else (missing permission): the sweeper's own retry leaves the
///   row as it is for the next tick, then `ended("undo_failed")` an hour after
///   it was due. An early end (any `reason` but `expired`) of an `active` row
///   instead schedules it: its end moves to now with `reason` stored, so the
///   sweeper retries it within a tick and the hour counts from now.
pub async fn finish(
    data: &Data,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    row: &ActionRow,
    reason: &str,
    now: DateTime<Utc>,
) -> Finished {
    let Some(pool) = data.database_pool.as_ref() else {
        return Finished::Skipped;
    };
    // Fresh, not the caller's copy: the row may have been ended since.
    let live = match economy::action_state(pool, row.id).await {
        Ok(Some(live @ (ActionState::Active | ActionState::Ending))) => live,
        Ok(_) => return Finished::Skipped,
        Err(err) => {
            tracing::warn!("economy: reading action {} failed: {err}", row.id);
            return Finished::Skipped;
        },
    };
    // Escrow (a theme, a bounty) changed nothing in Discord, so there is
    // nothing to undo: its end is the buyer's refund. This never reads
    // `undo_plan` and never calls Discord.
    if row.kind.is_escrow() {
        return match economy::refund_active(pool, row.id, reason).await {
            Ok(Some(_)) => Finished::Ended,
            Ok(None) => Finished::Skipped,
            Err(err) => {
                tracing::warn!("economy: refunding action {} failed: {err}", row.id);
                Finished::Skipped
            },
        };
    }
    let plan = undo_plan(row);
    let outcome = match (&plan, row.target) {
        (UndoPlan::Nothing, _) => Ok(()),
        (_, Some(target)) => apply(discord, guild, target, row.kind, &plan).await,
        (_, None) => {
            tracing::warn!("economy: action {} has no target to restore", row.id);
            Ok(())
        },
    };
    let (state, why) = match outcome {
        Ok(()) => (ActionState::Ended, reason),
        Err(EconDiscordError::NotConnected) => (ActionState::Ending, reason),
        Err(EconDiscordError::UnknownMember) => (ActionState::Ended, "left"),
        Err(err) => {
            // Only a row that is running gives up: an `ending` one waits for
            // the member and is retried at every join and reconcile.
            if live == ActionState::Active
                && row
                    .expires_at
                    .is_some_and(|due| now.signed_duration_since(due) > UNDO_GIVE_UP)
            {
                tracing::error!(
                    "economy: giving up undoing action {} ({:?}) in {guild}: {err:?}",
                    row.id,
                    row.kind
                );
                (ActionState::Ended, "undo_failed")
            } else if live == ActionState::Active && reason != "expired" {
                tracing::warn!(
                    "economy: undoing action {} ({:?}) in {guild} for {reason} failed, scheduling a retry: {err:?}",
                    row.id,
                    row.kind
                );
                return match economy::schedule_early_end(pool, row.id, reason, now).await {
                    Ok(true) => Finished::Scheduled,
                    Ok(false) => Finished::Skipped,
                    Err(err) => {
                        tracing::warn!("economy: scheduling action {} failed: {err}", row.id);
                        Finished::Skipped
                    },
                };
            } else {
                tracing::warn!(
                    "economy: undoing action {} ({:?}) in {guild} failed, will retry: {err:?}",
                    row.id,
                    row.kind
                );
                return Finished::Retry;
            }
        },
    };
    match economy::end_action(pool, row.id, state, why).await {
        Ok(true) if state == ActionState::Ending => Finished::Ending,
        Ok(true) => Finished::Ended,
        Ok(false) => Finished::Skipped,
        Err(err) => {
            tracing::warn!("economy: marking action {} {state:?} failed: {err}", row.id);
            Finished::Skipped
        },
    }
}

/// What [`end_running_pranks`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndedPranks {
    /// Rows this call ended, set aside for the member's next join, or
    /// scheduled.
    pub all: usize,
    /// Of those, the ones that were `active`: an `ending` row was already on
    /// its way out, so a cleanse doesn't count it as something it ended.
    pub were_active: usize,
}

/// End every running (`active` or `ending`) prank aimed at `target`, each
/// undone as if it had expired, with `reason` as the end reason. A standing
/// mirror is kept. Locks go first so the restoring rename is not fought by an
/// active lock. A row that cannot be undone right now is warned about by
/// [`finish`] and scheduled for the sweeper.
pub async fn end_running_pranks(
    data: &Data,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    target: UserId,
    reason: &str,
    now: DateTime<Utc>,
) -> Result<EndedPranks, sqlx::Error> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Ok(EndedPranks::default());
    };
    let mut rows =
        economy::active_of_kind(pool, guild, target, ActionKind::RUNNING_ON_TARGET).await?;
    rows.sort_by_key(|r| (r.kind != ActionKind::Lock, r.id));
    let mut ended = EndedPranks::default();
    for row in &rows {
        if finish(data, discord, guild, row, reason, now)
            .await
            .took_effect()
        {
            ended.all += 1;
            if row.state == ActionState::Active {
                ended.were_active += 1;
            }
        }
    }
    Ok(ended)
}

/// How long the sweeper waits before retrying an undo that has failed
/// `failures` times (1 or more): 15 s, 30 s, 1 min, 2 min, 4 min, then 5 min.
pub fn undo_backoff(failures: u32) -> ChronoDuration {
    let doublings = failures.saturating_sub(1).min(16);
    (UNDO_BACKOFF_BASE * (1 << doublings)).min(UNDO_BACKOFF_MAX)
}

/// Finish every row whose time is up, in every server, the economy on or off.
///
/// Rows in a server the cache doesn't hold (`held` says no: not arrived yet,
/// or the bot left) are skipped without a Discord call; reconcile finishes
/// them when the server arrives. A row whose undo failed waits out its
/// backoff ([`undo_backoff`]) before the next try, so a few broken rows
/// neither hold up the rest nor spend Discord's invalid-request budget. The
/// hour's give-up is unchanged.
pub async fn sweep(
    data: &Data,
    discord: &dyn EconomyDiscord,
    now: DateTime<Utc>,
    held: &(dyn Fn(GuildId) -> bool + Sync),
) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    let due = match economy::due_actions(pool, now, SWEEP_BATCH).await {
        Ok(due) => due,
        Err(err) => {
            tracing::warn!("economy: reading due actions failed: {err}");
            // Funds below are still due, whatever happened here.
            return sweep_funds(pool, now).await;
        },
    };
    // Every due row was seen: a backoff for a row not among them is for one
    // that has ended.
    if due.len() < SWEEP_BATCH as usize {
        let ids: std::collections::HashSet<i64> = due.iter().map(|(_, r)| r.id).collect();
        data.economy.keep_undo_backoff(&ids);
    }
    for (guild, row) in due {
        // Escrow makes no Discord call, so the cache has no say in it.
        if (!row.kind.is_escrow() && !held(guild)) || !data.economy.undo_due(row.id, now) {
            continue;
        }
        match finish(data, discord, guild, &row, "expired", now).await {
            Finished::Retry => data.economy.undo_failed(row.id, now),
            _ => data.economy.undo_settled(row.id),
        }
    }
    sweep_funds(pool, now).await;
}

/// Funds that did not fill: refund every pledge. No Discord call, so no
/// cached-guild check and no backoff. `expire_fund` re-checks the clock under
/// the row lock, so a fund filled a moment ago is left alone.
async fn sweep_funds(pool: &sqlx::PgPool, now: DateTime<Utc>) {
    match economy::due_funds(pool, now, SWEEP_BATCH).await {
        Ok(funds) => {
            for (guild, row) in funds {
                if let Err(err) = economy::expire_fund(pool, row.id).await {
                    tracing::warn!("economy: expiring fund {} in {guild} failed: {err}", row.id);
                }
            }
        },
        Err(err) => tracing::warn!("economy: reading due funds failed: {err}"),
    }
}

/// A member joined voice: finish their `ending` rows.
pub async fn on_voice_join(
    data: &Data,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    user: UserId,
    now: DateTime<Utc>,
) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    match economy::ending_for(pool, guild, user).await {
        Ok(rows) => {
            for row in rows {
                finish(data, discord, guild, &row, "expired", now).await;
            }
        },
        Err(err) => tracing::warn!("economy: reading ending actions of {user} failed: {err}"),
    }
}

/// A member's voice state as the cache holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceFacts {
    pub channel: Option<ChannelId>,
    /// Server mute.
    pub mute: bool,
    /// Server deafen.
    pub deaf: bool,
}

/// What reconcile can learn about a member without asking Discord. `None`
/// means the cache cannot say.
pub trait MemberView: Send + Sync {
    /// The member's nickname; `Some(None)` is a member with no nickname.
    fn nick(&self, guild: GuildId, user: UserId) -> Option<Option<String>>;
    fn voice(&self, guild: GuildId, user: UserId) -> Option<VoiceFacts>;
}

impl MemberView for Cache {
    fn nick(&self, guild: GuildId, user: UserId) -> Option<Option<String>> {
        let g = self.guild(guild)?;
        let m = g.members.get(&user)?;
        Some(m.nick.as_deref().map(str::to_owned))
    }

    fn voice(&self, guild: GuildId, user: UserId) -> Option<VoiceFacts> {
        let g = self.guild(guild)?;
        let vs = g.voice_states.get(&user)?;
        Some(VoiceFacts {
            channel: vs.channel_id,
            mute: vs.mute(),
            deaf: vs.deaf(),
        })
    }
}

/// Did a purchase that was left `pending` take effect? `None` when the cache cannot
/// say.
fn landed(view: &dyn MemberView, guild: GuildId, row: &ActionRow) -> Option<bool> {
    match &row.payload {
        // The purchase is the grant, written after the charge: a row still
        // pending after a restart never got it. These aim at no one.
        ActionPayload::Immunity { .. }
        | ActionPayload::Cleanse
        | ActionPayload::Reveal { .. }
        | ActionPayload::Theme { .. }
        | ActionPayload::Dedicate { .. }
        | ActionPayload::Jump { .. }
        | ActionPayload::Bounty => {
            return Some(false);
        },
        // Neither changes anything on Discord at purchase.
        ActionPayload::Lock { .. } | ActionPayload::Mirror => return Some(true),
        _ => {},
    }
    let Some(target) = row.target else {
        return Some(false);
    };
    match &row.payload {
        ActionPayload::Rename { name, .. } => {
            let nick = view.nick(guild, target)?;
            Some(nick.as_deref() == Some(name.as_str()))
        },
        ActionPayload::Clown { applied, .. } => {
            let nick = view.nick(guild, target)?;
            Some(nick.as_deref() == Some(applied.as_str()))
        },
        ActionPayload::Mute => Some(view.voice(guild, target)?.mute),
        ActionPayload::Deafen => Some(view.voice(guild, target)?.deaf),
        ActionPayload::Drag { to, .. } => Some(view.voice(guild, target)?.channel == Some(*to)),
        ActionPayload::Jail { jail } => Some(view.voice(guild, target)?.channel == Some(*jail)),
        // Decided above.
        ActionPayload::Immunity { .. }
        | ActionPayload::Cleanse
        | ActionPayload::Reveal { .. }
        | ActionPayload::Theme { .. }
        | ActionPayload::Dedicate { .. }
        | ActionPayload::Jump { .. }
        | ActionPayload::Bounty
        | ActionPayload::Lock { .. }
        | ActionPayload::Mirror => None,
    }
}

/// Resolve one `pending` row after a restart: landed rows become active (or
/// done), the rest are refunded. When the cache cannot tell, refund and put
/// back what the bot may have done, so nothing lingers unowned.
async fn resolve_pending(
    data: &Data,
    discord: &dyn EconomyDiscord,
    view: &dyn MemberView,
    guild: GuildId,
    row: &ActionRow,
) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    // A timed prank is charged with its end already set, so a pending one
    // with none was never prepared: a filled crowdfund waits that way until
    // its executor writes the payload and the end together, and the executor
    // never acts before that write. Nothing happened to the member; refund it
    // (pledge-aware), with no undo.
    if row.kind.is_timed() && row.expires_at.is_none() {
        if let Err(err) = economy::refund_action(pool, row.id).await {
            tracing::warn!(
                "economy: refunding unprepared action {} failed: {err}",
                row.id
            );
        }
        return;
    }
    let verdict = landed(view, guild, row);
    // A server mute or deafen outlives a disconnect, so a member who is not in
    // voice now may well be muted. Refunding would leave nothing owning it;
    // keep the row and let the expiry (or their next join) undo it.
    let owned = verdict.is_none() && matches!(row.kind, ActionKind::Mute | ActionKind::Deafen);
    if verdict == Some(true) || owned {
        if let Err(err) = economy::activate_action(pool, row.id, row.kind, row.expires_at).await {
            tracing::warn!("economy: activating action {} failed: {err}", row.id);
        }
        return;
    }
    // Known gap: a row left pending because `finish_blocked` failed (a
    // blocked mirror bounce) looks not-landed here and is refunded.
    match economy::refund_action(pool, row.id).await {
        Ok(Some(_)) => {},
        // Someone else already resolved the row: its effect is theirs.
        Ok(None) => return,
        Err(err) => {
            tracing::warn!("economy: refunding action {} failed: {err}", row.id);
            return;
        },
    }
    if verdict.is_none() {
        if let Some(target) = row.target {
            let plan = undo_plan(row);
            if let Err(err) = apply(discord, guild, target, row.kind, &plan).await {
                tracing::warn!(
                    "economy: putting back unresolved action {} in {guild} failed: {err:?}",
                    row.id
                );
            }
        }
    }
}

/// The name a rename or clown row put on its member.
fn prank_set_name(row: &ActionRow) -> Option<&str> {
    match &row.payload {
        ActionPayload::Rename { name, .. } => Some(name),
        ActionPayload::Clown { applied, .. } => Some(applied),
        _ => None,
    }
}

/// Whether a running rename or clown's member, as the cache shows them now,
/// wears a name other than the one the prank set: someone changed it while
/// the bot couldn't see who. `false` when the cache doesn't hold them.
fn renamed_while_away(view: &dyn MemberView, guild: GuildId, row: &ActionRow) -> bool {
    let (Some(set), Some(target)) = (prank_set_name(row), row.target) else {
        return false;
    };
    view.nick(guild, target)
        .is_some_and(|nick| nick.as_deref() != Some(set))
}

/// Whether reconcile may judge a name prank bought at `created_at` against
/// the cache, as I4 judges a pending row: one a previous process left
/// (bought before `process_start`), or one older than the grace on the
/// database's clock. A younger one may be a rename this process has just
/// made, which a GuildCreate in the middle of a run can predate.
fn past_the_grace(
    created_at: DateTime<Utc>,
    process_start: Option<DateTime<Utc>>,
    db_now: DateTime<Utc>,
) -> bool {
    let grace = db_now - ChronoDuration::seconds(PENDING_GRACE_SECS);
    created_at < process_start.map_or(grace, |start| start.max(grace))
}

/// End the running renames and clowns whose name was changed while the bot
/// was away, `overridden` and with no restore: the change stands, whoever
/// made it (a moderator's must never be undone, and the audit entry that
/// would say whose is gone). A lock riding on such a prank ends with it, as a
/// live override ends both.
///
/// Only rows past the grace ([`past_the_grace`]) are judged, and a mismatch
/// in the cache is confirmed with one fresh read from Discord first: the
/// cache can lag a nickname the bot itself just set (a purchase, or a lock's
/// revert). If that read fails the row is left alone; the sweeper ends it on
/// time anyway.
async fn let_away_renames_stand(
    data: &Data,
    discord: &dyn EconomyDiscord,
    view: &dyn MemberView,
    guild: GuildId,
) -> Result<(), sqlx::Error> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Ok(());
    };
    let db_now = economy::db_now(pool).await?;
    let started = data.economy.process_start();
    let rows =
        economy::active_for_guild(pool, guild, &[ActionKind::Rename, ActionKind::Clown]).await?;
    let judged = rows.iter().filter(|(row, created_at)| {
        past_the_grace(*created_at, started, db_now) && renamed_while_away(view, guild, row)
    });
    for (row, _) in judged {
        let (Some(set), Some(target)) = (prank_set_name(row), row.target) else {
            continue;
        };
        match discord.member_nick(guild, target).await {
            Ok(nick) if nick.as_deref() != Some(set) => {},
            // The cache was behind: the prank's name is on them.
            Ok(_) => continue,
            Err(err) => {
                tracing::warn!(
                    "economy: confirming {target}'s nickname in {guild} failed, leaving action {}: {err:?}",
                    row.id
                );
                continue;
            },
        }
        economy::end_action(pool, row.id, ActionState::Ended, "overridden").await?;
        if let Some(target) = row.target {
            for lock in economy::active_of_kind(pool, guild, target, &[ActionKind::Lock]).await? {
                if lock.state == ActionState::Active {
                    economy::end_action(pool, lock.id, ActionState::Ended, "overridden").await?;
                }
            }
        }
    }
    Ok(())
}

/// A server arrived (a start, a reconnect, a rejoin): resolve purchases the
/// last run left `pending`, let a nickname changed while the bot was away
/// stand, finish rows that came due while the bot was down, and finish
/// `ending` rows of members who are in voice now.
pub async fn reconcile(
    data: &Data,
    discord: &dyn EconomyDiscord,
    view: &dyn MemberView,
    guild: GuildId,
    now: DateTime<Utc>,
) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    // Every row a previous process left, whatever its age (a crash or a
    // redeploy between charge and activate); of this process's own, only
    // those older than the grace: a purchase between its charge and its
    // activate must not be refunded from under itself by a mid-run
    // GuildCreate.
    let started = data.economy.process_start();
    let grace = PENDING_GRACE_SECS as f64;
    match economy::pending_for_guild(pool, guild, started, grace).await {
        Ok(rows) => {
            for row in rows {
                resolve_pending(data, discord, view, guild, &row).await;
            }
        },
        Err(err) => tracing::warn!("economy: reading pending actions of {guild} failed: {err}"),
    }
    // Before the overdue ones: an expiry must not restore over such a name.
    if let Err(err) = let_away_renames_stand(data, discord, view, guild).await {
        tracing::warn!("economy: checking the running names in {guild} failed: {err}");
    }
    match economy::overdue_for_guild(pool, guild, now).await {
        Ok(rows) => {
            for row in rows {
                finish(data, discord, guild, &row, "expired", now).await;
            }
        },
        Err(err) => tracing::warn!("economy: reading overdue actions of {guild} failed: {err}"),
    }
    match economy::ending_for_guild(pool, guild).await {
        Ok(rows) => {
            for row in rows {
                let in_voice = row
                    .target
                    .and_then(|t| view.voice(guild, t))
                    .is_some_and(|v| v.channel.is_some());
                if in_voice {
                    finish(data, discord, guild, &row, "expired", now).await;
                }
            }
        },
        Err(err) => tracing::warn!("economy: reading ending actions of {guild} failed: {err}"),
    }
}

/// Run `fut` as a task and log at error when it returns or panics: the loops
/// handed to it are meant to run for the life of the process.
pub fn spawn_supervised<F>(name: &'static str, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawn_supervised_with(name, fut, true, |line| tracing::error!("{line}"));
}

/// Run a one-off `fut` as a task and log at error if it panics. It is meant
/// to end, so ending is not logged.
pub fn spawn_one_shot<F>(name: &'static str, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawn_supervised_with(name, fut, false, |line| tracing::error!("{line}"));
}

/// `forever`: whether ending is worth a line (a loop that should never end).
fn spawn_supervised_with<F, L>(name: &'static str, fut: F, forever: bool, log: L)
where
    F: Future<Output = ()> + Send + 'static,
    L: Fn(&str) + Send + 'static,
{
    let task = tokio::spawn(fut);
    tokio::spawn(async move {
        match task.await {
            Ok(()) if !forever => {},
            Ok(()) => log(&format!(
                "economy: task {name} ended; it should run forever"
            )),
            Err(err) if err.is_panic() => {
                let payload = err.into_panic();
                let why = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "no message".to_string());
                log(&format!("economy: task {name} panicked: {why}"));
            },
            Err(_) => log(&format!("economy: task {name} was cancelled")),
        }
    });
}

/// Spawns the 15 s sweeper, the first time only and only with a database.
pub fn start_sweeper(data: Data, http: Arc<Http>, cache: Arc<Cache>) {
    if !data.economy.start_sweeper_once() {
        return;
    }
    if data.database_pool.is_none() {
        return;
    }
    spawn_supervised("sweeper", async move {
        let discord = LiveEconomyDiscord { http };
        let mut tick = tokio::time::interval(SWEEP_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let held = |g: GuildId| cache.guild(g).is_some();
            sweep(&data, &discord, Utc::now(), &held).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::discord::fake::{Call, CallKind, FakeEconomyDiscord};
    use crate::economy::settings::EconomySettings;
    use crate::DataInner;
    use sqlx::{types::Json, PgPool};
    use std::collections::HashMap;
    use std::sync::Mutex;

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(10);
    const BUYER: UserId = UserId::new(20);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }
    fn mins(n: i64) -> ChronoDuration {
        ChronoDuration::minutes(n)
    }
    fn ch(n: u64) -> ChannelId {
        ChannelId::new(n)
    }

    fn row(kind: ActionKind, payload: ActionPayload) -> ActionRow {
        ActionRow {
            id: 1,
            kind,
            buyer: BUYER,
            target: Some(U),
            payload,
            state: ActionState::Active,
            expires_at: Some(t0()),
            note: None,
            signed: false,
            revealed_at: None,
        }
    }

    #[test]
    fn undo_plan_per_kind() {
        use ActionKind::*;
        let plan = |k, p| undo_plan(&row(k, p));
        assert_eq!(
            plan(
                Rename,
                ActionPayload::Rename {
                    name: "x".into(),
                    restore: Some("Al".into())
                }
            ),
            UndoPlan::SetNick(Some("Al".into()))
        );
        assert_eq!(
            plan(
                Rename,
                ActionPayload::Rename {
                    name: "x".into(),
                    restore: None
                }
            ),
            UndoPlan::SetNick(None)
        );
        assert_eq!(
            plan(
                Clown,
                ActionPayload::Clown {
                    applied: "Al 🤡".into(),
                    restore: Some("Al".into())
                }
            ),
            UndoPlan::SetNick(Some("Al".into()))
        );
        assert_eq!(plan(Mute, ActionPayload::Mute), UndoPlan::SetMute(false));
        assert_eq!(
            plan(Deafen, ActionPayload::Deafen),
            UndoPlan::SetDeaf(false)
        );
        for (k, p) in [
            (Lock, ActionPayload::Lock { locked: None }),
            (
                Drag,
                ActionPayload::Drag {
                    from: ch(1),
                    to: ch(2),
                },
            ),
            (Jail, ActionPayload::Jail { jail: ch(3) }),
            (Mirror, ActionPayload::Mirror),
            (Reveal, ActionPayload::Reveal { action_id: 1 }),
        ] {
            assert_eq!(plan(k, p), UndoPlan::Nothing, "{k:?}");
        }
    }

    #[test]
    fn undo_backoff_doubles_from_15s_and_stops_at_5min() {
        let secs = |n: u32| undo_backoff(n).num_seconds();
        assert_eq!(
            (1..=8).map(secs).collect::<Vec<_>>(),
            vec![15, 30, 60, 120, 240, 300, 300, 300]
        );
        // Far past any count a real row reaches, still capped (no overflow).
        assert_eq!(secs(u32::MAX), 300);
    }

    #[tokio::test]
    async fn a_supervised_task_that_ends_is_logged() {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        spawn_supervised_with("loop", async {}, true, move |l| {
            sink.lock().unwrap().push(l.to_string())
        });
        let sink = lines.clone();
        spawn_supervised_with(
            "boom",
            async {
                panic!("on purpose");
            },
            true,
            move |l| sink.lock().unwrap().push(l.to_string()),
        );
        for _ in 0..100 {
            if lines.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut got = lines.lock().unwrap().clone();
        got.sort();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            got[0].contains("boom") && got[0].contains("panicked: on purpose"),
            "{got:?}"
        );
        assert!(
            got[1].contains("loop") && got[1].contains("ended"),
            "{got:?}"
        );
    }

    #[tokio::test]
    async fn a_supervised_task_that_keeps_running_is_not_logged() {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        spawn_supervised_with("loop", std::future::pending(), true, move |l| {
            sink.lock().unwrap().push(l.to_string())
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(lines.lock().unwrap().is_empty());
    }

    /// A one-shot task (a theme played on a join) ends quietly, but its
    /// panic is still loud.
    #[tokio::test]
    async fn a_one_shot_task_is_logged_only_when_it_panics() {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        spawn_supervised_with("theme", async {}, false, move |l| {
            sink.lock().unwrap().push(l.to_string())
        });
        let sink = lines.clone();
        spawn_supervised_with(
            "theme",
            async {
                panic!("on purpose");
            },
            false,
            move |l| sink.lock().unwrap().push(l.to_string()),
        );
        for _ in 0..100 {
            if !lines.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Long enough for the quiet one to have ended too.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let got = lines.lock().unwrap().clone();
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].contains("theme panicked: on purpose"), "{got:?}");
    }

    // A cache stand-in.
    #[derive(Default)]
    struct FakeView {
        nicks: HashMap<UserId, Option<String>>,
        voice: HashMap<UserId, VoiceFacts>,
    }
    impl MemberView for FakeView {
        fn nick(&self, _g: GuildId, u: UserId) -> Option<Option<String>> {
            self.nicks.get(&u).cloned()
        }
        fn voice(&self, _g: GuildId, u: UserId) -> Option<VoiceFacts> {
            self.voice.get(&u).copied()
        }
    }
    fn facts(channel: u64, mute: bool, deaf: bool) -> VoiceFacts {
        VoiceFacts {
            channel: Some(ch(channel)),
            mute,
            deaf,
        }
    }

    /// A process that started at `t0()`: every row seeded before it was left
    /// by a previous process.
    fn data_with(pool: &PgPool) -> Data {
        data_started(pool, t0())
    }

    fn data_started(pool: &PgPool, at: DateTime<Utc>) -> Data {
        let data = Data(Arc::new(DataInner {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }));
        data.economy.set_process_start(at);
        data
    }

    async fn seed(
        pool: &PgPool,
        kind: ActionKind,
        target: Option<UserId>,
        payload: &ActionPayload,
        state: &str,
        expires_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO economy_actions
                 (guild_id, kind, buyer_id, target_id, price, payload, state, expires_at, created_at)
             VALUES (1, $1, $2, $3, 100, $4, $5, $6, $7) RETURNING id",
        )
        .bind(kind.key())
        .bind(BUYER.get() as i64)
        .bind(target.map(|t| t.get() as i64))
        .bind(Json(payload))
        .bind(state)
        .bind(expires_at)
        .bind(created_at)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn rename_row(pool: &PgPool, state: &str, expires: DateTime<Utc>) -> i64 {
        seed(
            pool,
            ActionKind::Rename,
            Some(U),
            &ActionPayload::Rename {
                name: "Goblin".into(),
                restore: Some("Al".into()),
            },
            state,
            Some(expires),
            t0() - mins(600),
        )
        .await
    }

    async fn state_of(pool: &PgPool, id: i64) -> (String, Option<String>) {
        sqlx::query_as("SELECT state, end_reason FROM economy_actions WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn balance(pool: &PgPool) -> i64 {
        crate::db::economy::balance(pool, G, BUYER).await.unwrap()
    }

    macro_rules! db_test {
        ($name:ident, |$pool:ident| $body:block) => {
            #[sqlx::test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) $body
        };
    }

    db_test!(sweep_restores_the_old_nickname, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let due = rename_row(&pool, "active", t0() - mins(1)).await;
        let later = rename_row(&pool, "active", t0() + mins(5)).await;
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(
            fake.calls(),
            vec![Call::Nick(U, Some("Al".into()), ActionKind::Rename)]
        );
        assert_eq!(
            state_of(&pool, due).await,
            ("ended".into(), Some("expired".into()))
        );
        assert_eq!(state_of(&pool, later).await.0, "active", "not due yet");
    });

    db_test!(sweep_runs_with_the_economy_off, |pool| {
        let data = data_with(&pool);
        assert!(!data.economy.is_enabled(G));
        let fake = FakeEconomyDiscord::default();
        let id = seed(
            &pool,
            ActionKind::Mute,
            Some(U),
            &ActionPayload::Mute,
            "active",
            Some(t0() - mins(1)),
            t0() - mins(30),
        )
        .await;
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(fake.calls(), vec![Call::Mute(U, false, ActionKind::Mute)]);
        assert_eq!(state_of(&pool, id).await.0, "ended");
        // ...and with it on, the same.
        let mut on = EconomySettings::off(G);
        on.enabled = true;
        data.economy.set_settings(on);
        let id2 = rename_row(&pool, "active", t0() - mins(1)).await;
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(state_of(&pool, id2).await.0, "ended");
    });

    db_test!(
        mute_undo_for_an_offline_member_lands_on_their_next_join,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let id = seed(
                &pool,
                ActionKind::Mute,
                Some(U),
                &ActionPayload::Mute,
                "active",
                Some(t0() - mins(1)),
                t0() - mins(30),
            )
            .await;
            fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
            sweep(&data, &fake, t0(), &|_| true).await;
            assert_eq!(state_of(&pool, id).await.0, "ending");
            // The sweep does not retry an ending row.
            sweep(&data, &fake, t0() + mins(1), &|_| true).await;
            assert_eq!(fake.calls().len(), 1);
            // Someone else joining voice does nothing.
            on_voice_join(&data, &fake, G, UserId::new(11), t0() + mins(2)).await;
            assert_eq!(fake.calls().len(), 1);
            on_voice_join(&data, &fake, G, U, t0() + mins(3)).await;
            assert_eq!(
                fake.calls(),
                vec![
                    Call::Mute(U, false, ActionKind::Mute),
                    Call::Mute(U, false, ActionKind::Mute)
                ]
            );
            assert_eq!(
                state_of(&pool, id).await,
                ("ended".into(), Some("expired".into()))
            );
        }
    );

    db_test!(unknown_member_ends_as_left, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = rename_row(&pool, "active", t0() - mins(1)).await;
        fake.fail_next(CallKind::Nick, EconDiscordError::UnknownMember);
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(
            state_of(&pool, id).await,
            ("ended".into(), Some("left".into()))
        );
    });

    db_test!(
        missing_permissions_retries_then_gives_up_after_an_hour,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let id = rename_row(&pool, "active", t0()).await;
            for at in [1, 30, 60] {
                fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
                sweep(&data, &fake, t0() + mins(at), &|_| true).await;
                assert_eq!(state_of(&pool, id).await.0, "active", "at +{at}m");
            }
            fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
            sweep(&data, &fake, t0() + mins(61), &|_| true).await;
            assert_eq!(
                state_of(&pool, id).await,
                ("ended".into(), Some("undo_failed".into()))
            );
            assert_eq!(fake.calls().len(), 4);
        }
    );

    db_test!(
        a_failing_undo_backs_off_and_others_are_not_held_up,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let broken = rename_row(&pool, "active", t0()).await;
            let s = ChronoDuration::seconds;
            let nicks = || {
                fake.calls()
                    .into_iter()
                    .filter(|c| matches!(c, Call::Nick(..)))
                    .count()
            };
            // Fails at t0: the next try waits 15 s, then 30 s.
            fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
            sweep(&data, &fake, t0(), &|_| true).await;
            assert_eq!(nicks(), 1);
            sweep(&data, &fake, t0() + s(10), &|_| true).await;
            assert_eq!(nicks(), 1, "inside its 15 s");
            fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
            sweep(&data, &fake, t0() + s(15), &|_| true).await;
            assert_eq!(nicks(), 2, "tried again at 15 s, and failed again");
            sweep(&data, &fake, t0() + s(40), &|_| true).await;
            assert_eq!(nicks(), 2, "now it waits 30 s");
            // Another row coming due meanwhile is not held up.
            let other = rename_row(&pool, "active", t0() + s(40)).await;
            sweep(&data, &fake, t0() + s(41), &|_| true).await;
            assert_eq!(state_of(&pool, other).await.0, "ended");
            assert_eq!(state_of(&pool, broken).await.0, "active");
            // Its wait over, it is tried and, this time, ends.
            sweep(&data, &fake, t0() + s(45), &|_| true).await;
            assert_eq!(state_of(&pool, broken).await.0, "ended");
        }
    );

    db_test!(rows_in_a_server_the_cache_lacks_are_skipped, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = rename_row(&pool, "active", t0() - mins(1)).await;
        sweep(&data, &fake, t0(), &|g| g != G).await;
        assert_eq!(fake.calls(), vec![], "no Discord call for it");
        assert_eq!(state_of(&pool, id).await.0, "active");
        // Once the server is there it ends as usual.
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(state_of(&pool, id).await.0, "ended");
    });

    db_test!(a_failure_that_clears_ends_the_row_normally, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = rename_row(&pool, "active", t0()).await;
        fake.fail_next(CallKind::Nick, EconDiscordError::MissingPermissions);
        sweep(&data, &fake, t0() + mins(1), &|_| true).await;
        sweep(&data, &fake, t0() + mins(2), &|_| true).await;
        assert_eq!(
            state_of(&pool, id).await,
            ("ended".into(), Some("expired".into()))
        );
    });

    db_test!(finishing_an_already_ended_row_restores_nothing, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        // A mod override ended the row; the caller still holds the old row.
        let id = rename_row(&pool, "ended", t0() - mins(1)).await;
        sqlx::query("UPDATE economy_actions SET end_reason = 'overridden' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        let stale = ActionRow {
            id,
            state: ActionState::Active,
            ..row(
                ActionKind::Rename,
                ActionPayload::Rename {
                    name: "Goblin".into(),
                    restore: Some("Al".into()),
                },
            )
        };
        finish(&data, &fake, G, &stale, "cleansed", t0()).await;
        assert!(fake.calls().is_empty(), "the mod's change is not undone");
        assert_eq!(
            state_of(&pool, id).await,
            ("ended".into(), Some("overridden".into()))
        );
    });

    db_test!(due_actions_never_picks_a_mirror_or_an_ending_row, |pool| {
        seed(
            &pool,
            ActionKind::Mirror,
            Some(U),
            &ActionPayload::Mirror,
            "active",
            None,
            t0() - mins(900),
        )
        .await;
        rename_row(&pool, "ending", t0() - mins(1)).await;
        let due = rename_row(&pool, "active", t0() - mins(1)).await;
        rename_row(&pool, "active", t0() + mins(1)).await;
        let got = crate::db::economy::due_actions(&pool, t0(), 10)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].0, got[0].1.id), (G, due));
    });

    db_test!(end_action_moves_only_a_running_row, |pool| {
        let p = &pool;
        let db = crate::db::economy::end_action;
        let running = rename_row(p, "active", t0()).await;
        let ended = rename_row(p, "ended", t0()).await;
        let pending = rename_row(p, "pending", t0()).await;
        // Only Ended and Ending are targets.
        assert!(!db(p, running, ActionState::Done, "x").await.unwrap());
        assert_eq!(state_of(p, running).await.0, "active");
        assert!(db(p, running, ActionState::Ending, "expired")
            .await
            .unwrap());
        assert_eq!(state_of(p, running).await.0, "ending");
        assert!(db(p, running, ActionState::Ended, "expired").await.unwrap());
        // A final row, or one never activated, never moves.
        assert!(!db(p, ended, ActionState::Ending, "again").await.unwrap());
        assert!(!db(p, pending, ActionState::Ended, "early").await.unwrap());
        assert_eq!(state_of(p, ended).await.0, "ended");
        assert_eq!(state_of(p, pending).await.0, "pending");
        let stamped: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT ended_at FROM economy_actions WHERE id = $1")
                .bind(running)
                .fetch_one(p)
                .await
                .unwrap();
        assert!(stamped.is_some(), "a final state stamps ended_at");
    });

    db_test!(reconcile_finishes_overdue_rows, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let overdue = rename_row(&pool, "active", t0() - mins(30)).await;
        let running = rename_row(&pool, "active", t0() + mins(30)).await;
        reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
        assert_eq!(
            fake.calls(),
            vec![Call::Nick(U, Some("Al".into()), ActionKind::Rename)]
        );
        assert_eq!(state_of(&pool, overdue).await.0, "ended");
        assert_eq!(state_of(&pool, running).await.0, "active");
    });

    db_test!(reconcile_lets_a_name_changed_while_away_stand, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let (changed, kept, uncached) = (U, UserId::new(11), UserId::new(12));
        // The cache shows a change for these two, but Discord says the
        // prank's name is on one (the cache was behind) and can't be
        // read for the other.
        let (behind, unreadable) = (UserId::new(14), UserId::new(15));
        let mut ids = Vec::new();
        for user in [changed, kept, uncached, behind, unreadable] {
            ids.push(
                seed(
                    &pool,
                    ActionKind::Rename,
                    Some(user),
                    &pending_rename(),
                    "active",
                    // Overdue too: the expiry must not restore over it.
                    Some(t0() - mins(1)),
                    t0() - mins(600),
                )
                .await,
            );
        }
        let lock = seed(
            &pool,
            ActionKind::Lock,
            Some(changed),
            &ActionPayload::Lock {
                locked: Some("Al".into()),
            },
            "active",
            Some(t0() + mins(30)),
            t0() - mins(600),
        )
        .await;
        let clown = seed(
            &pool,
            ActionKind::Clown,
            Some(UserId::new(13)),
            &ActionPayload::Clown {
                applied: "Al 🤡".into(),
                restore: Some("Al".into()),
            },
            "active",
            Some(t0() + mins(30)),
            t0() - mins(600),
        )
        .await;
        let mut view = FakeView::default();
        view.nicks.insert(changed, Some("Mine".into()));
        view.nicks.insert(kept, Some("Goblin".into()));
        view.nicks.insert(UserId::new(13), None);
        view.nicks.insert(behind, Some("Mine".into()));
        view.nicks.insert(unreadable, Some("Mine".into()));
        // What a fresh read from Discord says.
        fake.set_member_nick(changed, Some("Mine"));
        fake.set_member_nick(UserId::new(13), None);
        fake.set_member_nick(behind, Some("Goblin"));
        reconcile(&data, &fake, &view, G, t0()).await;
        // Changed while away: overridden, nothing restored, lock too.
        for id in [ids[0], lock] {
            assert_eq!(
                state_of(&pool, id).await,
                ("ended".into(), Some("overridden".into()))
            );
        }
        // A cleared nickname is a change too.
        assert_eq!(
            state_of(&pool, clown).await,
            ("ended".into(), Some("overridden".into()))
        );
        // Still wearing the prank's name, not cached, the cache behind
        // Discord, or Discord unreadable: not judged changed, so the
        // expiry restores as usual.
        for id in [ids[1], ids[2], ids[3], ids[4]] {
            assert_eq!(
                state_of(&pool, id).await,
                ("ended".into(), Some("expired".into()))
            );
        }
        assert_eq!(
            fake.calls(),
            vec![
                Call::Nick(kept, Some("Al".into()), ActionKind::Rename),
                Call::Nick(uncached, Some("Al".into()), ActionKind::Rename),
                Call::Nick(behind, Some("Al".into()), ActionKind::Rename),
                Call::Nick(unreadable, Some("Al".into()), ActionKind::Rename),
            ]
        );
    });

    #[test]
    fn a_name_prank_inside_the_grace_is_not_judged() {
        let now = t0();
        let s = ChronoDuration::seconds;
        // This process started long ago: the grace alone decides.
        let long_ago = Some(now - s(3600));
        assert!(!past_the_grace(now - s(10), long_ago, now), "10 s old");
        assert!(
            !past_the_grace(now - s(120), long_ago, now),
            "exactly the grace"
        );
        assert!(past_the_grace(now - s(121), long_ago, now));
        // No recorded start: the same.
        assert!(!past_the_grace(now - s(10), None, now));
        assert!(past_the_grace(now - s(600), None, now));
        // Bought before this process started: a previous process's, judged
        // whatever its age.
        assert!(past_the_grace(now - s(10), Some(now - s(5)), now));
        assert!(!past_the_grace(now - s(5), Some(now - s(10)), now));
    }

    db_test!(reconcile_leaves_a_fresh_name_prank_to_its_buyer, |pool| {
        let db_now = crate::db::economy::db_now(&pool).await.unwrap();
        let s = ChronoDuration::seconds;
        // Started an hour ago; one rename bought 10 s ago, one 10 min ago.
        let data = data_started(&pool, db_now - s(3600));
        let fake = FakeEconomyDiscord::default();
        let (fresh_user, old_user) = (U, UserId::new(11));
        let mut view = FakeView::default();
        let mut ids = Vec::new();
        for (user, age) in [(fresh_user, 10), (old_user, 600)] {
            ids.push(
                seed(
                    &pool,
                    ActionKind::Rename,
                    Some(user),
                    &pending_rename(),
                    "active",
                    Some(db_now + ChronoDuration::hours(24)),
                    db_now - s(age),
                )
                .await,
            );
            // A mid-run GuildCreate's snapshot, and Discord, both show
            // another name.
            view.nicks.insert(user, Some("Mine".into()));
            fake.set_member_nick(user, Some("Mine"));
        }
        reconcile(&data, &fake, &view, G, Utc::now()).await;
        assert_eq!(
            state_of(&pool, ids[0]).await.0,
            "active",
            "inside the grace"
        );
        assert_eq!(
            state_of(&pool, ids[1]).await,
            ("ended".into(), Some("overridden".into()))
        );
    });

    db_test!(reconcile_finishes_ending_rows_of_members_in_voice, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = seed(
            &pool,
            ActionKind::Deafen,
            Some(U),
            &ActionPayload::Deafen,
            "ending",
            Some(t0() - mins(5)),
            t0() - mins(30),
        )
        .await;
        reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
        assert!(fake.calls().is_empty(), "not in voice: still waiting");
        assert_eq!(state_of(&pool, id).await.0, "ending");
        let mut view = FakeView::default();
        view.voice.insert(U, facts(5, false, true));
        reconcile(&data, &fake, &view, G, t0()).await;
        assert_eq!(fake.calls(), vec![Call::Deaf(U, false, ActionKind::Deafen)]);
        assert_eq!(state_of(&pool, id).await.0, "ended");
    });

    db_test!(reconcile_refunds_a_pending_immunity_row, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = seed(
            &pool,
            ActionKind::Immunity,
            None,
            &ActionPayload::Immunity { hours: 24 },
            "pending",
            None,
            t0() - mins(10),
        )
        .await;
        reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
        assert_eq!(state_of(&pool, id).await.0, "failed");
        assert_eq!(balance(&pool).await, 100);
        assert!(fake.calls().is_empty());
    });

    db_test!(reconcile_leaves_a_fresh_pending_row_alone, |pool| {
        // This process started before the row: a purchase in flight.
        let data = data_started(&pool, t0() - mins(60));
        let fake = FakeEconomyDiscord::default();
        let fresh = seed(
            &pool,
            ActionKind::Immunity,
            None,
            &ActionPayload::Immunity { hours: 24 },
            "pending",
            None,
            t0() - ChronoDuration::seconds(30),
        )
        .await;
        reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
        assert_eq!(state_of(&pool, fresh).await.0, "pending");
        assert_eq!(balance(&pool).await, 0);
    });

    db_test!(
        reconcile_resolves_a_fresh_row_left_by_a_previous_process,
        |pool| {
            // On the database's clock, as production reads it: a row 10 s old.
            let db_now = crate::db::economy::db_now(&pool).await.unwrap();
            let secs = ChronoDuration::seconds;
            let id = seed(
                &pool,
                ActionKind::Immunity,
                None,
                &ActionPayload::Immunity { hours: 24 },
                "pending",
                None,
                db_now - secs(10),
            )
            .await;
            let fake = FakeEconomyDiscord::default();
            // This process started before the row: it may be in flight.
            let running = data_started(&pool, db_now - secs(20));
            reconcile(&running, &fake, &FakeView::default(), G, Utc::now()).await;
            assert_eq!(state_of(&pool, id).await.0, "pending", "left alone");
            assert_eq!(balance(&pool).await, 0);
            // A process that started after the row: a crash left it. Resolved
            // at the first GuildCreate, though it is far inside the grace.
            let restarted = data_started(&pool, db_now - secs(5));
            reconcile(&restarted, &fake, &FakeView::default(), G, Utc::now()).await;
            assert_eq!(state_of(&pool, id).await.0, "failed", "refunded");
            assert_eq!(balance(&pool).await, 100);
            // Without a recorded start only the grace counts.
            let fresh = seed(
                &pool,
                ActionKind::Immunity,
                None,
                &ActionPayload::Immunity { hours: 24 },
                "pending",
                None,
                db_now - secs(10),
            )
            .await;
            let unknown = Data(Arc::new(DataInner {
                database_pool: Some(pool.clone()),
                ..Default::default()
            }));
            reconcile(&unknown, &fake, &FakeView::default(), G, Utc::now()).await;
            assert_eq!(state_of(&pool, fresh).await.0, "pending");
        }
    );

    fn pending_rename() -> ActionPayload {
        ActionPayload::Rename {
            name: "Goblin".into(),
            restore: Some("Al".into()),
        }
    }

    db_test!(
        reconcile_activates_a_landed_pending_rename_and_refunds_one_that_did_not_land,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let other = UserId::new(11);
            let landed = seed(
                &pool,
                ActionKind::Rename,
                Some(U),
                &pending_rename(),
                "pending",
                Some(t0() + mins(30)),
                t0() - mins(10),
            )
            .await;
            let missed = seed(
                &pool,
                ActionKind::Rename,
                Some(other),
                &pending_rename(),
                "pending",
                Some(t0() + mins(30)),
                t0() - mins(10),
            )
            .await;
            let mut view = FakeView::default();
            view.nicks.insert(U, Some("Goblin".into()));
            view.nicks.insert(other, Some("Al".into()));
            reconcile(&data, &fake, &view, G, t0()).await;
            assert_eq!(state_of(&pool, landed).await.0, "active");
            assert_eq!(state_of(&pool, missed).await.0, "failed");
            assert_eq!(balance(&pool).await, 100, "one refund");
            assert!(fake.calls().is_empty(), "a known miss undoes nothing");
            // The landed row still ends on time.
            sweep(&data, &fake, t0() + mins(31), &|_| true).await;
            assert_eq!(state_of(&pool, landed).await.0, "ended");
        }
    );

    db_test!(
        reconcile_judges_every_kind_by_what_the_cache_shows,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let exp = Some(t0() + mins(30));
            let old = t0() - mins(10);
            let mut ids = HashMap::new();
            let mut view = FakeView::default();
            let cases: [(
                &str,
                ActionKind,
                ActionPayload,
                u64,
                Option<VoiceFacts>,
                bool,
            ); 12] = [
                (
                    "mute_yes",
                    ActionKind::Mute,
                    ActionPayload::Mute,
                    31,
                    Some(facts(5, true, false)),
                    true,
                ),
                (
                    "mute_no",
                    ActionKind::Mute,
                    ActionPayload::Mute,
                    32,
                    Some(facts(5, false, false)),
                    false,
                ),
                (
                    "deaf_yes",
                    ActionKind::Deafen,
                    ActionPayload::Deafen,
                    33,
                    Some(facts(5, false, true)),
                    true,
                ),
                (
                    "deaf_no",
                    ActionKind::Deafen,
                    ActionPayload::Deafen,
                    34,
                    Some(facts(5, true, false)),
                    false,
                ),
                (
                    "jail_yes",
                    ActionKind::Jail,
                    ActionPayload::Jail { jail: ch(9) },
                    35,
                    Some(facts(9, false, false)),
                    true,
                ),
                (
                    "jail_no",
                    ActionKind::Jail,
                    ActionPayload::Jail { jail: ch(9) },
                    36,
                    Some(facts(5, false, false)),
                    false,
                ),
                (
                    "drag_yes",
                    ActionKind::Drag,
                    ActionPayload::Drag {
                        from: ch(1),
                        to: ch(2),
                    },
                    37,
                    Some(facts(2, false, false)),
                    true,
                ),
                (
                    "drag_no",
                    ActionKind::Drag,
                    ActionPayload::Drag {
                        from: ch(1),
                        to: ch(2),
                    },
                    38,
                    Some(facts(1, false, false)),
                    false,
                ),
                (
                    "lock",
                    ActionKind::Lock,
                    ActionPayload::Lock { locked: None },
                    39,
                    None,
                    true,
                ),
                (
                    "mirror",
                    ActionKind::Mirror,
                    ActionPayload::Mirror,
                    40,
                    None,
                    true,
                ),
                (
                    "clown_yes",
                    ActionKind::Clown,
                    ActionPayload::Clown {
                        applied: "Al 🤡".into(),
                        restore: None,
                    },
                    41,
                    None,
                    true,
                ),
                (
                    "clown_no",
                    ActionKind::Clown,
                    ActionPayload::Clown {
                        applied: "Al 🤡".into(),
                        restore: None,
                    },
                    42,
                    None,
                    false,
                ),
            ];
            for (name, kind, payload, user, voice, expect) in cases {
                let user = UserId::new(user);
                let expires = if kind == ActionKind::Mirror {
                    None
                } else {
                    exp
                };
                let id = seed(&pool, kind, Some(user), &payload, "pending", expires, old).await;
                if let Some(v) = voice {
                    view.voice.insert(user, v);
                }
                if kind == ActionKind::Clown {
                    view.nicks.insert(
                        user,
                        expect.then(|| "Al 🤡".to_string()).or(Some("Al".into())),
                    );
                }
                ids.insert(name, (id, expect, kind));
            }
            reconcile(&data, &fake, &view, G, t0()).await;
            for (name, (id, expect, kind)) in ids {
                let (state, _) = state_of(&pool, id).await;
                let want = match (expect, kind) {
                    (false, _) => "failed",
                    (true, ActionKind::Drag) => "done",
                    (true, _) => "active",
                };
                assert_eq!(state, want, "{name}");
            }
            assert!(fake.calls().is_empty());
            assert_eq!(balance(&pool).await, 500, "five refunds");
        }
    );

    db_test!(
        reconcile_refunds_and_undoes_a_pending_row_it_cannot_judge,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            // Member not cached at all.
            let id = seed(
                &pool,
                ActionKind::Rename,
                Some(U),
                &pending_rename(),
                "pending",
                Some(t0() + mins(30)),
                t0() - mins(10),
            )
            .await;
            reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
            assert_eq!(state_of(&pool, id).await.0, "failed");
            assert_eq!(balance(&pool).await, 100);
            assert_eq!(
                fake.calls(),
                vec![Call::Nick(U, Some("Al".into()), ActionKind::Rename)]
            );
        }
    );

    db_test!(an_unjudgeable_pending_mute_stays_owned, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let mute = seed(
            &pool,
            ActionKind::Mute,
            Some(U),
            &ActionPayload::Mute,
            "pending",
            Some(t0() + mins(5)),
            t0() - mins(10),
        )
        .await;
        let deaf = seed(
            &pool,
            ActionKind::Deafen,
            Some(UserId::new(11)),
            &ActionPayload::Deafen,
            "pending",
            Some(t0() + mins(5)),
            t0() - mins(10),
        )
        .await;
        // Neither member is in voice (so the cache cannot say).
        reconcile(&data, &fake, &FakeView::default(), G, t0()).await;
        assert_eq!(state_of(&pool, mute).await.0, "active");
        assert_eq!(state_of(&pool, deaf).await.0, "active");
        assert_eq!(balance(&pool).await, 0, "nothing refunded");
        assert!(fake.calls().is_empty());
        // The expiry then undoes it, or waits for the member's next join.
        fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
        sweep(&data, &fake, t0() + mins(6), &|_| true).await;
        assert_eq!(state_of(&pool, mute).await.0, "ending");
        assert_eq!(state_of(&pool, deaf).await.0, "ended");
    });

    db_test!(a_refund_that_did_nothing_undoes_nothing, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        // Another actor already activated the row; our copy is stale.
        let id = seed(
            &pool,
            ActionKind::Rename,
            Some(U),
            &pending_rename(),
            "active",
            Some(t0() + mins(30)),
            t0() - mins(10),
        )
        .await;
        let stale = ActionRow {
            id,
            state: ActionState::Pending,
            ..row(ActionKind::Rename, pending_rename())
        };
        resolve_pending(&data, &fake, &FakeView::default(), G, &stale).await;
        assert!(
            fake.calls().is_empty(),
            "no undo after a refund that did nothing"
        );
        assert_eq!(state_of(&pool, id).await.0, "active");
        assert_eq!(balance(&pool).await, 0);
    });

    db_test!(a_pending_cleanse_or_reveal_is_refunded, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let cleanse = seed(
            &pool,
            ActionKind::Cleanse,
            Some(U),
            &ActionPayload::Cleanse,
            "pending",
            None,
            t0() - mins(10),
        )
        .await;
        let reveal = seed(
            &pool,
            ActionKind::Reveal,
            Some(U),
            &ActionPayload::Reveal { action_id: 7 },
            "pending",
            None,
            t0() - mins(10),
        )
        .await;
        let mut view = FakeView::default();
        view.voice.insert(U, facts(5, true, true));
        reconcile(&data, &fake, &view, G, t0()).await;
        assert_eq!(state_of(&pool, cleanse).await.0, "failed");
        assert_eq!(state_of(&pool, reveal).await.0, "failed");
        assert_eq!(balance(&pool).await, 200);
        assert!(fake.calls().is_empty());
    });

    db_test!(an_ending_row_whose_undo_fails_stays_ending, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        // Due two hours ago, set aside as ending.
        let id = seed(
            &pool,
            ActionKind::Mute,
            Some(U),
            &ActionPayload::Mute,
            "ending",
            Some(t0() - mins(120)),
            t0() - mins(200),
        )
        .await;
        for err in [
            EconDiscordError::MissingPermissions,
            EconDiscordError::Other("boom".into()),
        ] {
            fake.fail_next(CallKind::Mute, err);
            on_voice_join(&data, &fake, G, U, t0()).await;
            assert_eq!(state_of(&pool, id).await.0, "ending");
        }
        on_voice_join(&data, &fake, G, U, t0()).await;
        assert_eq!(state_of(&pool, id).await.0, "ended");
        assert_eq!(fake.calls().len(), 3);
    });

    db_test!(
        an_offline_cleanse_keeps_its_reason_after_the_rejoin,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let other = UserId::new(11);
            let mut ids = Vec::new();
            for user in [U, other] {
                let id = seed(
                    &pool,
                    ActionKind::Mute,
                    Some(user),
                    &ActionPayload::Mute,
                    "active",
                    Some(t0() + mins(30)),
                    t0() - mins(10),
                )
                .await;
                fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
                let r = ActionRow {
                    id,
                    target: Some(user),
                    ..row(ActionKind::Mute, ActionPayload::Mute)
                };
                finish(&data, &fake, G, &r, "cleansed", t0()).await;
                assert_eq!(
                    state_of(&pool, id).await,
                    ("ending".into(), Some("cleansed".into()))
                );
                ids.push(id);
            }
            // A failed retry while still ending keeps the reason too.
            fake.fail_next(CallKind::Mute, EconDiscordError::NotConnected);
            on_voice_join(&data, &fake, G, U, t0() + mins(1)).await;
            assert_eq!(state_of(&pool, ids[0]).await.1.as_deref(), Some("cleansed"));
            on_voice_join(&data, &fake, G, U, t0() + mins(2)).await;
            assert_eq!(
                state_of(&pool, ids[0]).await,
                ("ended".into(), Some("cleansed".into()))
            );
            // A member who left the server is "left", whatever it was set aside for.
            fake.fail_next(CallKind::Mute, EconDiscordError::UnknownMember);
            on_voice_join(&data, &fake, G, other, t0() + mins(3)).await;
            assert_eq!(
                state_of(&pool, ids[1]).await,
                ("ended".into(), Some("left".into()))
            );
        }
    );

    db_test!(an_unknown_kind_does_not_stop_the_known_rows, |pool| {
        // Rows from a newer build (an image-only rollback leaves them).
        for state in ["active", "ending", "pending"] {
            sqlx::query(
                "INSERT INTO economy_actions
                     (guild_id, kind, buyer_id, target_id, price, payload, state, expires_at, created_at)
                 VALUES (1, 'teleport', 20, 10, 100, '{\"kind\":\"teleport\",\"x\":1}'::jsonb, $1, $2, $3)",
            )
            .bind(state)
            .bind(t0() - mins(1))
            .bind(t0() - mins(30))
            .execute(&pool)
            .await
            .unwrap();
        }
        let due = rename_row(&pool, "active", t0() - mins(1)).await;
        let ending = rename_row(&pool, "ending", t0() - mins(1)).await;
        let pending = rename_row(&pool, "pending", t0() - mins(1)).await;
        let ids = |rows: Vec<ActionRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
        use crate::db::economy as db;
        let got = db::due_actions(&pool, t0(), 10).await.unwrap();
        assert_eq!(
            got.into_iter().map(|(_, r)| r.id).collect::<Vec<_>>(),
            vec![due]
        );
        assert_eq!(
            ids(db::overdue_for_guild(&pool, G, t0()).await.unwrap()),
            vec![due]
        );
        assert_eq!(
            ids(db::ending_for(&pool, G, U).await.unwrap()),
            vec![ending]
        );
        assert_eq!(
            ids(db::ending_for_guild(&pool, G).await.unwrap()),
            vec![ending]
        );
        assert_eq!(
            ids(db::pending_for_guild(&pool, G, Some(t0()), 120.0)
                .await
                .unwrap()),
            vec![pending]
        );
        // /economy status's record: newest first, at most the limit, and
        // only kinds this build knows.
        let p = &pool;
        let recent = |limit| async move {
            db::recent_actions(p, G, limit)
                .await
                .unwrap()
                .into_iter()
                .map(|a| a.row.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(recent(10).await, vec![pending, ending, due]);
        assert_eq!(recent(2).await, vec![pending, ending]);
    });

    db_test!(an_override_beats_a_stored_reason, |pool| {
        let mut ids = Vec::new();
        for _ in 0..2 {
            ids.push(
                seed(
                    &pool,
                    ActionKind::Mute,
                    Some(U),
                    &ActionPayload::Mute,
                    "active",
                    Some(t0() + mins(30)),
                    t0() - mins(10),
                )
                .await,
            );
        }
        sqlx::query("UPDATE economy_actions SET end_reason = 'cleansed'")
            .execute(&pool)
            .await
            .unwrap();
        // A specific reason replaces the stored one; the generic one keeps it.
        assert!(
            economy::end_action(&pool, ids[0], ActionState::Ended, "overridden")
                .await
                .unwrap()
        );
        assert!(
            economy::end_action(&pool, ids[1], ActionState::Ended, "expired")
                .await
                .unwrap()
        );
        assert_eq!(
            state_of(&pool, ids[0]).await,
            ("ended".into(), Some("overridden".into()))
        );
        assert_eq!(
            state_of(&pool, ids[1]).await,
            ("ended".into(), Some("cleansed".into()))
        );
    });

    // ---- Part 3: themes, bounties, funds and the unplayed ----

    const FRIEND: UserId = UserId::new(21);

    async fn fund_wallet(pool: &PgPool, user: UserId, amount: i64) {
        crate::db::economy::credit_earnings(
            pool,
            G,
            &[crate::db::economy::EarnRow {
                user,
                voice: amount,
                messages: 0,
            }],
        )
        .await
        .unwrap();
    }

    fn theme_payload() -> ActionPayload {
        ActionPayload::Theme {
            url: "https://example.com/clip".into(),
            start_secs: 0,
        }
    }

    /// Charge an escrow row to `BUYER` the way the shop does.
    async fn escrow(
        pool: &PgPool,
        kind: ActionKind,
        price: i64,
        payload: &ActionPayload,
        expires: DateTime<Utc>,
    ) -> i64 {
        let ledger = if kind == ActionKind::Bounty {
            crate::economy::wallet::LedgerKind::BountyPlaced
        } else {
            crate::economy::wallet::LedgerKind::Purchase
        };
        crate::db::economy::charge_active(
            pool,
            &crate::db::economy::NewAction {
                guild: G,
                kind,
                buyer: BUYER,
                target: Some(U),
                price,
                note: None,
                signed: false,
                payload,
                expires_at: Some(expires),
            },
            ledger,
        )
        .await
        .unwrap()
        .action_id
    }

    /// Every wallet equals what it was given plus its ledger rows.
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
                crate::db::economy::balance(pool, G, *user).await.unwrap(),
                start + delta,
                "wallet of {user} disagrees with its ledger"
            );
        }
    }

    #[test]
    fn dedications_and_jumps_never_count_as_landed() {
        let view = FakeView::default();
        let song = || "a song".to_string();
        for payload in [
            ActionPayload::Dedicate { song: song() },
            ActionPayload::Jump { song: song() },
        ] {
            let kind = match payload {
                ActionPayload::Dedicate { .. } => ActionKind::Dedicate,
                _ => ActionKind::Jump,
            };
            assert_eq!(landed(&view, G, &row(kind, payload)), Some(false));
        }
    }

    db_test!(an_expired_theme_refunds_its_buyer, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        let due = escrow(
            &pool,
            ActionKind::Theme,
            60,
            &theme_payload(),
            t0() - mins(1),
        )
        .await;
        let later = escrow(
            &pool,
            ActionKind::Theme,
            10,
            &theme_payload(),
            t0() + mins(5),
        )
        .await;
        assert_eq!(balance(&pool).await, 30);
        sweep(&data, &fake, t0(), &|_| true).await;
        assert!(fake.calls().is_empty(), "{:?}", fake.calls());
        assert_eq!(balance(&pool).await, 90);
        assert_eq!(
            state_of(&pool, due).await,
            ("ended".into(), Some("expired".into()))
        );
        assert_eq!(state_of(&pool, later).await.0, "active", "not due yet");
        // A second sweep refunds nothing twice.
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(balance(&pool).await, 90);
        agree(&pool, &[(BUYER, 100)]).await;
    });

    db_test!(an_expired_bounty_refunds_its_placer, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        let id = escrow(
            &pool,
            ActionKind::Bounty,
            70,
            &ActionPayload::Bounty,
            t0() - mins(1),
        )
        .await;
        assert_eq!(balance(&pool).await, 30);
        // The cache not holding the guild changes nothing: no Discord call.
        sweep(&data, &fake, t0(), &|_| false).await;
        assert!(fake.calls().is_empty());
        assert_eq!(balance(&pool).await, 100);
        assert_eq!(
            state_of(&pool, id).await,
            ("ended".into(), Some("expired".into()))
        );
        agree(&pool, &[(BUYER, 100)]).await;
    });

    db_test!(finish_skips_an_escrow_row_that_already_ended, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        let id = escrow(
            &pool,
            ActionKind::Theme,
            60,
            &theme_payload(),
            t0() + mins(5),
        )
        .await;
        let live = economy::due_actions(&pool, t0() + mins(10), 10)
            .await
            .unwrap();
        let row = live.into_iter().find(|(_, r)| r.id == id).unwrap().1;
        assert_eq!(
            finish(&data, &fake, G, &row, "cleansed", t0()).await,
            Finished::Ended
        );
        // The caller's copy is stale; the database says it is over.
        assert_eq!(
            finish(&data, &fake, G, &row, "expired", t0()).await,
            Finished::Skipped
        );
        assert_eq!(balance(&pool).await, 100);
        assert_eq!(
            state_of(&pool, id).await,
            ("ended".into(), Some("cleansed".into()))
        );
        assert!(fake.calls().is_empty());
        agree(&pool, &[(BUYER, 100)]).await;
    });

    db_test!(cleanse_ends_a_waiting_theme_and_refunds_it, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        let theme = escrow(
            &pool,
            ActionKind::Theme,
            60,
            &theme_payload(),
            t0() + mins(60),
        )
        .await;
        // A bounty on the same member is not touched.
        let bounty = escrow(
            &pool,
            ActionKind::Bounty,
            20,
            &ActionPayload::Bounty,
            t0() + mins(60),
        )
        .await;
        let rename = rename_row(&pool, "active", t0() + mins(60)).await;
        let ended = end_running_pranks(&data, &fake, G, U, "cleansed", t0())
            .await
            .unwrap();
        assert_eq!(
            ended,
            EndedPranks {
                all: 2,
                were_active: 2
            }
        );
        // Only the rename touched Discord.
        assert_eq!(
            fake.calls(),
            vec![Call::Nick(U, Some("Al".into()), ActionKind::Rename)]
        );
        assert_eq!(
            state_of(&pool, theme).await,
            ("ended".into(), Some("cleansed".into()))
        );
        assert_eq!(state_of(&pool, bounty).await.0, "active");
        assert_eq!(state_of(&pool, rename).await.0, "ended");
        assert_eq!(
            balance(&pool).await,
            80,
            "the theme's 60 is back, the bounty's 20 is not"
        );
        agree(&pool, &[(BUYER, 100)]).await;
    });

    db_test!(an_unfilled_fund_is_refunded_after_24h, |pool| {
        use crate::db::economy as db;
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        fund_wallet(&pool, FRIEND, 100).await;
        let payload = pending_rename();
        let new = |price: i64, kind: ActionKind| db::NewAction {
            guild: G,
            kind,
            buyer: BUYER,
            target: Some(U),
            price,
            note: None,
            signed: false,
            payload: &payload,
            expires_at: Some(Utc::now() + ChronoDuration::hours(24)),
        };
        let lapsed = db::open_fund(&pool, &new(90, ActionKind::Rename), Some((40, false)))
            .await
            .unwrap()
            .action_id;
        db::pledge(&pool, lapsed, G, FRIEND, 30, true)
            .await
            .unwrap();
        // Another fund, on another prank, still has time.
        let open = db::open_fund(&pool, &new(90, ActionKind::Clown), Some((10, false)))
            .await
            .unwrap()
            .action_id;
        sqlx::query(
            "UPDATE economy_actions SET expires_at = now() - interval '1 minute' WHERE id = $1",
        )
        .bind(lapsed)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(balance(&pool).await, 50);
        sweep(&data, &fake, Utc::now(), &|_| false).await;
        assert!(fake.calls().is_empty(), "{:?}", fake.calls());
        assert_eq!(
            state_of(&pool, lapsed).await,
            ("ended".into(), Some("unfunded".into()))
        );
        assert_eq!(state_of(&pool, open).await.0, "funding");
        assert_eq!(balance(&pool).await, 90, "the 40 is back, the 10 is not");
        assert_eq!(
            crate::db::economy::balance(&pool, G, FRIEND).await.unwrap(),
            100
        );
        agree(&pool, &[(BUYER, 100), (FRIEND, 100)]).await;
        // A second sweep refunds nothing twice.
        sweep(&data, &fake, Utc::now(), &|_| false).await;
        assert_eq!(balance(&pool).await, 90);
        // A sweep whose clock runs ahead of the database's still leaves a fund
        // that has time (or was filled a moment ago): `expire_fund` judges.
        sweep(&data, &fake, t0(), &|_| false).await;
        assert_eq!(state_of(&pool, open).await.0, "funding");
        assert_eq!(balance(&pool).await, 90);
    });

    db_test!(reconcile_refunds_a_pending_dedication, |pool| {
        use crate::db::economy as db;
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        fund_wallet(&pool, BUYER, 100).await;
        let mut ids = Vec::new();
        for (kind, payload) in [
            (
                ActionKind::Dedicate,
                ActionPayload::Dedicate { song: "x".into() },
            ),
            (ActionKind::Jump, ActionPayload::Jump { song: "y".into() }),
        ] {
            ids.push(
                db::charge_for_action(
                    &pool,
                    &db::NewAction {
                        guild: G,
                        kind,
                        buyer: BUYER,
                        target: None,
                        price: 40,
                        note: None,
                        signed: false,
                        payload: &payload,
                        expires_at: None,
                    },
                )
                .await
                .unwrap()
                .action_id,
            );
        }
        assert_eq!(balance(&pool).await, 20);
        // `data_with` started at t0, after both charges.
        reconcile(&data, &fake, &FakeView::default(), G, Utc::now()).await;
        assert!(fake.calls().is_empty());
        assert_eq!(balance(&pool).await, 100);
        for id in ids {
            assert_eq!(state_of(&pool, id).await.0, "failed");
        }
        agree(&pool, &[(BUYER, 100)]).await;
    });

    /// A mute fund, filled by two members and left `pending`: what a fill
    /// leaves behind (no end yet), for a crash to find.
    async fn filled_mute_fund(pool: &PgPool) -> i64 {
        use crate::db::economy as db;
        let other = UserId::new(11);
        fund_wallet(pool, BUYER, 100).await;
        fund_wallet(pool, other, 100).await;
        let f = db::open_fund(
            pool,
            &db::NewAction {
                guild: G,
                kind: ActionKind::Mute,
                buyer: BUYER,
                target: Some(U),
                price: 90,
                note: None,
                signed: true,
                payload: &ActionPayload::Mute,
                expires_at: Some(Utc::now() + ChronoDuration::hours(24)),
            },
            Some((45, true)),
        )
        .await
        .unwrap();
        let p = db::pledge(pool, f.action_id, G, other, 45, false)
            .await
            .unwrap();
        assert!(p.filled);
        f.action_id
    }

    db_test!(
        reconcile_refunds_an_unprepared_filled_fund_without_acting,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let id = filled_mute_fund(&pool).await;
            // Even with the target showing as muted: nothing was prepared, so
            // nothing of ours is on them to own or undo.
            let mut view = FakeView::default();
            view.voice.insert(U, facts(5, true, false));
            reconcile(&data, &fake, &view, G, Utc::now()).await;
            assert_eq!(state_of(&pool, id).await.0, "failed");
            assert!(fake.calls().is_empty());
            assert_eq!(balance(&pool).await, 100, "the starter's pledge came back");
            assert_eq!(
                crate::db::economy::balance(&pool, G, UserId::new(11))
                    .await
                    .unwrap(),
                100
            );
            let refunds: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM economy_ledger WHERE kind = 'pledge_refund'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(refunds, 2);
        }
    );

    db_test!(reconcile_activates_a_prepared_landed_mute_fund, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = filled_mute_fund(&pool).await;
        // What the executor writes before it acts.
        let end = Utc::now() + mins(5);
        assert!(
            crate::db::economy::prepare_fund(&pool, id, &ActionPayload::Mute, Some(end))
                .await
                .unwrap()
        );
        let mut view = FakeView::default();
        view.voice.insert(U, facts(5, true, false));
        reconcile(&data, &fake, &view, G, Utc::now()).await;
        assert_eq!(state_of(&pool, id).await.0, "active");
        assert!(fake.calls().is_empty());
        let row = crate::db::economy::action_by_id(&pool, G, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.expires_at.map(|t| t.timestamp_micros()),
            Some(end.timestamp_micros())
        );
        // Nobody was refunded.
        assert_eq!(balance(&pool).await, 55);
    });
}
