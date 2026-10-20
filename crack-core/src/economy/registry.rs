//! The economy's in-memory state, on `Data`: per-server settings, which
//! servers' stored settings have been read, and who is opted out.

use super::settings::EconomySettings;
use dashmap::{DashMap, DashSet};
use serenity::all::{GuildId, UserId};
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct EconomyRegistry {
    settings: DashMap<GuildId, EconomySettings>,
    loaded: DashSet<GuildId>,
    opted_out: DashMap<GuildId, HashSet<UserId>>,
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

    #[test]
    fn unloaded_server_reads_off_and_not_loaded() {
        let reg = EconomyRegistry::default();
        assert!(!reg.loaded(G));
        assert!(!reg.is_enabled(G));
        reg.mark_loaded(G);
        assert!(reg.loaded(G));
    }
}
