//! The economy's in-memory state, held by the bot's shared data: per-server
//! settings, which servers' stored settings have been read, who is opted out,
//! and which walk-on themes are playing.

use super::earn::cooldown_allows;
use super::lock::{jail_decision, JailMove};
use super::settings::EconomySettings;
use chrono::{DateTime, Utc};
use dashmap::{mapref::entry::Entry, DashMap, DashSet};
use serenity::all::{GuildId, UserId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct EconomyRegistry {
    settings: DashMap<GuildId, EconomySettings>,
    loaded: DashSet<GuildId>,
    opted_out: DashMap<GuildId, HashSet<UserId>>,
    /// Messages counted since the last tick, per server and member.
    pending_messages: DashMap<GuildId, HashMap<UserId, u32>>,
    /// When each member's last message was counted (the cooldown's clock).
    last_counted: DashMap<(GuildId, UserId), Instant>,
    /// When each jailed member was last moved back (the jail's rate limit).
    /// Entries older than the limit are pruned on each move.
    last_jail_move: DashMap<(GuildId, UserId), Instant>,
    /// The one re-check scheduled per jailed member who moved inside the
    /// rate limit, and when it runs.
    jail_rechecks: DashMap<(GuildId, UserId), Instant>,
    /// The earning tick has been spawned.
    started: Arc<AtomicBool>,
    sweeper_started: Arc<AtomicBool>,
    /// Undos the sweeper is backing off from, by action id: how many times
    /// each has failed and when it may be tried next.
    undo_backoff: DashMap<i64, (u32, DateTime<Utc>)>,
    /// When this process started, on the database's clock: a `pending` row
    /// older than this was left by a previous process.
    process_start: Arc<OnceLock<DateTime<Utc>>>,
    /// Servers where a walk-on theme is playing: one at a time per server.
    /// Held by a [`ThemeGuard`], which lets go when dropped.
    theme_playing: Arc<DashSet<GuildId>>,
    /// The tracks that are walk-on themes, mixed over a queue rather than in
    /// it. A theme's end is not the queue moving on: the global track-end
    /// handler leaves these alone.
    theme_tracks: Arc<DashSet<Uuid>>,
    /// Where the CrackCoin token stands.
    #[cfg(feature = "crack-coin")]
    pub coin: crate::coin::registry::CoinRegistry,
}

/// A server's turn to play a walk-on theme. Dropping it ends the turn.
#[must_use = "the turn ends when the guard is dropped"]
#[derive(Debug)]
pub struct ThemeGuard {
    playing: Arc<DashSet<GuildId>>,
    guild: GuildId,
}

impl Drop for ThemeGuard {
    fn drop(&mut self) {
        self.playing.remove(&self.guild);
    }
}

impl EconomyRegistry {
    /// The server's settings; `off` when none were stored or read.
    pub fn settings(&self, g: GuildId) -> EconomySettings {
        self.settings
            .get(&g)
            .map(|s| s.clone())
            .unwrap_or_else(|| EconomySettings::off(g))
    }

    pub fn set_settings(&self, s: EconomySettings) {
        self.settings.insert(s.guild, s);
    }

    pub fn is_enabled(&self, g: GuildId) -> bool {
        self.settings.get(&g).is_some_and(|s| s.enabled)
    }

    /// The server's stored settings have been read.
    pub fn mark_loaded(&self, g: GuildId) {
        self.loaded.insert(g);
    }

    /// Whether the stored settings were read; until then
    /// [`Self::settings`] answers defaults a save must not write over.
    pub fn loaded(&self, g: GuildId) -> bool {
        self.loaded.contains(&g)
    }

    pub fn set_opted_out(&self, g: GuildId, users: HashSet<UserId>) {
        self.opted_out.insert(g, users);
    }

    pub fn is_opted_out(&self, g: GuildId, u: UserId) -> bool {
        self.opted_out.get(&g).is_some_and(|s| s.contains(&u))
    }

    pub fn opted_out(&self, g: GuildId) -> HashSet<UserId> {
        self.opted_out
            .get(&g)
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Servers whose economy is switched on.
    pub fn enabled_guilds(&self) -> Vec<GuildId> {
        self.settings
            .iter()
            .filter(|e| e.enabled)
            .map(|e| *e.key())
            .collect()
    }

    /// Servers with messages counted since the last tick.
    pub fn pending_guilds(&self) -> Vec<GuildId> {
        self.pending_messages.iter().map(|e| *e.key()).collect()
    }

    /// Counts a message for `u` unless one was counted within `cooldown`.
    /// Returns whether it counted (and records `now` when it did).
    pub fn count_message(&self, g: GuildId, u: UserId, now: Instant, cooldown: Duration) -> bool {
        match self.last_counted.entry((g, u)) {
            Entry::Occupied(mut e) => {
                if !cooldown_allows(Some(*e.get()), now, cooldown) {
                    return false;
                }
                e.insert(now);
            },
            Entry::Vacant(e) => {
                e.insert(now);
            },
        }
        *self
            .pending_messages
            .entry(g)
            .or_default()
            .entry(u)
            .or_insert(0) += 1;
        true
    }

    /// What to do about a jailed member out of their jail at `now`
    /// ([`jail_decision`]), recording it: a move back is stamped (and moves
    /// older than `cooldown` are pruned), a re-check is marked so only one is
    /// ever scheduled per member.
    pub fn jail_step(&self, g: GuildId, u: UserId, now: Instant, cooldown: Duration) -> JailMove {
        let key = (g, u);
        let last = self.last_jail_move.get(&key).map(|t| *t);
        let scheduled = self.jail_rechecks.get(&key).is_some_and(|at| *at > now);
        match jail_decision(now, last, scheduled, cooldown) {
            JailMove::MoveNow => {
                self.last_jail_move
                    .retain(|_, t| now.saturating_duration_since(*t) < cooldown);
                self.jail_rechecks.retain(|_, at| *at > now);
                self.last_jail_move.insert(key, now);
                JailMove::MoveNow
            },
            JailMove::ScheduleAt(at) => match self.jail_rechecks.entry(key) {
                // Another event scheduled it first.
                Entry::Occupied(ref e) if *e.get() > now => JailMove::Nothing,
                entry => {
                    entry.insert(at);
                    JailMove::ScheduleAt(at)
                },
            },
            JailMove::Nothing => JailMove::Nothing,
        }
    }

    /// A scheduled jail re-check ran: the next move inside the limit may
    /// schedule another.
    pub fn clear_jail_recheck(&self, g: GuildId, u: UserId) {
        self.jail_rechecks.remove(&(g, u));
    }

    /// How many members' last jail moves are held (for the pruning test).
    #[cfg(test)]
    pub(crate) fn jail_moves_held(&self) -> usize {
        self.last_jail_move.len()
    }

    /// Takes (and clears) the server's counted messages.
    pub fn take_pending_messages(&self, g: GuildId) -> HashMap<UserId, u32> {
        self.pending_messages
            .remove(&g)
            .map(|(_, m)| m)
            .unwrap_or_default()
    }

    /// True the first time only: the caller then spawns the earning tick.
    pub fn start_once(&self) -> bool {
        !self.started.swap(true, Ordering::SeqCst)
    }

    /// Whether the sweeper may try action `id`'s undo at `now`: never failed,
    /// or its backoff has run out.
    pub fn undo_due(&self, id: i64, now: DateTime<Utc>) -> bool {
        self.undo_backoff.get(&id).is_none_or(|e| e.1 <= now)
    }

    /// Action `id`'s undo failed at `now`: wait [`super::effects::undo_backoff`]
    /// before the next try.
    pub fn undo_failed(&self, id: i64, now: DateTime<Utc>) {
        let mut e = self.undo_backoff.entry(id).or_insert((0, now));
        e.0 += 1;
        e.1 = now + super::effects::undo_backoff(e.0);
    }

    /// Action `id` is settled (ended, or no longer due): forget its backoff.
    pub fn undo_settled(&self, id: i64) {
        self.undo_backoff.remove(&id);
    }

    /// Forget the backoff of every action not in `due`.
    pub fn keep_undo_backoff(&self, due: &HashSet<i64>) {
        self.undo_backoff.retain(|id, _| due.contains(id));
    }

    /// Record when this process started (the database's `now()`). The first
    /// record stands.
    pub fn set_process_start(&self, at: DateTime<Utc>) {
        let _ = self.process_start.set(at);
    }

    /// When this process started, if it was recorded.
    pub fn process_start(&self) -> Option<DateTime<Utc>> {
        self.process_start.get().copied()
    }

    /// True the first time only: the caller then spawns the sweeper.
    pub fn start_sweeper_once(&self) -> bool {
        !self.sweeper_started.swap(true, Ordering::SeqCst)
    }

    /// The server's turn to play a walk-on theme; `None` while another one
    /// plays there.
    pub fn try_theme_guard(&self, g: GuildId) -> Option<ThemeGuard> {
        self.theme_playing.insert(g).then(|| ThemeGuard {
            playing: self.theme_playing.clone(),
            guild: g,
        })
    }

    /// Mark `track` as a walk-on theme, before it can play.
    pub fn add_theme_track(&self, track: Uuid) {
        self.theme_tracks.insert(track);
    }

    /// Whether `track` is a walk-on theme. If it is, it is forgotten: a
    /// track ends once.
    pub fn take_theme_track(&self, track: Uuid) -> bool {
        self.theme_tracks.remove(&track).is_some()
    }

    /// Whether `track` is a walk-on theme still remembered.
    pub fn is_theme_track(&self, track: Uuid) -> bool {
        self.theme_tracks.contains(&track)
    }

    pub fn set_one_opted_out(&self, g: GuildId, u: UserId, out: bool) {
        let mut set = self.opted_out.entry(g).or_default();
        if out {
            set.insert(u);
        } else {
            set.remove(&u);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = GuildId::new(1);

    #[test]
    fn opt_out_set_tracks_one_member_at_a_time() {
        let reg = EconomyRegistry::default();
        let (a, b) = (UserId::new(2), UserId::new(3));
        assert!(!reg.is_opted_out(G, a));
        reg.set_one_opted_out(G, a, true);
        reg.set_one_opted_out(G, b, true);
        reg.set_one_opted_out(G, b, false);
        assert!(reg.is_opted_out(G, a));
        assert!(!reg.is_opted_out(G, b));
        assert!(!reg.is_opted_out(GuildId::new(9), a), "per server");
        reg.set_opted_out(G, HashSet::from([b]));
        assert_eq!(reg.opted_out(G), HashSet::from([b]));
    }

    #[tokio::test(start_paused = true)]
    async fn count_message_respects_cooldown_and_take_drains() {
        let reg = EconomyRegistry::default();
        let (a, b) = (UserId::new(2), UserId::new(3));
        let cd = Duration::from_secs(60);
        assert!(reg.count_message(G, a, Instant::now(), cd));
        assert!(
            !reg.count_message(G, a, Instant::now(), cd),
            "inside cooldown"
        );
        assert!(reg.count_message(G, b, Instant::now(), cd), "per member");
        assert!(
            reg.count_message(GuildId::new(9), a, Instant::now(), cd),
            "per server"
        );
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(!reg.count_message(G, a, Instant::now(), cd));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(reg.count_message(G, a, Instant::now(), cd), "cooldown over");
        tokio::time::advance(Duration::from_secs(59)).await;
        assert!(
            !reg.count_message(G, a, Instant::now(), cd),
            "the cooldown restarts from the last COUNTED message"
        );
        let taken = reg.take_pending_messages(G);
        assert_eq!(taken, HashMap::from([(a, 2), (b, 1)]));
        assert!(reg.take_pending_messages(G).is_empty(), "drained");
        assert_eq!(reg.take_pending_messages(GuildId::new(9)).len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn moving_twice_within_10s_schedules_one_recheck() {
        let reg = EconomyRegistry::default();
        let (a, b) = (UserId::new(2), UserId::new(3));
        let cd = Duration::from_secs(10);
        let t = Instant::now();
        assert_eq!(reg.jail_step(G, a, t, cd), JailMove::MoveNow);
        // Out again a second later: one re-check, at the limit's end.
        let s = Duration::from_secs;
        assert_eq!(
            reg.jail_step(G, a, t + s(1), cd),
            JailMove::ScheduleAt(t + cd)
        );
        // A third move inside the limit adds nothing: one is enough.
        assert_eq!(reg.jail_step(G, a, t + s(2), cd), JailMove::Nothing);
        // Another member's clock is their own.
        assert_eq!(reg.jail_step(G, b, t + s(2), cd), JailMove::MoveNow);
        // The re-check ran: the next early move may schedule again.
        reg.clear_jail_recheck(G, a);
        assert_eq!(
            reg.jail_step(G, a, t + s(3), cd),
            JailMove::ScheduleAt(t + cd)
        );
        // At the limit's end it moves.
        assert_eq!(reg.jail_step(G, a, t + cd, cd), JailMove::MoveNow);
    }

    #[tokio::test(start_paused = true)]
    async fn old_jail_moves_are_pruned_when_one_is_stamped() {
        let reg = EconomyRegistry::default();
        let cd = Duration::from_secs(10);
        let t = Instant::now();
        for n in 1..=3 {
            reg.jail_step(G, UserId::new(n), t, cd);
        }
        assert_eq!(reg.jail_moves_held(), 3);
        // Twenty seconds on, one new move: the three old ones go.
        reg.jail_step(G, UserId::new(9), t + Duration::from_secs(20), cd);
        assert_eq!(reg.jail_moves_held(), 1);
    }

    #[test]
    fn start_once_is_true_once() {
        let reg = EconomyRegistry::default();
        assert!(reg.start_once());
        assert!(!reg.start_once());
        assert!(!reg.clone().start_once(), "clones share the flag");
    }

    #[test]
    fn start_sweeper_once_is_true_once_and_apart_from_the_earner() {
        let reg = EconomyRegistry::default();
        assert!(reg.start_sweeper_once());
        assert!(!reg.start_sweeper_once());
        assert!(reg.start_once(), "its own flag");
    }

    #[test]
    fn one_theme_plays_per_server_until_its_guard_drops() {
        let reg = EconomyRegistry::default();
        let other = GuildId::new(9);
        let held = reg.try_theme_guard(G).expect("free");
        assert!(reg.try_theme_guard(G).is_none(), "taken");
        let elsewhere = reg.try_theme_guard(other).expect("per server");
        drop(held);
        assert!(reg.try_theme_guard(G).is_some(), "released on drop");
        assert!(reg.try_theme_guard(other).is_none(), "still held");
        drop(elsewhere);
        assert!(reg.try_theme_guard(other).is_some());
    }

    #[test]
    fn theme_tracks_are_remembered_until_taken() {
        let reg = EconomyRegistry::default();
        let (a, b) = (Uuid::from_u128(1), Uuid::from_u128(2));
        reg.add_theme_track(a);
        assert!(reg.is_theme_track(a));
        assert!(!reg.is_theme_track(b));
        assert!(!reg.take_theme_track(b), "not a theme");
        assert!(reg.take_theme_track(a));
        assert!(!reg.is_theme_track(a), "taken once");
        assert!(!reg.take_theme_track(a));
    }

    #[test]
    fn unloaded_server_reads_off_and_not_loaded() {
        let reg = EconomyRegistry::default();
        assert!(!reg.loaded(G));
        assert!(!reg.is_enabled(G));
        reg.mark_loaded(G);
        assert!(reg.loaded(G));
    }
}
