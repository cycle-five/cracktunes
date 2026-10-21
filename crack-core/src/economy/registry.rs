//! The economy's in-memory state, on `Data`: per-server settings, which
//! servers' stored settings have been read, and who is opted out.

use super::earn::cooldown_allows;
use super::settings::EconomySettings;
use dashmap::{mapref::entry::Entry, DashMap, DashSet};
use serenity::all::{GuildId, UserId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct EconomyRegistry {
    settings: DashMap<GuildId, EconomySettings>,
    loaded: DashSet<GuildId>,
    opted_out: DashMap<GuildId, HashSet<UserId>>,
    /// Messages counted since the last tick, per server and member.
    pending_messages: DashMap<GuildId, HashMap<UserId, u32>>,
    /// When each member's last message was counted (the cooldown's clock).
    last_counted: DashMap<(GuildId, UserId), Instant>,
    /// The earning tick has been spawned.
    started: Arc<AtomicBool>,
    sweeper_started: Arc<AtomicBool>,
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

    /// True the first time only: the caller then spawns the sweeper.
    pub fn start_sweeper_once(&self) -> bool {
        !self.sweeper_started.swap(true, Ordering::SeqCst)
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
    fn unloaded_server_reads_off_and_not_loaded() {
        let reg = EconomyRegistry::default();
        assert!(!reg.loaded(G));
        assert!(!reg.is_enabled(G));
        reg.mark_loaded(G);
        assert!(reg.loaded(G));
    }
}
