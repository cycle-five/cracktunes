//! What the audit log and voice say about running pranks: the nickname lock,
//! a moderator's change winning, and jail pulling a wanderer back.
//!
//! Everything here acts on rows that are already running, so none of it
//! checks whether the server still has the economy on (spec 14).

use super::discord::{EconomyDiscord, LiveEconomyDiscord};
use super::effects::MemberView;
use super::settings::ActionKind;
use super::wallet::{ActionPayload, ActionState};
use crate::db::economy::{self, ActionRow};
use crate::economy::EconomyData;
use chrono::{DateTime, Utc};
use serenity::all::{AuditLogEntry, Cache, ChannelId, GuildId, Http, UserId};
use serenity::model::guild::audit_log::{Action, Change, MemberAction};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// How often a jailed member is moved back, at most.
pub const JAIL_MOVE_EVERY: Duration = Duration::from_secs(10);

/// What a nickname change by `executor` means for a running name prank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NickRule {
    /// The target changed their own name: put it back (while a lock runs).
    Revert,
    /// The bot's own change, such as the shop's: nothing to do.
    Ignore,
    /// Someone else, such as a moderator: they win.
    Overridden,
}

/// Who changed the nickname decides what happens (spec 6.4). The bot wins
/// over the target, so CrackTunes renaming itself is also `Ignore`.
pub fn nick_change_rule(executor: UserId, target: UserId, bot: UserId) -> NickRule {
    if executor == bot {
        NickRule::Ignore
    } else if executor == target {
        NickRule::Revert
    } else {
        NickRule::Overridden
    }
}

/// One change a `MEMBER_UPDATE` entry records.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberChange {
    /// The new nickname; `None` is a cleared one.
    Nick(Option<String>),
    Mute,
    Deaf,
}

fn user_of(id: u64) -> Option<UserId> {
    (id != 0 && id != u64::MAX).then(|| UserId::new(id))
}

/// `(executor, target, changes)` of a member update; None for another action
/// or an entry without both ids.
fn member_changes(entry: &AuditLogEntry) -> Option<(UserId, UserId, Vec<MemberChange>)> {
    if !matches!(entry.action, Action::Member(MemberAction::Update)) {
        return None;
    }
    let executor = user_of(entry.user_id?.get())?;
    let target = user_of(entry.target_id?.get())?;
    let changes = entry
        .changes
        .iter()
        .filter_map(|c| match c {
            Change::Nick { new, .. } => {
                Some(MemberChange::Nick(new.as_ref().map(|n| n.to_string())))
            },
            Change::Mute { .. } => Some(MemberChange::Mute),
            Change::Deaf { .. } => Some(MemberChange::Deaf),
            _ => None,
        })
        .collect();
    Some((executor, target, changes))
}

const NAME_KINDS: [ActionKind; 3] = [ActionKind::Rename, ActionKind::Clown, ActionKind::Lock];

async fn running(
    data: &EconomyData,
    guild: GuildId,
    target: UserId,
    kinds: &[ActionKind],
    include_ending: bool,
) -> Vec<ActionRow> {
    let Some(pool) = data.database_pool.as_ref() else {
        return Vec::new();
    };
    match economy::active_of_kind(pool, guild, target, kinds).await {
        Ok(rows) => rows
            .into_iter()
            .filter(|r| include_ending || r.state == ActionState::Active)
            .collect(),
        Err(err) => {
            tracing::warn!("economy: reading running actions of {target} failed: {err}");
            Vec::new()
        },
    }
}

/// Ends rows with a reason and restores nothing.
async fn end_without_restore(data: &EconomyData, rows: &[ActionRow], reason: &str) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    for row in rows {
        if let Err(err) = economy::end_action(pool, row.id, ActionState::Ended, reason).await {
            tracing::warn!(
                "economy: ending action {} as {reason} failed: {err}",
                row.id
            );
        }
    }
}

/// An audit entry arrived (whether or not the mod log is on).
///
/// - a nickname change on a member with a running name prank:
///   - by the member, with a lock running: reverted to the locked name;
///   - by the member, with no lock: the prank ends `escaped` (restoring over
///     their own new name would undo a change the bot didn't make);
///   - by CrackTunes: ignored;
///   - by anyone else: the lock and the prank end `overridden`;
/// - a server mute or deafen by anyone but CrackTunes ends the running
///   Mute or Deafen row `overridden`.
///
/// Nothing is restored for an override.
pub async fn on_audit_entry(
    data: &EconomyData,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    entry: &AuditLogEntry,
    bot: UserId,
) {
    let Some((executor, target, changes)) = member_changes(entry) else {
        return;
    };
    for change in changes {
        match change {
            MemberChange::Nick(new) => {
                on_nick_change(data, discord, guild, executor, target, bot, new).await;
            },
            MemberChange::Mute | MemberChange::Deaf => {
                if executor == bot {
                    continue;
                }
                let kind = if change == MemberChange::Mute {
                    ActionKind::Mute
                } else {
                    ActionKind::Deafen
                };
                let rows = running(data, guild, target, &[kind], true).await;
                end_without_restore(data, &rows, "overridden").await;
            },
        }
    }
}

async fn on_nick_change(
    data: &EconomyData,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    executor: UserId,
    target: UserId,
    bot: UserId,
    new: Option<String>,
) {
    let rule = nick_change_rule(executor, target, bot);
    if rule == NickRule::Ignore {
        return;
    }
    let rows = running(data, guild, target, &NAME_KINDS, false).await;
    if rows.is_empty() {
        return;
    }
    match rule {
        NickRule::Ignore => {},
        NickRule::Overridden => end_without_restore(data, &rows, "overridden").await,
        NickRule::Revert => {
            let locked = rows.iter().find_map(|r| match &r.payload {
                ActionPayload::Lock { locked } => Some(locked.clone()),
                _ => None,
            });
            match locked {
                Some(locked) => {
                    // A lock bought before the rename or clown holds the
                    // prank's name, not the one it captured first.
                    let locked = prank_name(&rows).map(Some).unwrap_or(locked);
                    if locked != new {
                        if let Err(err) = discord
                            .set_nick(guild, target, locked.as_deref(), ActionKind::Lock)
                            .await
                        {
                            tracing::warn!(
                                "economy: reverting {target}'s nickname in {guild} failed: {err:?}"
                            );
                        }
                    }
                },
                None => end_without_restore(data, &rows, "escaped").await,
            }
        },
    }
}

/// The name a running (`active`) rename or clown among `rows` put on the
/// member, if one runs.
fn prank_name(rows: &[ActionRow]) -> Option<String> {
    rows.iter()
        .filter(|r| r.state == ActionState::Active)
        .find_map(|r| match &r.payload {
            ActionPayload::Rename { name, .. } => Some(name.clone()),
            ActionPayload::Clown { applied, .. } => Some(applied.clone()),
            _ => None,
        })
}

/// What to do about a jailed member who is out of their jail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailMove {
    /// Move them back now.
    MoveNow,
    /// Inside the rate limit: look again at this time.
    ScheduleAt(Instant),
    /// Inside the rate limit, and a re-check is already scheduled.
    Nothing,
}

/// The jail's rate limit: a move back at most once per `cooldown`; a move
/// out inside it schedules one re-check at its end (unless one is already
/// scheduled), so moving out twice quickly doesn't escape the jail.
pub fn jail_decision(
    now: Instant,
    last_move: Option<Instant>,
    scheduled: bool,
    cooldown: Duration,
) -> JailMove {
    match last_move {
        Some(last) if now.saturating_duration_since(last) < cooldown => {
            if scheduled {
                JailMove::Nothing
            } else {
                JailMove::ScheduleAt(last + cooldown)
            }
        },
        _ => JailMove::MoveNow,
    }
}

/// A member's voice channel changed (`to` is None for a disconnect).
///
/// A member with a running Jail row (`active`, and its time not up at `at`:
/// the sweeper may not have ended it yet) who lands in a channel other than
/// the row's jail channel is moved back, at most once per [`JAIL_MOVE_EVERY`];
/// inside that limit, the answer is when to look again (the caller schedules
/// it with [`spawn_jail_recheck`]). A disconnect is allowed. If the move fails
/// the row is left as it is.
pub async fn on_voice_move(
    data: &EconomyData,
    discord: &dyn EconomyDiscord,
    guild: GuildId,
    user: UserId,
    to: Option<ChannelId>,
    now: Instant,
    at: DateTime<Utc>,
) -> Option<Instant> {
    let to = to?;
    let rows = running(data, guild, user, &[ActionKind::Jail], false).await;
    let jail = rows
        .iter()
        .filter(|r| r.expires_at.is_none_or(|end| end > at))
        .find_map(|r| match r.payload {
            ActionPayload::Jail { jail } => Some(jail),
            _ => None,
        })?;
    if to == jail {
        return None;
    }
    match data.economy.jail_step(guild, user, now, JAIL_MOVE_EVERY) {
        JailMove::MoveNow => {
            if let Err(err) = discord
                .move_member(guild, user, jail, ActionKind::Jail)
                .await
            {
                tracing::warn!(
                    "economy: moving jailed {user} back to {jail} in {guild} failed: {err:?}"
                );
            }
            None
        },
        JailMove::ScheduleAt(at) => Some(at),
        JailMove::Nothing => None,
    }
}

/// The scheduled re-check: clears the member's mark, then reads where the
/// cache has them now and handles it like a move ([`on_voice_move`]: still
/// jailed, out of the jail, through the rate limit). Returns when to look
/// again, if the limit says so.
pub async fn jail_recheck(
    data: &EconomyData,
    discord: &dyn EconomyDiscord,
    view: &dyn MemberView,
    guild: GuildId,
    user: UserId,
    now: Instant,
    at: DateTime<Utc>,
) -> Option<Instant> {
    data.economy.clear_jail_recheck(guild, user);
    let channel = view.voice(guild, user).and_then(|v| v.channel);
    on_voice_move(data, discord, guild, user, channel, now, at).await
}

/// Run the one re-check for a jailed member who moved out inside the rate
/// limit: sleep until `at`, then [`jail_recheck`] (again, if it asks).
pub fn spawn_jail_recheck(
    data: EconomyData,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild: GuildId,
    user: UserId,
    at: Instant,
) {
    tokio::spawn(async move {
        let discord = LiveEconomyDiscord { http };
        let mut at = at;
        loop {
            tokio::time::sleep_until(at).await;
            let checked = jail_recheck(
                &data,
                &discord,
                cache.as_ref(),
                guild,
                user,
                Instant::now(),
                Utc::now(),
            );
            match checked.await {
                Some(next) => at = next,
                None => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::discord::fake::{Call, CallKind, FakeEconomyDiscord};
    use crate::economy::discord::EconDiscordError;
    use crate::economy::effects::sweep;
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use sqlx::{types::Json, PgPool};

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(10);
    const BUYER: UserId = UserId::new(20);
    const BOT: UserId = UserId::new(30);
    const MOD: UserId = UserId::new(40);
    const JAIL: ChannelId = ChannelId::new(7);
    const ELSEWHERE: ChannelId = ChannelId::new(8);

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn data_with(pool: &PgPool) -> EconomyData {
        EconomyData {
            database_pool: Some(pool.clone()),
            ..Default::default()
        }
    }

    async fn seed(pool: &PgPool, kind: ActionKind, payload: &ActionPayload, state: &str) -> i64 {
        seed_until(
            pool,
            kind,
            payload,
            state,
            t0() - ChronoDuration::minutes(1),
        )
        .await
    }

    async fn seed_until(
        pool: &PgPool,
        kind: ActionKind,
        payload: &ActionPayload,
        state: &str,
        expires_at: DateTime<Utc>,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO economy_actions
                 (guild_id, kind, buyer_id, target_id, price, payload, state, expires_at, created_at)
             VALUES (1, $1, $2, $3, 100, $4, $5, $6, $7) RETURNING id",
        )
        .bind(kind.key())
        .bind(BUYER.get() as i64)
        .bind(U.get() as i64)
        .bind(Json(payload))
        .bind(state)
        .bind(expires_at)
        .bind(t0() - ChronoDuration::minutes(600))
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn rename_row(pool: &PgPool) -> i64 {
        seed(
            pool,
            ActionKind::Rename,
            &ActionPayload::Rename {
                name: "Goblin".into(),
                restore: Some("Al".into()),
            },
            "active",
        )
        .await
    }

    async fn lock_row(pool: &PgPool) -> i64 {
        seed(
            pool,
            ActionKind::Lock,
            &ActionPayload::Lock {
                locked: Some("Goblin".into()),
            },
            "active",
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

    fn entry(executor: UserId, changes: &str) -> AuditLogEntry {
        serde_json::from_str::<AuditLogEntry>(&format!(
            r#"{{"id": "900", "action_type": 24, "user_id": "{executor}", "target_id": "{U}",
                "changes": [{changes}]}}"#
        ))
        .expect("an audit log entry")
    }

    fn nick_entry(executor: UserId, new: &str) -> AuditLogEntry {
        entry(
            executor,
            &format!(r#"{{"key": "nick", "old_value": "Goblin", "new_value": "{new}"}}"#),
        )
    }

    macro_rules! db_test {
        ($name:ident, |$pool:ident| $body:block) => {
            #[crack_test_macros::db_test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) $body
        };
    }

    #[test]
    fn nick_change_rule_table() {
        assert_eq!(nick_change_rule(U, U, BOT), NickRule::Revert);
        assert_eq!(nick_change_rule(BOT, U, BOT), NickRule::Ignore);
        assert_eq!(nick_change_rule(MOD, U, BOT), NickRule::Overridden);
        assert_eq!(nick_change_rule(BOT, BOT, BOT), NickRule::Ignore);
    }

    db_test!(a_self_rename_under_lock_is_reverted, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let rename = rename_row(&pool).await;
        let lock = lock_row(&pool).await;
        on_audit_entry(&data, &fake, G, &nick_entry(U, "Mine"), BOT).await;
        assert_eq!(
            fake.calls(),
            vec![Call::Nick(U, Some("Goblin".into()), ActionKind::Lock)]
        );
        assert_eq!(state_of(&pool, rename).await.0, "active");
        assert_eq!(state_of(&pool, lock).await.0, "active");
    });

    db_test!(lock_bought_before_a_rename_holds_the_prank_name, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        // The lock first, while they are still "Al"; then the rename.
        let lock = seed(
            &pool,
            ActionKind::Lock,
            &ActionPayload::Lock {
                locked: Some("Al".into()),
            },
            "active",
        )
        .await;
        let rename = rename_row(&pool).await;
        assert!(lock < rename, "bought in that order");
        on_audit_entry(&data, &fake, G, &nick_entry(U, "Mine"), BOT).await;
        assert_eq!(
            fake.calls(),
            vec![Call::Nick(U, Some("Goblin".into()), ActionKind::Lock)],
            "the paid rename holds, not the name the lock captured"
        );
        assert_eq!(state_of(&pool, rename).await.0, "active");
        assert_eq!(state_of(&pool, lock).await.0, "active");

        // A clown holds the same way.
        sqlx::query("UPDATE economy_actions SET state = 'ended' WHERE id = $1")
            .bind(rename)
            .execute(&pool)
            .await
            .unwrap();
        seed(
            &pool,
            ActionKind::Clown,
            &ActionPayload::Clown {
                applied: "Al 🤡".into(),
                restore: Some("Al".into()),
            },
            "active",
        )
        .await;
        on_audit_entry(&data, &fake, G, &nick_entry(U, "Mine"), BOT).await;
        assert_eq!(
            fake.calls()[1],
            Call::Nick(U, Some("Al 🤡".into()), ActionKind::Lock)
        );

        // With the name prank over, the lock holds what it captured.
        sqlx::query("UPDATE economy_actions SET state = 'ended' WHERE kind = 'clown'")
            .execute(&pool)
            .await
            .unwrap();
        on_audit_entry(&data, &fake, G, &nick_entry(U, "Mine"), BOT).await;
        assert_eq!(
            fake.calls()[2],
            Call::Nick(U, Some("Al".into()), ActionKind::Lock)
        );
    });

    db_test!(a_self_rename_without_a_lock_escapes_the_prank, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let rename = rename_row(&pool).await;
        on_audit_entry(&data, &fake, G, &nick_entry(U, "Mine"), BOT).await;
        assert_eq!(
            state_of(&pool, rename).await,
            ("ended".into(), Some("escaped".into()))
        );
        // The sweeper has nothing left to restore over their new name.
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(fake.calls(), vec![]);
    });

    db_test!(a_mods_rename_ends_the_lock_and_is_kept, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let rename = rename_row(&pool).await;
        let lock = lock_row(&pool).await;
        on_audit_entry(&data, &fake, G, &nick_entry(MOD, "Boss"), BOT).await;
        for id in [rename, lock] {
            assert_eq!(
                state_of(&pool, id).await,
                ("ended".into(), Some("overridden".into()))
            );
        }
        // Both were due; the sweeper restores nothing for them.
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(fake.calls(), vec![]);
    });

    db_test!(a_mods_unmute_ends_the_mute_row, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let mute = seed(&pool, ActionKind::Mute, &ActionPayload::Mute, "active").await;
        let deaf = seed(&pool, ActionKind::Deafen, &ActionPayload::Deafen, "active").await;
        let unmute = entry(
            MOD,
            r#"{"key": "mute", "old_value": true, "new_value": false}"#,
        );
        on_audit_entry(&data, &fake, G, &unmute, BOT).await;
        assert_eq!(
            state_of(&pool, mute).await,
            ("ended".into(), Some("overridden".into()))
        );
        assert_eq!(state_of(&pool, deaf).await.0, "active", "only the mute");
        let undeaf = entry(
            MOD,
            r#"{"key": "deaf", "old_value": true, "new_value": false}"#,
        );
        on_audit_entry(&data, &fake, G, &undeaf, BOT).await;
        assert_eq!(
            state_of(&pool, deaf).await,
            ("ended".into(), Some("overridden".into()))
        );
        sweep(&data, &fake, t0(), &|_| true).await;
        assert_eq!(fake.calls(), vec![], "nothing is put back over a mod");
    });

    db_test!(the_shops_own_nick_change_is_ignored, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let rename = rename_row(&pool).await;
        let lock = lock_row(&pool).await;
        let mute = seed(&pool, ActionKind::Mute, &ActionPayload::Mute, "active").await;
        on_audit_entry(&data, &fake, G, &nick_entry(BOT, "Goblin"), BOT).await;
        let own_mute = entry(
            BOT,
            r#"{"key": "mute", "old_value": false, "new_value": true}"#,
        );
        on_audit_entry(&data, &fake, G, &own_mute, BOT).await;
        assert_eq!(fake.calls(), vec![]);
        for id in [rename, lock, mute] {
            assert_eq!(state_of(&pool, id).await.0, "active");
        }
    });

    db_test!(an_override_acts_with_the_economy_off, |pool| {
        let data = data_with(&pool);
        assert!(!data.economy.is_enabled(G));
        let fake = FakeEconomyDiscord::default();
        let rename = rename_row(&pool).await;
        on_audit_entry(&data, &fake, G, &nick_entry(MOD, "Boss"), BOT).await;
        assert_eq!(state_of(&pool, rename).await.0, "ended");
    });

    /// A jail running until half an hour after `t0()`.
    async fn jailed(pool: &PgPool) -> i64 {
        seed_until(
            pool,
            ActionKind::Jail,
            &ActionPayload::Jail { jail: JAIL },
            "active",
            t0() + ChronoDuration::minutes(30),
        )
        .await
    }

    db_test!(jail_moves_a_wanderer_back_once_per_10s, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        jailed(&pool).await;
        let t = Instant::now();
        let back = || vec![Call::Move(U, JAIL, ActionKind::Jail)];
        assert_eq!(
            on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t, t0()).await,
            None
        );
        assert_eq!(fake.calls(), back());
        // Inside 10 s: not moved now, but a re-check is asked for at the end.
        let recheck = on_voice_move(
            &data,
            &fake,
            G,
            U,
            Some(ELSEWHERE),
            t + Duration::from_secs(9),
            t0(),
        )
        .await;
        assert_eq!(fake.calls(), back(), "inside 10 s");
        assert_eq!(recheck, Some(t + JAIL_MOVE_EVERY));
        // The re-check runs (it clears its mark first), so this move is
        // the one it stands for.
        data.economy.clear_jail_recheck(G, U);
        on_voice_move(
            &data,
            &fake,
            G,
            U,
            Some(ELSEWHERE),
            t + Duration::from_secs(10),
            t0(),
        )
        .await;
        assert_eq!(fake.calls().len(), 2, "10 s later");
        // Another member's clock is their own.
        sqlx::query("UPDATE economy_actions SET target_id = 11")
            .execute(&pool)
            .await
            .unwrap();
        let other = UserId::new(11);
        on_voice_move(&data, &fake, G, other, Some(ELSEWHERE), t, t0()).await;
        assert_eq!(fake.calls().len(), 3);
    });

    #[test]
    fn jail_decision_table() {
        let t = Instant::now();
        let cd = JAIL_MOVE_EVERY;
        let s = Duration::from_secs;
        // Never moved, or the limit has passed: move now.
        assert_eq!(jail_decision(t, None, false, cd), JailMove::MoveNow);
        assert_eq!(
            jail_decision(t + s(10), Some(t), false, cd),
            JailMove::MoveNow
        );
        assert_eq!(
            jail_decision(t + s(10), Some(t), true, cd),
            JailMove::MoveNow
        );
        // Moved twice within 10 s: a re-check at the limit's end.
        assert_eq!(
            jail_decision(t + s(1), Some(t), false, cd),
            JailMove::ScheduleAt(t + cd)
        );
        // ...and only one.
        assert_eq!(
            jail_decision(t + s(2), Some(t), true, cd),
            JailMove::Nothing
        );
    }

    db_test!(moving_out_twice_quickly_does_not_escape_jail, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = jailed(&pool).await;
        let t = Instant::now();
        let s = Duration::from_secs;
        on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t, t0()).await;
        let at = on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t + s(1), t0())
            .await
            .expect("a re-check is scheduled");
        assert_eq!(fake.calls().len(), 1, "moved back once so far");
        // A third quick move schedules nothing more.
        assert_eq!(
            on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t + s(2), t0()).await,
            None
        );
        // At the re-check they are still out: moved back.
        let out = JailView(Some(ELSEWHERE));
        assert_eq!(jail_recheck(&data, &fake, &out, G, U, at, t0()).await, None);
        assert_eq!(
            fake.calls(),
            vec![
                Call::Move(U, JAIL, ActionKind::Jail),
                Call::Move(U, JAIL, ActionKind::Jail)
            ]
        );
        // A re-check that finds them in the jail, or gone from voice, or
        // the sentence over, does nothing.
        for (view, later) in [(JailView(Some(JAIL)), 30), (JailView(None), 50)] {
            assert_eq!(
                jail_recheck(&data, &fake, &view, G, U, t + s(later), t0()).await,
                None
            );
        }
        sqlx::query("UPDATE economy_actions SET state = 'ended' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            jail_recheck(&data, &fake, &out, G, U, t + s(70), t0()).await,
            None
        );
        assert_eq!(fake.calls().len(), 2);
    });

    /// Where the cache has the jailed member.
    struct JailView(Option<ChannelId>);
    impl MemberView for JailView {
        fn nick(&self, _g: GuildId, _u: UserId) -> Option<Option<String>> {
            None
        }
        fn voice(&self, _g: GuildId, _u: UserId) -> Option<crate::economy::effects::VoiceFacts> {
            Some(crate::economy::effects::VoiceFacts {
                channel: self.0,
                mute: false,
                deaf: false,
            })
        }
    }

    db_test!(a_jail_whose_time_is_up_moves_nobody, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        // Still `active` (the sweeper hasn't reached it), but its time is up.
        seed_until(
            &pool,
            ActionKind::Jail,
            &ActionPayload::Jail { jail: JAIL },
            "active",
            t0() - ChronoDuration::seconds(1),
        )
        .await;
        let t = Instant::now();
        assert_eq!(
            on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t, t0()).await,
            None
        );
        // Nor does a scheduled re-check that runs after the end.
        let out = JailView(Some(ELSEWHERE));
        assert_eq!(
            jail_recheck(&data, &fake, &out, G, U, t + Duration::from_secs(10), t0()).await,
            None
        );
        assert_eq!(fake.calls(), vec![], "free to go");
        // A second earlier it was still running, and they'd be moved back.
        let before = t0() - ChronoDuration::seconds(2);
        on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t, before).await;
        assert_eq!(fake.calls(), vec![Call::Move(U, JAIL, ActionKind::Jail)]);
    });

    db_test!(jail_lets_a_disconnect_be, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        jailed(&pool).await;
        on_voice_move(&data, &fake, G, U, None, Instant::now(), t0()).await;
        assert_eq!(fake.calls(), vec![]);
    });

    db_test!(
        jail_ignores_the_jail_channel_and_other_members_and_old_rows,
        |pool| {
            let data = data_with(&pool);
            let fake = FakeEconomyDiscord::default();
            let id = jailed(&pool).await;
            let t = Instant::now();
            on_voice_move(&data, &fake, G, U, Some(JAIL), t, t0()).await;
            on_voice_move(&data, &fake, G, UserId::new(99), Some(ELSEWHERE), t, t0()).await;
            assert_eq!(fake.calls(), vec![], "the jail itself; not the prisoner");
            sqlx::query("UPDATE economy_actions SET state = 'ended' WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
            on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), t, t0()).await;
            assert_eq!(fake.calls(), vec![], "the sentence is over");
        }
    );

    db_test!(a_failed_move_back_leaves_the_row, |pool| {
        let data = data_with(&pool);
        let fake = FakeEconomyDiscord::default();
        let id = jailed(&pool).await;
        fake.fail_next(CallKind::Move, EconDiscordError::MissingPermissions);
        on_voice_move(&data, &fake, G, U, Some(ELSEWHERE), Instant::now(), t0()).await;
        assert_eq!(fake.calls().len(), 1);
        assert_eq!(state_of(&pool, id).await.0, "active");
    });
}
