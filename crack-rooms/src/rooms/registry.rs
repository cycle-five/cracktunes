//! The live rooms, in memory: one mutex per room, so two presses on one room
//! run one after the other, plus the indexes and per-server settings the
//! lifecycle reads on every voice event.

use super::mute_hold::HoldIndex;
use super::settings::RoomSettings;
use super::state::{RoomState, StoredOverwrite};
use crack_common::throttle::Throttle;
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
    /// When the bot last wrote the room's overwrites. Until the cache has
    /// caught up with that, its view of the room is not folded into the
    /// baseline (R33): the bot's own writes would read as someone else's.
    pub wrote_at: Option<Instant>,
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
    /// Servers whose stored settings have been read.
    loaded: DashSet<GuildId>,
    /// A pending empty-room deletion per room.
    timers: DashMap<ChannelId, AbortHandle>,
    /// One new room per member per [`NEW_ROOM_COOLDOWN`].
    pub cooldown: Throttle<(GuildId, UserId)>,
    /// Members whose room is being created right now, so a doubled voice
    /// event cannot make two.
    pub hub_creating: DashSet<(GuildId, UserId)>,
    /// Rooms a hub join has registered but is still setting up. Reconcile
    /// leaves them alone (the cache may not have seen their channel yet),
    /// and the hub does not move their owner into them.
    pub creating: DashSet<ChannelId>,
    /// Who holds a room's server mute, and the per-server lock that
    /// serializes `mute_hold::settle`.
    pub holds: HoldIndex,
}

impl Default for RoomRegistry {
    fn default() -> Self {
        Self {
            rooms: DashMap::new(),
            owners: DashMap::new(),
            settings: DashMap::new(),
            loaded: DashSet::new(),
            timers: DashMap::new(),
            cooldown: Throttle::new(NEW_ROOM_COOLDOWN),
            hub_creating: DashSet::new(),
            creating: DashSet::new(),
            holds: HoldIndex::default(),
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

    /// Whether the server's stored settings have been read (or found
    /// absent). Until they are, [`Self::settings`] answers defaults that a
    /// save must not write over what is stored.
    pub fn settings_loaded(&self, g: GuildId) -> bool {
        self.loaded.contains(&g)
    }

    /// The server's stored settings have been read.
    pub fn mark_settings_loaded(&self, g: GuildId) {
        self.loaded.insert(g);
    }

    pub fn get(&self, ch: ChannelId) -> Option<Arc<Mutex<LiveRoom>>> {
        self.rooms.get(&ch).map(|e| e.value().1.clone())
    }

    /// Register a room and index it under its owner, unless the room blocks
    /// them: a moderator kicked its owner, who stays `state.owner` but owns
    /// it no more (see [`Self::unindex_owner`]). A room already registered
    /// under the channel is replaced, and its owner entry dropped. The
    /// registered room's handle.
    pub fn insert(&self, room: LiveRoom) -> Arc<Mutex<LiveRoom>> {
        let (guild, channel, owner) = (room.guild, room.channel, room.state.owner);
        self.owners.retain(|_, room| *room != channel);
        if !room.state.blocked.contains(&owner) {
            self.owners.insert((guild, owner), channel);
        }
        let handle = Arc::new(Mutex::new(room));
        self.rooms.insert(channel, (guild, handle.clone()));
        handle
    }

    /// Forget a room: its owner index entry goes, and a pending deletion timer
    /// is aborted.
    ///
    /// Callers must hold the room's mutex while removing it. `ops::act` takes
    /// the room and then waits for that lock; once it has the lock it checks
    /// the room is still the registered one, and an action that lost the race
    /// is refused rather than run on a room nobody can find.
    ///
    /// 🪤 A timer task that calls this aborts itself at its next `.await`:
    /// take its own handle out with [`Self::release_timer`] first
    /// ([`Self::disarm_timer`] aborts it too), or call this last.
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

    /// `u` no longer owns `ch` (kicked from it). An entry pointing at
    /// another room is left alone.
    pub fn unindex_owner(&self, g: GuildId, u: UserId, ch: ChannelId) {
        self.owners.remove_if(&(g, u), |_, room| *room == ch);
    }

    /// How many rooms live in the server.
    pub fn count_in(&self, g: GuildId) -> usize {
        self.rooms.iter().filter(|e| e.value().0 == g).count()
    }

    /// The server's rooms.
    pub fn rooms_in(&self, g: GuildId) -> Vec<ChannelId> {
        self.rooms
            .iter()
            .filter(|e| e.value().0 == g)
            .map(|e| *e.key())
            .collect()
    }

    /// Whether a deletion is pending for the room: armed, and not yet run
    /// to its end.
    pub fn timer_pending(&self, ch: ChannelId) -> bool {
        self.timers.get(&ch).is_some_and(|h| !h.is_finished())
    }

    /// A timer task taking its own handle out of the map, unaborted, before
    /// it deletes the room ([`Self::remove`] would otherwise abort it mid-way).
    /// Only the handle of task `id` goes: one armed since stays.
    pub fn release_timer(&self, ch: ChannelId, id: tokio::task::Id) {
        self.timers.remove_if(&ch, |_, h| h.id() == id);
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
            wrote_at: None,
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

    #[test]
    fn settings_count_as_loaded_once_marked_per_server() {
        let reg = RoomRegistry::default();
        assert!(!reg.settings_loaded(G));
        reg.mark_settings_loaded(G);
        assert!(reg.settings_loaded(G));
        assert!(!reg.settings_loaded(GuildId::new(7)));
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
    fn reinserting_a_channel_drops_its_old_owner_entry() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        reg.insert(room(10, G, 2));
        assert_eq!(reg.owned_by(G, UserId::new(1)), None);
        assert_eq!(reg.owned_by(G, UserId::new(2)), Some(ChannelId::new(10)));
        assert_eq!(reg.count_in(G), 1);
    }

    /// A kicked owner stays the room's `state.owner`. Loading that room
    /// again (a reconcile after a restart) must not index them under it:
    /// they would count as owning it (a transfer or claim to them refused),
    /// and the entry of a room they do own would be overwritten.
    #[test]
    fn insert_does_not_index_an_owner_the_room_blocks() {
        let reg = RoomRegistry::default();
        reg.insert(room(11, G, 1));
        let mut kicked = room(10, G, 1);
        kicked.state.blocked.insert(UserId::new(1));
        reg.insert(kicked.clone());
        assert_eq!(
            reg.owned_by(G, UserId::new(1)),
            Some(ChannelId::new(11)),
            "their own room stays theirs"
        );
        assert!(
            reg.get(ChannelId::new(10)).is_some(),
            "the room is registered"
        );
        assert_eq!(reg.count_in(G), 2);

        // With no other room, they own none.
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 2));
        reg.insert(kicked);
        assert_eq!(reg.owned_by(G, UserId::new(1)), None);
        assert_eq!(reg.owned_by(G, UserId::new(2)), None, "the old entry went");
    }

    #[test]
    fn unindex_owner_drops_only_that_rooms_entry() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        reg.unindex_owner(G, UserId::new(1), ChannelId::new(11));
        assert_eq!(
            reg.owned_by(G, UserId::new(1)),
            Some(ChannelId::new(10)),
            "another room's entry stays"
        );
        reg.unindex_owner(G, UserId::new(1), ChannelId::new(10));
        assert_eq!(reg.owned_by(G, UserId::new(1)), None);
        assert!(reg.get(ChannelId::new(10)).is_some(), "the room stays");
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

    #[test]
    fn rooms_in_lists_only_that_servers_rooms() {
        let reg = RoomRegistry::default();
        reg.insert(room(10, G, 1));
        reg.insert(room(11, G, 2));
        reg.insert(room(20, GuildId::new(200), 1));
        let mut got = reg.rooms_in(G);
        got.sort();
        assert_eq!(got, vec![ChannelId::new(10), ChannelId::new(11)]);
        assert!(reg.rooms_in(GuildId::new(300)).is_empty());
    }

    #[tokio::test]
    async fn timer_pending_is_false_once_the_task_ends_or_is_disarmed() {
        let reg = RoomRegistry::default();
        let ch = ChannelId::new(10);
        assert!(!reg.timer_pending(ch));
        let task = tokio::spawn(std::future::pending::<()>());
        reg.arm_timer(ch, task.abort_handle());
        assert!(reg.timer_pending(ch));
        reg.disarm_timer(ch);
        assert!(!reg.timer_pending(ch));

        // A handle whose task already ran to its end is not pending.
        let done = tokio::spawn(async {});
        let handle = done.abort_handle();
        done.await.unwrap();
        reg.arm_timer(ch, handle);
        assert!(!reg.timer_pending(ch));
    }

    #[tokio::test]
    async fn release_timer_takes_out_only_that_tasks_handle_without_aborting_it() {
        let reg = Arc::new(RoomRegistry::default());
        let ch = ChannelId::new(10);
        let (go, wait) = tokio::sync::oneshot::channel::<()>();
        let r = reg.clone();
        let task = tokio::spawn(async move {
            wait.await.unwrap();
            r.release_timer(ch, tokio::task::id());
            // Still running after the release: the next await is reached.
            tokio::task::yield_now().await;
            7
        });
        reg.arm_timer(ch, task.abort_handle());
        go.send(()).unwrap();
        assert_eq!(task.await.expect("not aborted"), 7);
        assert!(!reg.timer_pending(ch));
        assert!(!reg.disarm_timer(ch), "the handle was taken out");

        // Another task's handle stays put.
        let other = tokio::spawn(std::future::pending::<()>());
        reg.arm_timer(ch, other.abort_handle());
        let stranger = tokio::spawn(async { tokio::task::id() }).await.unwrap();
        reg.release_timer(ch, stranger);
        assert!(reg.timer_pending(ch));
        assert!(reg.disarm_timer(ch));
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
