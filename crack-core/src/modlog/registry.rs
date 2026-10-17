//! The mod log's in-memory state, on `Data`: per-server settings, which
//! servers' stored settings have been read, and for servers with the log on,
//! the voice changes waiting for an audit fetch, the audit snapshots, and the
//! fetch suspension (see `modlog::batch`).
//!
//! Times are [`tokio::time::Instant`]s, so the debounce runs on tokio's clock
//! (and a paused test clock).

use super::attribution::{AuditKind, Change, Snapshots};
use super::settings::ModLogSettings;
use dashmap::{DashMap, DashSet};
use serenity::all::{GuildId, UserId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::task::AbortHandle;
use tokio::time::Instant;

/// How long a disconnect the bot is about to make itself (a room kick) is
/// held, so its voice leave is not attributed.
pub const EXPECT_FOR: Duration = Duration::from_secs(10);

/// Tells one life of a server's batch state from the next: a fetch that
/// started before the state was forgotten must not write into a new one.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// One enabled server's attribution state. Only `modlog::batch` touches it.
#[derive(Debug, Clone)]
pub(super) struct GuildBatch {
    pub(super) epoch: u64,
    /// Voice changes waiting for the debounce, each with when it arrived.
    pub(super) pending: Vec<(Change, Instant)>,
    /// The armed debounce, if any.
    pub(super) debounce: Option<AbortHandle>,
    /// A fetch (flush or seed) is running; at most one per server.
    pub(super) in_flight: bool,
    pub(super) snaps: Snapshots,
    /// Kinds whose snapshot must be re-read before anything is attributed
    /// from them again: after a failed fetch (M15) or a suspension (M8b).
    pub(super) reseed: Vec<AuditKind>,
    /// A 403 paused fetching until then.
    pub(super) suspended_until: Option<Instant>,
}

impl GuildBatch {
    fn new() -> Self {
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            pending: Vec::new(),
            debounce: None,
            in_flight: false,
            snaps: Snapshots::default(),
            reseed: Vec::new(),
            suspended_until: None,
        }
    }

    pub(super) fn suspended(&self, now: Instant) -> bool {
        self.suspended_until.is_some_and(|until| now < until)
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

    pub fn set_settings(&self, s: ModLogSettings) {
        self.settings.insert(s.guild, s);
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

    /// Lift a fetch suspension (a settings save). The snapshots stay marked
    /// for a re-read, so nothing is diffed against the stale ones.
    pub fn clear_suspension(&self, g: GuildId) {
        if let Some(mut b) = self.batches.get_mut(&g) {
            b.suspended_until = None;
        }
    }

    /// Drop everything held for the server but its settings: pending
    /// changes, the armed debounce, snapshots, the suspension and expected
    /// disconnects. A fetch already running finds its state gone and does
    /// nothing with what it read.
    pub fn forget(&self, g: GuildId) {
        if let Some((_, b)) = self.batches.remove(&g) {
            if let Some(h) = b.debounce {
                h.abort();
            }
        }
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
        if !self.is_enabled(g) {
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

    /// The bot's disconnect of `u` failed: no leave is coming.
    pub fn clear_expected(&self, g: GuildId, u: UserId) {
        self.expected.remove(&(g, u));
    }

    /// The server's snapshots, if any state is held.
    pub fn snapshots(&self, g: GuildId) -> Option<Snapshots> {
        self.batches.get(&g).map(|b| b.snaps.clone())
    }

    /// Run `f` on the server's batch state, creating it. Callers check the
    /// server is enabled first.
    pub(super) fn batch<R>(&self, g: GuildId, f: impl FnOnce(&mut GuildBatch) -> R) -> R {
        let mut b = self.batches.entry(g).or_insert_with(GuildBatch::new);
        f(&mut b)
    }

    /// Run `f` on the server's batch state if it exists (and, with `epoch`,
    /// only if it is that life of it).
    pub(super) fn batch_existing<R>(
        &self,
        g: GuildId,
        epoch: Option<u64>,
        f: impl FnOnce(&mut GuildBatch) -> R,
    ) -> Option<R> {
        let mut b = self.batches.get_mut(&g)?;
        if epoch.is_some_and(|e| e != b.epoch) {
            return None;
        }
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
    async fn clear_expected_drops_the_marker() {
        let reg = enabled();
        let t0 = Instant::now();
        reg.expect_disconnect(G, U, t0);
        reg.clear_expected(G, U);
        assert!(!reg.take_expected(G, U, t0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_disabled_server_holds_no_expected_disconnect() {
        let reg = ModLogRegistry::default();
        reg.expect_disconnect(G, U, Instant::now());
        assert!(!reg.has_state(G));
        assert!(!reg.take_expected(G, U, Instant::now()));
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
