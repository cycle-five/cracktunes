//! A room's mute as a real server mute that lasts only while the member is in
//! the room.
//!
//! A hold row means CrackTunes server-muted this member for room R. One rule
//! covers every exit ([`hold_step`]); [`settle`] reads the member's voice
//! state fresh from Discord, applies it, and acts, one member at a time per
//! server.

use super::discord::{DiscordError, RoomsDiscord};
use super::RoomsData;
use crate::db::mute_holds;
use dashmap::DashMap;
use serenity::all::{ChannelId, GuildId, UserId};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// A member's voice state as Discord has it right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceNow {
    /// The channel they are in; `None` when not in voice.
    pub channel: Option<ChannelId>,
    /// Whether a server mute is on them, whoever put it there.
    pub server_muted: bool,
}

/// Why [`settle`] was called. Only the first two may take a new hold, and
/// only in that channel; a voice update in place (a self-mute, a camera) is
/// `Other`, so a moderator's unmute is never undone by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The member just joined or moved into this channel.
    Arrived(ChannelId),
    /// The room just muted the member (a button press, or a reconcile).
    MutedInRoom(ChannelId),
    Other,
}

impl Cause {
    fn may_take_in(self) -> Option<ChannelId> {
        match self {
            Cause::Arrived(c) | Cause::MutedInRoom(c) => Some(c),
            Cause::Other => None,
        }
    }
}

/// What to do about one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldStep {
    Nothing,
    /// Server-mute them and record a hold for this room.
    Mute(ChannelId),
    /// Lift the server mute and delete the hold.
    Unmute,
    /// The hold is now for this room (they moved from one muting room to another).
    Retarget(ChannelId),
    /// Delete the hold without touching Discord.
    Drop,
}

/// The whole rule, pure. `held_for` is the room the hold is for; `mutes(c)`
/// whether room `c` lists the member as muted (only ever asked of the channel
/// they are in); `may_take_in` the one channel a new hold may be taken in.
pub fn hold_step(
    held_for: Option<ChannelId>,
    now: VoiceNow,
    mutes: impl Fn(ChannelId) -> bool,
    may_take_in: Option<ChannelId>,
) -> HoldStep {
    match (held_for, now.channel) {
        (Some(_), None) => HoldStep::Unmute,
        (Some(r), Some(c)) if c != r => {
            if !now.server_muted {
                HoldStep::Drop
            } else if mutes(c) {
                HoldStep::Retarget(c)
            } else {
                HoldStep::Unmute
            }
        },
        // In the held room: someone else unmuting them wins; a room that no
        // longer mutes them means an earlier unmute failed.
        (Some(r), Some(_)) => {
            if !now.server_muted {
                HoldStep::Drop
            } else if !mutes(r) {
                HoldStep::Unmute
            } else {
                HoldStep::Nothing
            }
        },
        (None, Some(c)) if may_take_in == Some(c) && mutes(c) && !now.server_muted => {
            HoldStep::Mute(c)
        },
        // Someone else's mute stays theirs.
        (None, _) => HoldStep::Nothing,
    }
}

/// How [`settle`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    Unchanged,
    Muted,
    /// The bot may not mute members; the room's Speak deny applies on rejoin.
    MuteRefused,
    Unmuted,
    /// The member is not in voice: the hold is kept for their next join.
    StillOwed,
    Retargeted,
    Dropped,
    /// A database or Discord error; retried at the next event or reconcile.
    Failed,
}

/// Who holds a mute, as a filter, and the per-server locks that serialize
/// [`settle`]. The table is the truth; the filter only saves a database read
/// on voice events of members who hold nothing.
#[derive(Debug, Clone, Default)]
pub struct HoldIndex {
    held: DashMap<(GuildId, UserId), ChannelId>,
    locks: DashMap<GuildId, Arc<Mutex<()>>>,
}

impl HoldIndex {
    pub fn may_hold(&self, g: GuildId, u: UserId) -> bool {
        self.held.contains_key(&(g, u))
    }

    pub fn set(&self, g: GuildId, u: UserId, ch: ChannelId) {
        self.held.insert((g, u), ch);
    }

    pub fn clear(&self, g: GuildId, u: UserId) {
        self.held.remove(&(g, u));
    }

    /// The server's hold lock. Order: this first, then a room's lock, never
    /// the reverse.
    pub async fn lock(&self, g: GuildId) -> OwnedMutexGuard<()> {
        let lock = self.locks.entry(g).or_default().clone();
        lock.lock_owned().await
    }
}

/// Whether live room `ch` of server `g` has `u` muted. Takes the room's lock
/// briefly.
async fn room_mutes(data: &RoomsData, g: GuildId, ch: ChannelId, u: UserId) -> bool {
    let Some(room) = data.rooms.get(ch) else {
        return false;
    };
    let room = room.lock().await;
    room.guild == g && room.state.muted.contains(&u)
}

/// Bring `u`'s server mute in step with the rooms: read their voice state
/// now, decide with [`hold_step`], and act. Without a database: `Unchanged`.
///
/// 🪤 Lock order: the server's hold lock (taken here), then briefly a room's
/// lock to read `state.muted`. Never take the hold lock while holding a room
/// lock, so never call this with one held.
///
/// A failed unmute keeps the hold; it is retried at the next event or
/// reconcile, never looped.
pub async fn settle(
    data: &RoomsData,
    discord: &dyn RoomsDiscord,
    g: GuildId,
    u: UserId,
    cause: Cause,
) -> Settled {
    let Some(pool) = &data.database_pool else {
        return Settled::Unchanged;
    };
    let holds = &data.rooms.holds;
    let _serialized = holds.lock(g).await;

    let held = match mute_holds::held_for(pool, g, u).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("room mute: {g}: reading the hold on {u}: {e}");
            return Settled::Failed;
        },
    };
    match held {
        Some(r) => holds.set(g, u, r),
        None => holds.clear(g, u),
    }
    let may_take_in = cause.may_take_in();
    if held.is_none() {
        // Nothing owed and nothing to take: no reason to ask Discord.
        let Some(c) = may_take_in else {
            return Settled::Unchanged;
        };
        if !room_mutes(data, g, c, u).await {
            return Settled::Unchanged;
        }
    }

    let now = match discord.voice_now(g, u).await {
        Ok(now) => now,
        Err(e) => {
            tracing::warn!("room mute: {g}: reading the voice state of {u}: {e:?}");
            return Settled::Failed;
        },
    };
    // `hold_step` only asks about the channel they are in.
    let in_room_mutes = match now.channel {
        Some(c) => room_mutes(data, g, c, u).await,
        None => false,
    };
    let mutes = |c: ChannelId| now.channel == Some(c) && in_room_mutes;

    match hold_step(held, now, mutes, may_take_in) {
        HoldStep::Nothing => Settled::Unchanged,
        HoldStep::Mute(c) => {
            if let Err(e) = mute_holds::hold(pool, g, u, c).await {
                tracing::warn!("room mute: {g}: recording the hold on {u}: {e}");
                return Settled::Failed;
            }
            holds.set(g, u, c);
            match discord.set_mute(g, u, true).await {
                Ok(()) => Settled::Muted,
                Err(e) => {
                    forget(pool, holds, g, u).await;
                    match e {
                        DiscordError::MissingPermissions => Settled::MuteRefused,
                        DiscordError::NotConnected => Settled::Unchanged,
                        other => {
                            tracing::warn!("room mute: {g}: muting {u}: {other:?}");
                            Settled::Failed
                        },
                    }
                },
            }
        },
        HoldStep::Unmute => match discord.set_mute(g, u, false).await {
            Ok(()) => {
                forget(pool, holds, g, u).await;
                Settled::Unmuted
            },
            Err(DiscordError::NotConnected) => Settled::StillOwed,
            Err(e) => {
                tracing::warn!("room mute: {g}: unmuting {u}: {e:?}");
                Settled::Failed
            },
        },
        HoldStep::Retarget(c) => match mute_holds::hold(pool, g, u, c).await {
            Ok(()) => {
                holds.set(g, u, c);
                Settled::Retargeted
            },
            Err(e) => {
                tracing::warn!("room mute: {g}: moving the hold on {u}: {e}");
                Settled::Failed
            },
        },
        HoldStep::Drop => {
            forget(pool, holds, g, u).await;
            Settled::Dropped
        },
    }
}

/// Delete the hold row and its filter entry. A failed delete is logged: the
/// row then says a mute is owed that is not, and the next settle drops it.
async fn forget(pool: &sqlx::PgPool, holds: &HoldIndex, g: GuildId, u: UserId) {
    holds.clear(g, u);
    if let Err(e) = mute_holds::release(pool, g, u).await {
        tracing::warn!("room mute: {g}: deleting the hold on {u}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::registry::{LiveRoom, RenameWindow};
    use crate::rooms::state::RoomState;
    use crate::rooms::test_support::{Call, CallKind, FakeRoomsDiscord};
    use sqlx::PgPool;

    const R: ChannelId = ChannelId::new(10);
    const OTHER: ChannelId = ChannelId::new(11);
    const HUB: ChannelId = ChannelId::new(12);

    fn at(c: ChannelId, server_muted: bool) -> VoiceNow {
        VoiceNow {
            channel: Some(c),
            server_muted,
        }
    }
    const OUT: VoiceNow = VoiceNow {
        channel: None,
        server_muted: false,
    };

    #[test]
    fn hold_step_covers_every_exit() {
        let mutes_r = |c: ChannelId| c == R;
        let mutes_other = |c: ChannelId| c == OTHER;
        let none = |_: ChannelId| false;
        // held for R:
        // not in voice at all
        assert_eq!(hold_step(Some(R), OUT, none, None), HoldStep::Unmute);
        assert_eq!(
            hold_step(
                Some(R),
                VoiceNow {
                    channel: None,
                    server_muted: true
                },
                none,
                None
            ),
            HoldStep::Unmute
        );
        // moved to a room that mutes them too
        assert_eq!(
            hold_step(Some(R), at(OTHER, true), mutes_other, None),
            HoldStep::Retarget(OTHER)
        );
        // moved to the hub / a room that does not mute them
        assert_eq!(
            hold_step(Some(R), at(HUB, true), none, None),
            HoldStep::Unmute
        );
        assert_eq!(
            hold_step(Some(R), at(OTHER, true), mutes_r, None),
            HoldStep::Unmute
        );
        // elsewhere and already unmuted by someone: nothing left to lift
        assert_eq!(
            hold_step(Some(R), at(OTHER, false), mutes_other, None),
            HoldStep::Drop
        );
        assert_eq!(
            hold_step(Some(R), at(HUB, false), none, None),
            HoldStep::Drop
        );
        // in R:
        assert_eq!(
            hold_step(Some(R), at(R, false), mutes_r, None),
            HoldStep::Drop
        );
        assert_eq!(
            hold_step(Some(R), at(R, true), none, None),
            HoldStep::Unmute
        );
        assert_eq!(
            hold_step(Some(R), at(R, true), mutes_r, None),
            HoldStep::Nothing
        );
        // no hold:
        assert_eq!(
            hold_step(None, at(R, false), mutes_r, Some(R)),
            HoldStep::Mute(R)
        );
        assert_eq!(
            hold_step(None, at(R, false), none, Some(R)),
            HoldStep::Nothing
        );
        assert_eq!(
            hold_step(None, at(OTHER, false), mutes_other, Some(R)),
            HoldStep::Nothing
        );
        assert_eq!(hold_step(None, OUT, mutes_r, Some(R)), HoldStep::Nothing);
    }

    #[test]
    fn in_place_update_never_takes_a_hold() {
        // A moderator unmuted them in R; their self-mute toggle must not re-mute.
        assert_eq!(
            hold_step(None, at(R, false), |c| c == R, None),
            HoldStep::Nothing
        );
    }

    #[test]
    fn someone_elses_mute_is_never_taken() {
        assert_eq!(
            hold_step(None, at(R, true), |c| c == R, Some(R)),
            HoldStep::Nothing
        );
    }

    const G: GuildId = GuildId::new(1);
    const T: UserId = UserId::new(7);
    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

    /// Rooms data on `pool`, with room R live and muting T.
    fn data_with_room(pool: PgPool) -> RoomsData {
        let data = RoomsData {
            database_pool: Some(pool),
            ..RoomsData::default()
        };
        let mut state = RoomState::new(UserId::new(100));
        state.muted.insert(T);
        data.rooms.insert(LiveRoom {
            guild: G,
            channel: R,
            name: "room".into(),
            state,
            baseline: vec![],
            limit: 0,
            renames: RenameWindow::default(),
            wrote_at: None,
        });
        data
    }

    async fn held(data: &RoomsData) -> Option<ChannelId> {
        let pool = data.database_pool.as_ref().unwrap();
        mute_holds::held_for(pool, G, T).await.unwrap()
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn muting_in_the_room_takes_a_hold_and_mutes(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        let out = settle(&data, &fake, G, T, Cause::MutedInRoom(R)).await;
        assert_eq!(out, Settled::Muted);
        assert_eq!(
            fake.calls(),
            vec![Call::VoiceNow(7), Call::SetMute(7, true)]
        );
        assert_eq!(held(&data).await, Some(R));
        assert!(data.rooms.holds.may_hold(G, T));
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn moving_out_unmutes_and_releases(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        settle(&data, &fake, G, T, Cause::Arrived(R)).await;
        fake.set_voice(T, at(HUB, true));
        let out = settle(&data, &fake, G, T, Cause::Arrived(HUB)).await;
        assert_eq!(out, Settled::Unmuted);
        assert_eq!(fake.calls().last(), Some(&Call::SetMute(7, false)));
        assert_eq!(held(&data).await, None);
        assert!(!data.rooms.holds.may_hold(G, T));
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn disconnecting_keeps_the_debt(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        settle(&data, &fake, G, T, Cause::Arrived(R)).await;
        // They left voice; Discord refuses to unmute them.
        fake.set_voice(T, OUT);
        fake.fail_next(CallKind::SetMute, DiscordError::NotConnected);
        let out = settle(&data, &fake, G, T, Cause::Other).await;
        assert_eq!(out, Settled::StillOwed);
        assert_eq!(held(&data).await, Some(R));
        // They join elsewhere, still muted: the debt is paid.
        fake.set_voice(T, at(HUB, true));
        let out = settle(&data, &fake, G, T, Cause::Arrived(HUB)).await;
        assert_eq!(out, Settled::Unmuted);
        assert_eq!(held(&data).await, None);
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_moderator_unmute_drops_the_hold_without_a_call(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        settle(&data, &fake, G, T, Cause::Arrived(R)).await;
        // A moderator unmutes them in R; then they toggle their camera.
        fake.set_voice(T, at(R, false));
        let out = settle(&data, &fake, G, T, Cause::Other).await;
        assert_eq!(out, Settled::Dropped);
        assert_eq!(held(&data).await, None);
        assert!(!fake.calls().contains(&Call::SetMute(7, false)));
        // The next in-place update takes nothing.
        let out = settle(&data, &fake, G, T, Cause::Other).await;
        assert_eq!(out, Settled::Unchanged);
        assert_eq!(held(&data).await, None);
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn no_mute_members_falls_back(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        fake.fail_next(CallKind::SetMute, DiscordError::MissingPermissions);
        let out = settle(&data, &fake, G, T, Cause::MutedInRoom(R)).await;
        assert_eq!(out, Settled::MuteRefused);
        assert_eq!(held(&data).await, None);
        assert!(!data.rooms.holds.may_hold(G, T));
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn no_hold_and_no_cause_makes_no_call(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        assert_eq!(
            settle(&data, &fake, G, T, Cause::Other).await,
            Settled::Unchanged
        );
        // A cause for a room that does not mute them takes nothing either.
        assert_eq!(
            settle(&data, &fake, G, T, Cause::Arrived(OTHER)).await,
            Settled::Unchanged
        );
        assert_eq!(fake.calls(), vec![]);
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_failed_unmute_is_kept_for_retry(pool: PgPool) {
        let data = data_with_room(pool);
        let fake = FakeRoomsDiscord::default();
        fake.set_voice(T, at(R, false));
        settle(&data, &fake, G, T, Cause::Arrived(R)).await;
        fake.set_voice(T, at(HUB, true));
        fake.fail_next(CallKind::SetMute, DiscordError::Other("boom".into()));
        let out = settle(&data, &fake, G, T, Cause::Arrived(HUB)).await;
        assert_eq!(out, Settled::Failed);
        assert_eq!(held(&data).await, Some(R));
        // The next event retries and pays it.
        let out = settle(&data, &fake, G, T, Cause::Other).await;
        assert_eq!(out, Settled::Unmuted);
        assert_eq!(held(&data).await, None);
    }
}
