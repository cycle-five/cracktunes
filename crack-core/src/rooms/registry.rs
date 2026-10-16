//! The live rooms, in memory: one mutex per room, so two presses on one room
//! run one after the other, plus the indexes and per-server settings the
//! lifecycle reads on every voice event.

use super::settings::RoomSettings;
use super::state::{RoomState, StoredOverwrite};
use crate::messaging::throttle::Throttle;
use dashmap::{DashMap, DashSet};
use serenity::all::{ChannelId, GuildId, UserId};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// One new room per member per this long.
pub const NEW_ROOM_COOLDOWN: Duration = Duration::from_secs(10);

/// A room as the bot holds it while it lives.
#[derive(Debug, Clone)]
pub struct LiveRoom {
    pub guild: GuildId,
    pub channel: ChannelId,
    /// The channel's current name, for the panel's title.
    pub name: String,
    pub state: RoomState,
    /// The overwrites Discord gave the channel at creation.
    pub baseline: Vec<StoredOverwrite>,
    /// The channel's member limit; 0 means none.
    pub limit: u8,
    pub renames: RenameWindow,
}

/// Discord lets a channel be renamed twice per ten minutes, and serenity
/// waits out a 429 rather than failing, which would hold the room's mutex for
/// the rest of the window. This counts the renames so the third is refused
/// up front with the wait. In memory only.
#[derive(Debug, Clone, Default)]
pub struct RenameWindow(VecDeque<Instant>);

impl RenameWindow {
    pub const MAX: usize = 2;
    pub const WINDOW: Duration = Duration::from_secs(600);

    /// Take a rename slot at `now`, or say how long until one frees up.
    pub fn try_take(&mut self, now: Instant) -> Result<(), Duration> {
        while self
            .0
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= Self::WINDOW)
        {
            self.0.pop_front();
        }
        match self.0.front() {
            Some(oldest) if self.0.len() >= Self::MAX => {
                Err(Self::WINDOW - now.saturating_duration_since(*oldest))
            },
            _ => {
                self.0.push_back(now);
                Ok(())
            },
        }
    }
}

/// Every live room, and what the lifecycle needs to find them.
#[derive(Debug, Clone)]
pub struct RoomRegistry {
    /// Each room with its guild, so a server's rooms can be counted without
    /// taking every room's lock.
    rooms: DashMap<ChannelId, (GuildId, Arc<Mutex<LiveRoom>>)>,
    /// One room per member per server.
    owners: DashMap<(GuildId, UserId), ChannelId>,
    settings: DashMap<GuildId, RoomSettings>,
    /// A pending empty-room deletion per room.
    timers: DashMap<ChannelId, AbortHandle>,
    /// One new room per member per [`NEW_ROOM_COOLDOWN`].
    pub cooldown: Throttle<(GuildId, UserId)>,
    /// Members whose room is being created right now, so a doubled voice
    /// event cannot make two.
    pub hub_creating: DashSet<(GuildId, UserId)>,
}

impl Default for RoomRegistry {
    fn default() -> Self {
        Self {
            rooms: DashMap::new(),
            owners: DashMap::new(),
            settings: DashMap::new(),
            timers: DashMap::new(),
            cooldown: Throttle::new(NEW_ROOM_COOLDOWN),
            hub_creating: DashSet::new(),
        }
    }
}

impl RoomRegistry {
    /// The server's settings; rooms off when it never configured them.
    pub fn settings(&self, g: GuildId) -> RoomSettings {
        self.settings
            .get(&g)
            .map(|s| s.clone())
            .unwrap_or_else(|| RoomSettings::off(g))
    }

    pub fn set_settings(&self, s: RoomSettings) {
        self.settings.insert(s.guild, s);
    }

    pub fn get(&self, ch: ChannelId) -> Option<Arc<Mutex<LiveRoom>>> {
        self.rooms.get(&ch).map(|e| e.value().1.clone())
    }

    /// Register a room and index it under its owner.
    pub fn insert(&self, room: LiveRoom) {
        let (guild, channel, owner) = (room.guild, room.channel, room.state.owner);
        self.owners.insert((guild, owner), channel);
        self.rooms
            .insert(channel, (guild, Arc::new(Mutex::new(room))));
    }

    /// Forget a room: its owner index entry goes, and a pending deletion timer
    /// is aborted. 🪤 A timer task that calls this aborts itself at its next
    /// `.await`: take the handle out with [`Self::disarm_timer`] first, or
    /// call this last.
    pub fn remove(&self, ch: ChannelId) -> Option<Arc<Mutex<LiveRoom>>> {
        let removed = self.rooms.remove(&ch).map(|(_, (_, room))| room);
        self.owners.retain(|_, room| *room != ch);
        self.disarm_timer(ch);
        removed
    }

    pub fn owned_by(&self, g: GuildId, u: UserId) -> Option<ChannelId> {
        self.owners.get(&(g, u)).map(|c| *c)
    }

    /// Ownership of `ch` moved from `old` to `new`.
    pub fn reindex_owner(&self, g: GuildId, old: UserId, new: UserId, ch: ChannelId) {
        self.owners.remove_if(&(g, old), |_, room| *room == ch);
        self.owners.insert((g, new), ch);
    }

    /// How many rooms live in the server.
    pub fn count_in(&self, g: GuildId) -> usize {
        self.rooms.iter().filter(|e| e.value().0 == g).count()
    }

    /// Hold `handle` as the room's pending deletion, aborting any earlier one.
    pub fn arm_timer(&self, ch: ChannelId, handle: AbortHandle) {
        if let Some(old) = self.timers.insert(ch, handle) {
            old.abort();
        }
    }

    /// Abort the room's pending deletion, if any. Whether there was one.
    pub fn disarm_timer(&self, ch: ChannelId) -> bool {
        match self.timers.remove(&ch) {
            Some((_, handle)) => {
                handle.abort();
                true
            },
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = GuildId::new(100);
    const SEC: Duration = Duration::from_secs(1);

    fn room(channel: u64, guild: GuildId, owner: u64) -> LiveRoom {
        LiveRoom {
            guild,
            channel: ChannelId::new(channel),
            name: "room".into(),
            state: RoomState::new(UserId::new(owner)),
            baseline: vec![],
            limit: 0,
            renames: RenameWindow::default(),
        }
    }

    #[test]
    fn rename_window_allows_two_then_says_how_long_to_wait() {
        let mut w = RenameWindow::default();
        let t0 = Instant::now();
        assert_eq!(w.try_take(t0), Ok(()));
        assert_eq!(w.try_take(t0 + 60 * SEC), Ok(()));
        assert_eq!(
            w.try_take(t0 + 100 * SEC),
            Err(RenameWindow::WINDOW - 100 * SEC),
            "the oldest rename frees its slot ten minutes after it"
        );
    }

    #[test]
    fn rename_window_frees_a_slot_once_the_oldest_rename_ages_out() {
        let mut w = RenameWindow::default();
        let t0 = Instant::now();
        w.try_take(t0).unwrap();
        w.try_take(t0 + 60 * SEC).unwrap();
        assert!(w.try_take(t0 + RenameWindow::WINDOW - SEC).is_err());
        assert_eq!(w.try_take(t0 + RenameWindow::WINDOW), Ok(()));
        // The second rename is still inside its window, and so is the third.
        assert_eq!(
            w.try_take(t0 + RenameWindow::WINDOW + SEC),
            Err(60 * SEC - SEC)
        );
    }

    #[test]
    fn rename_window_refusal_does_not_take_a_slot() {
        let mut w = RenameWindow::default();
        let t0 = Instant::now();
        w.try_take(t0).unwrap();
        w.try_take(t0).unwrap();
        for _ in 0..5 {
            assert!(w.try_take(t0 + SEC).is_err());
        }
        assert_eq!(w.try_take(t0 + RenameWindow::WINDOW), Ok(()));
        assert_eq!(w.try_take(t0 + RenameWindow::WINDOW), Ok(()));
    }

    #[test]
    fn settings_default_to_off() {
        let reg = RoomRegistry::default();
        assert_eq!(reg.settings(G), RoomSettings::off(G));
        let on = RoomSettings {
            enabled: true,
            hub: Some(ChannelId::new(5)),
            ..RoomSettings::off(G)
        };
        reg.set_settings(on.clone());
        assert_eq!(reg.settings(G), on);
        assert_eq!(
            reg.settings(GuildId::new(7)),
            RoomSettings::off(GuildId::new(7))
        );
    }

    #[tokio::test]
    async fn insert_indexes_the_owner_and_remove_forgets_both() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        reg.insert(room(11, G, 2));
        reg.insert(room(20, GuildId::new(200), 1));
        assert_eq!(reg.owned_by(G, UserId::new(1)), Some(ChannelId::new(10)));
        assert_eq!(
            reg.owned_by(GuildId::new(200), UserId::new(1)),
            Some(ChannelId::new(20))
        );
        assert_eq!(reg.count_in(G), 2);
        let got = reg.get(ChannelId::new(11)).expect("registered");
        assert_eq!(got.lock().await.state.owner, UserId::new(2));

        assert!(reg.remove(ChannelId::new(10)).is_some());
        assert!(reg.get(ChannelId::new(10)).is_none());
        assert_eq!(reg.owned_by(G, UserId::new(1)), None);
        assert_eq!(
            reg.owned_by(GuildId::new(200), UserId::new(1)),
            Some(ChannelId::new(20)),
            "another server's room of the same member stays"
        );
        assert_eq!(reg.count_in(G), 1);
        assert!(reg.remove(ChannelId::new(10)).is_none());
    }

    #[test]
    fn reindex_owner_moves_the_index_entry() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        reg.reindex_owner(G, UserId::new(1), UserId::new(2), ChannelId::new(10));
        assert_eq!(reg.owned_by(G, UserId::new(1)), None);
        assert_eq!(reg.owned_by(G, UserId::new(2)), Some(ChannelId::new(10)));
    }

    #[test]
    fn reindex_owner_keeps_the_old_owners_other_room() {
        let reg = RoomRegistry::default();
        reg.insert(room(11, G, 1));
        // Room 10 was never indexed under member 1, so its transfer must not
        // drop member 1's own room.
        reg.reindex_owner(G, UserId::new(1), UserId::new(2), ChannelId::new(10));
        assert_eq!(reg.owned_by(G, UserId::new(1)), Some(ChannelId::new(11)));
    }

    /// The task's end, which must be an abort: a timer left running fails
    /// here rather than hanging the test run.
    async fn cancelled(task: tokio::task::JoinHandle<()>) -> tokio::task::JoinError {
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the timer was aborted, not left running")
            .expect_err("aborted")
    }

    #[tokio::test]
    async fn remove_aborts_the_pending_deletion() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        let task = tokio::spawn(std::future::pending::<()>());
        reg.arm_timer(ChannelId::new(10), task.abort_handle());
        reg.remove(ChannelId::new(10));
        let err = cancelled(task).await;
        assert!(err.is_cancelled());
        assert!(!reg.disarm_timer(ChannelId::new(10)), "and forgets it");
    }

    #[tokio::test]
    async fn arming_again_aborts_the_earlier_timer() {
        let reg = RoomRegistry::default();
        let first = tokio::spawn(std::future::pending::<()>());
        let second = tokio::spawn(std::future::pending::<()>());
        reg.arm_timer(ChannelId::new(10), first.abort_handle());
        reg.arm_timer(ChannelId::new(10), second.abort_handle());
        assert!(cancelled(first).await.is_cancelled());
        assert!(reg.disarm_timer(ChannelId::new(10)));
        assert!(cancelled(second).await.is_cancelled());
    }
}
