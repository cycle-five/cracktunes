//! The mod log's in-memory state, on `RoomsData`: per-server settings, which
//! servers' stored settings have been read, and for servers with the log on,
//! the voice changes waiting for an audit fetch, the audit snapshots, and the
//! fetch suspension (see `modlog::batch`).
//!
//! Times are [`tokio::time::Instant`]s, so the debounce runs on tokio's clock
//! (and a paused test clock).

use super::attribution::{AuditKind, Change, Snapshots};
use super::batch::SKEW;
use super::settings::ModLogSettings;
use dashmap::{mapref::entry::Entry, DashMap, DashSet};
use serenity::all::{GuildId, UserId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::task::AbortHandle;
use tokio::time::Instant;

/// How long a disconnect the bot is about to make itself (a room kick) is
/// held, so its voice leave is not attributed.
pub const EXPECT_FOR: Duration = Duration::from_secs(10);

/// A settings save resumes a server's audit fetching (lifts a 403 pause,
/// outdates running fetches, seeds) at most this often (M26).
pub const RESUME_EVERY: Duration = Duration::from_secs(60);

/// Tells one life of a server's batch state from the next: a fetch that
/// started before the state was forgotten, or before a settings save, must
/// not write into what came after.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Names the fetch that holds a server's fetch slot.
static NEXT_FETCH: AtomicU64 = AtomicU64::new(1);

/// A fresh token for [`GuildBatch::in_flight`].
pub(super) fn next_fetch() -> u64 {
    NEXT_FETCH.fetch_add(1, Ordering::Relaxed)
}

/// One enabled server's attribution state. Only `modlog::batch` touches it.
#[derive(Debug, Clone)]
pub(super) struct GuildBatch {
    pub(super) epoch: u64,
    /// Voice changes waiting for the debounce, each with when it arrived.
    pub(super) pending: Vec<(Change, Instant)>,
    /// The armed debounce, if any.
    pub(super) debounce: Option<AbortHandle>,
    /// The fetch (flush or seed) running, by token; at most one per server.
    /// Only that fetch frees it, whatever became of the epoch meanwhile.
    pub(super) in_flight: Option<u64>,
    pub(super) snaps: Snapshots,
    /// Kinds whose snapshot must be re-read before anything is attributed
    /// from them again, each with the earliest fetch start whose read
    /// clears the mark: after a failed fetch (M15) or a suspension (M8b),
    /// any later read; after a room kick's leave turned out not to be the
    /// bot's (M28c), a read from [`SKEW`] on. See [`Self::mark_reread`].
    pub(super) reseed: Vec<(AuditKind, Instant)>,
    /// A 403 paused fetching until then.
    pub(super) suspended_until: Option<Instant>,
}

impl GuildBatch {
    fn new() -> Self {
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            pending: Vec::new(),
            debounce: None,
            in_flight: None,
            snaps: Snapshots::default(),
            reseed: Vec::new(),
            suspended_until: None,
        }
    }

    pub(super) fn suspended(&self, now: Instant) -> bool {
        self.suspended_until.is_some_and(|until| now < until)
    }

    /// Mark `kind` for a re-read that only a fetch started at or after
    /// `from` satisfies. Marked already, the later instant wins: a mark is
    /// never cleared by a read that started before it was set.
    pub(super) fn mark_reread(&mut self, kind: AuditKind, from: Instant) {
        match self.reseed.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, at)) => *at = (*at).max(from),
            None => self.reseed.push((kind, from)),
        }
    }

    /// Whether `kind` awaits a re-read.
    pub(super) fn awaits_reread(&self, kind: AuditKind) -> bool {
        self.reseed.iter().any(|(k, _)| *k == kind)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModLogRegistry {
    settings: DashMap<GuildId, ModLogSettings>,
    /// Servers whose stored settings have been read (or found absent).
    loaded: DashSet<GuildId>,
    /// Attribution state, for enabled servers only.
    batches: DashMap<GuildId, GuildBatch>,
    /// Members the bot is disconnecting itself, since when.
    expected: DashMap<(GuildId, UserId), Instant>,
    /// When a save last resumed each server's fetching (M26). Kept when the
    /// log is turned off, so off and on again cannot fetch more often.
    resumed: DashMap<GuildId, Instant>,
    /// Servers a save resumed that have not had their seed started yet.
    seed_owed: DashSet<GuildId>,
}

impl ModLogRegistry {
    /// The server's settings; off when it never configured them.
    pub fn settings(&self, g: GuildId) -> ModLogSettings {
        self.settings
            .get(&g)
            .map(|s| s.clone())
            .unwrap_or_else(|| ModLogSettings::off(g))
    }

    pub fn is_enabled(&self, g: GuildId) -> bool {
        self.settings.get(&g).is_some_and(|s| s.enabled)
    }

    /// Store the server's settings. With the log off, everything else held
    /// for the server goes (only enabled servers hold any state).
    pub fn set_settings(&self, s: ModLogSettings) {
        let (g, enabled) = (s.guild, s.enabled);
        self.settings.insert(g, s);
        if !enabled {
            self.forget(g);
        }
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

    /// Whether a 403 has paused audit fetches for the server at `now`.
    pub fn is_suspended(&self, g: GuildId, now: Instant) -> bool {
        self.batches.get(&g).is_some_and(|b| b.suspended(now))
    }

    /// The settings were saved with the log on: resume the server's audit
    /// fetching, at most once per [`RESUME_EVERY`] (M26). Resuming lifts a
    /// fetch suspension, starts a new epoch so a fetch that began before the
    /// save (a 403 included) writes nothing after it, and owes the server a
    /// seed ([`Self::take_seed_owed`]). Inside the window it changes nothing
    /// and answers false. It marks nothing for a re-read: a suspension has
    /// already marked both kinds, and snapshots read before a save stay good.
    pub fn settings_saved(&self, g: GuildId, now: Instant) -> bool {
        match self.resumed.entry(g) {
            Entry::Occupied(last) if now.saturating_duration_since(*last.get()) < RESUME_EVERY => {
                tracing::debug!(
                    "modlog: {g}: saved, but fetching was resumed less than {} s ago",
                    RESUME_EVERY.as_secs()
                );
                return false;
            },
            Entry::Occupied(mut last) => {
                last.insert(now);
            },
            Entry::Vacant(first) => {
                first.insert(now);
            },
        }
        if let Some(mut b) = self.batches.get_mut(&g) {
            b.suspended_until = None;
            b.epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);
        }
        self.seed_owed.insert(g);
        true
    }

    /// Consume the seed a resuming save owes the server: true once per
    /// [`Self::settings_saved`] that resumed it.
    pub fn take_seed_owed(&self, g: GuildId) -> bool {
        self.seed_owed.remove(&g).is_some()
    }

    /// Drop everything held for the server but its settings (and when a save
    /// last resumed it): pending changes, the armed debounce, snapshots, the
    /// suspension, an owed seed and expected disconnects. A fetch already
    /// running finds its state gone and does nothing with what it read.
    pub fn forget(&self, g: GuildId) {
        if let Some((_, b)) = self.batches.remove(&g) {
            if let Some(h) = b.debounce {
                h.abort();
            }
        }
        self.seed_owed.remove(&g);
        self.expected.retain(|(eg, _), _| *eg != g);
    }

    /// Whether anything but settings is held for the server.
    pub fn has_state(&self, g: GuildId) -> bool {
        self.batches.contains_key(&g) || self.expected.iter().any(|e| e.key().0 == g)
    }

    /// The bot is about to disconnect `u` itself: the leave that follows
    /// within [`EXPECT_FOR`] is not attributed. Nothing is held for a server
    /// with the log off.
    pub fn expect_disconnect(&self, g: GuildId, u: UserId, now: Instant) {
        // Held while the marker is stored, so turning the log off waits for
        // it and then forgets it (see `batch_if_enabled`).
        let Some(settings) = self.settings.get(&g) else {
            return;
        };
        if !settings.enabled {
            return;
        }
        self.expected
            .retain(|_, at| now.saturating_duration_since(*at) < EXPECT_FOR);
        self.expected.insert((g, u), now);
    }

    /// Consume `u`'s expected disconnect: true when one was held and has not
    /// expired at `now`. An expired one is dropped too.
    pub fn take_expected(&self, g: GuildId, u: UserId, now: Instant) -> bool {
        self.expected
            .remove(&(g, u))
            .is_some_and(|(_, at)| now.saturating_duration_since(at) < EXPECT_FOR)
    }

    /// The bot's disconnect of `u` failed: no leave is coming. True when a
    /// marker was still held; false when none was, and so a leave of `u` has
    /// already taken it for the bot's own (or the log is off).
    pub fn clear_expected(&self, g: GuildId, u: UserId) -> bool {
        self.expected.remove(&(g, u)).is_some()
    }

    /// The server's snapshot of `kind` may be missing a unit that no pending
    /// change will explain (a room kick's leave turned out to be a
    /// moderator's: M28c). The next flush re-reads `kind`, attributes
    /// nothing of it, and absorbs what it reads, rather than leave that
    /// unit for whoever leaves next. Only a fetch started [`SKEW`] after
    /// `now` clears the mark: the unit is written by then. A fetch already
    /// running attributes nothing of `kind` either. Nothing is held for a
    /// server with the log off.
    pub fn mark_reread(&self, g: GuildId, kind: AuditKind, now: Instant) {
        if self
            .batch_if_enabled(g, |b| b.mark_reread(kind, now + SKEW))
            .is_some()
        {
            tracing::debug!(
                "modlog: {g}: {kind:?} may hold a unit no change explains; it is re-read before \
                 anything of it is attributed"
            );
        }
    }

    /// `GuildCreate` fired for a server that already holds a batch life: the
    /// gateway re-identified without a restart. Whatever moderators did
    /// while the bot was away left audit units whose voice events it never
    /// saw, so both kinds are marked for a re-read (see [`Self::mark_reread`]):
    /// the next flush attributes nothing of them and absorbs what it reads.
    /// A server with no life is untouched (its first flush seeds it). True
    /// when a life was marked.
    pub fn mark_reconnected(&self, g: GuildId, now: Instant) -> bool {
        let marked = self
            .batch_existing(g, |b| {
                b.mark_reread(AuditKind::Disconnect, now + SKEW);
                b.mark_reread(AuditKind::Move, now + SKEW);
            })
            .is_some();
        if marked {
            tracing::debug!(
                "modlog: {g}: reconnected; both kinds are re-read before anything is attributed"
            );
        }
        marked
    }

    /// The server's snapshots, if any state is held.
    pub fn snapshots(&self, g: GuildId) -> Option<Snapshots> {
        self.batches.get(&g).map(|b| b.snaps.clone())
    }

    /// Run `f` on the server's batch state, creating it, if the server has
    /// the log on; `None` (and nothing held) when it is off. The settings
    /// entry is held throughout, so a `set_settings` turning the log off
    /// waits until the state exists, then forgets it. `f` must not touch the
    /// registry.
    pub(super) fn batch_if_enabled<R>(
        &self,
        g: GuildId,
        f: impl FnOnce(&mut GuildBatch) -> R,
    ) -> Option<R> {
        let settings = self.settings.get(&g)?;
        if !settings.enabled {
            return None;
        }
        let mut b = self.batches.entry(g).or_insert_with(GuildBatch::new);
        Some(f(&mut b))
    }

    /// Run `f` on the server's batch state if it exists.
    pub(super) fn batch_existing<R>(
        &self,
        g: GuildId,
        f: impl FnOnce(&mut GuildBatch) -> R,
    ) -> Option<R> {
        let mut b = self.batches.get_mut(&g)?;
        Some(f(&mut b))
    }

    /// Store the server's armed debounce, aborting the one it replaces.
    /// False (and nothing stored) when no state is held for the server.
    pub(super) fn set_debounce(&self, g: GuildId, handle: AbortHandle) -> bool {
        let Some(mut b) = self.batches.get_mut(&g) else {
            return false;
        };
        if let Some(old) = b.debounce.replace(handle) {
            old.abort();
        }
        true
    }

    /// Whether a debounce is armed and has not fired yet.
    pub(super) fn debounce_armed(&self, g: GuildId) -> bool {
        self.batches
            .get(&g)
            .is_some_and(|b| b.debounce.as_ref().is_some_and(|h| !h.is_finished()))
    }

    /// A debounce task taking its own handle out, unaborted, before it
    /// flushes: a later arm then cannot abort the flush mid-way.
    pub(super) fn release_debounce(&self, g: GuildId, id: tokio::task::Id) {
        if let Some(mut b) = self.batches.get_mut(&g) {
            if b.debounce.as_ref().is_some_and(|h| h.id() == id) {
                b.debounce = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(2);

    fn enabled() -> ModLogRegistry {
        let reg = ModLogRegistry::default();
        reg.set_settings(ModLogSettings {
            enabled: true,
            channel: Some(serenity::all::ChannelId::new(3)),
            ..ModLogSettings::off(G)
        });
        reg
    }

    #[tokio::test(start_paused = true)]
    async fn an_expected_disconnect_holds_for_ten_seconds() {
        let reg = enabled();
        let t0 = Instant::now();
        reg.expect_disconnect(G, U, t0);
        assert!(reg.take_expected(G, U, t0 + Duration::from_millis(9_999)));
        assert!(!reg.take_expected(G, U, t0), "consumed");
        reg.expect_disconnect(G, U, t0);
        assert!(!reg.take_expected(G, U, t0 + EXPECT_FOR), "expired");
        assert!(!reg.has_state(G), "an expired marker is dropped too");
    }

    #[tokio::test(start_paused = true)]
    async fn clear_expected_drops_the_marker_and_says_whether_one_was_held() {
        let reg = enabled();
        let t0 = Instant::now();
        reg.expect_disconnect(G, U, t0);
        assert!(reg.clear_expected(G, U), "held");
        assert!(!reg.take_expected(G, U, t0));
        reg.expect_disconnect(G, U, t0);
        assert!(reg.take_expected(G, U, t0), "a leave took it");
        assert!(!reg.clear_expected(G, U), "taken already (M28c)");
        assert!(!reg.clear_expected(G, UserId::new(4)), "never held");
    }

    /// M28c: a mark asks for a read started SKEW after it; marking again
    /// keeps the later instant; a server with the log off holds nothing.
    #[tokio::test(start_paused = true)]
    async fn mark_reread_asks_for_a_read_skew_after_it() {
        let reg = ModLogRegistry::default();
        let t0 = Instant::now();
        reg.mark_reread(G, AuditKind::Disconnect, t0);
        assert!(!reg.has_state(G), "never configured");
        let reg = enabled();
        reg.batch_if_enabled(G, |b| b.mark_reread(AuditKind::Move, t0));
        reg.mark_reread(G, AuditKind::Disconnect, t0);
        let marks = |reg: &ModLogRegistry| reg.batch_existing(G, |b| b.reseed.clone());
        assert_eq!(
            marks(&reg),
            Some(vec![
                (AuditKind::Move, t0),
                (AuditKind::Disconnect, t0 + SKEW)
            ])
        );
        reg.batch_existing(G, |b| b.mark_reread(AuditKind::Disconnect, t0));
        assert_eq!(
            marks(&reg).unwrap()[1],
            (AuditKind::Disconnect, t0 + SKEW),
            "an earlier instant does not shorten a mark"
        );
        reg.mark_reread(G, AuditKind::Disconnect, t0 + SKEW);
        assert_eq!(
            marks(&reg).unwrap()[1],
            (AuditKind::Disconnect, t0 + SKEW * 2),
            "a later one lengthens it"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_disabled_server_holds_no_expected_disconnect() {
        let reg = ModLogRegistry::default();
        reg.expect_disconnect(G, U, Instant::now());
        assert!(!reg.has_state(G), "never configured");
        reg.set_settings(ModLogSettings::off(G));
        reg.expect_disconnect(G, U, Instant::now());
        assert!(!reg.has_state(G), "configured off");
        assert!(!reg.take_expected(G, U, Instant::now()));
    }

    /// M26: a save resumes fetching at most once a minute per server, and
    /// turning the log off and on again does not start the minute over.
    #[tokio::test(start_paused = true)]
    async fn a_save_resumes_fetching_at_most_once_a_minute() {
        let reg = enabled();
        let t0 = Instant::now();
        let pause = |reg: &ModLogRegistry| {
            reg.batch_if_enabled(G, |b| {
                b.suspended_until = Some(t0 + Duration::from_secs(600));
            })
        };
        let epoch = |reg: &ModLogRegistry| reg.batch_existing(G, |b| b.epoch);
        pause(&reg);
        let before = epoch(&reg);
        assert!(reg.settings_saved(G, t0));
        assert!(!reg.is_suspended(G, t0), "lifted");
        assert_ne!(epoch(&reg), before, "older fetches are void");
        assert!(reg.take_seed_owed(G), "a seed is owed");
        assert!(!reg.take_seed_owed(G), "one seed");

        pause(&reg);
        let before = epoch(&reg);
        let almost = t0 + RESUME_EVERY - Duration::from_millis(1);
        assert!(!reg.settings_saved(G, almost));
        assert!(reg.is_suspended(G, almost), "still paused");
        assert_eq!(epoch(&reg), before, "nothing outdated");
        assert!(!reg.take_seed_owed(G), "no seed");
        let on = reg.settings(G);
        reg.set_settings(ModLogSettings::off(G));
        reg.set_settings(on);
        assert!(!reg.settings_saved(G, almost), "off and on again");
        assert!(!reg.take_seed_owed(G));
        let other = GuildId::new(5);
        reg.set_settings(ModLogSettings {
            enabled: true,
            channel: Some(serenity::all::ChannelId::new(3)),
            ..ModLogSettings::off(other)
        });
        assert!(reg.settings_saved(other, almost), "another server's minute");

        // A minute after the last resume, a save resumes again.
        pause(&reg);
        assert!(reg.settings_saved(G, t0 + RESUME_EVERY));
        assert!(!reg.is_suspended(G, t0 + RESUME_EVERY));
        assert!(reg.take_seed_owed(G));
    }

    #[tokio::test(start_paused = true)]
    async fn expired_markers_are_pruned_on_the_next_expect() {
        let reg = enabled();
        let t0 = Instant::now();
        reg.expect_disconnect(G, U, t0);
        reg.expect_disconnect(G, UserId::new(4), t0 + EXPECT_FOR);
        assert!(!reg.expected.contains_key(&(G, U)));
    }
}
