//! The mod log's in-memory state, on `Data`: per-server settings and which
//! servers' stored settings have been read.

use super::settings::ModLogSettings;
use dashmap::{DashMap, DashSet};
use serenity::all::GuildId;

#[derive(Debug, Clone, Default)]
pub struct ModLogRegistry {
    settings: DashMap<GuildId, ModLogSettings>,
    /// Servers whose stored settings have been read (or found absent).
    loaded: DashSet<GuildId>,
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
}
