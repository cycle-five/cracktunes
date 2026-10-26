//! Per-server settings for the mod log.

use serenity::all::{ChannelId, GuildId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModLogSettings {
    pub guild: GuildId,
    pub enabled: bool,
    pub channel: Option<ChannelId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLogSettingsError {
    NoDatabase,
    NoChannel,
}

impl ModLogSettings {
    /// The settings of a server that never configured the mod log.
    pub fn off(guild: GuildId) -> Self {
        Self {
            guild,
            enabled: false,
            channel: None,
        }
    }

    /// Check a config before saving it. Enabling needs a database and a
    /// channel.
    pub fn validate(&self, has_db: bool) -> Result<(), ModLogSettingsError> {
        if self.enabled {
            if !has_db {
                return Err(ModLogSettingsError::NoDatabase);
            }
            if self.channel.is_none() {
                return Err(ModLogSettingsError::NoChannel);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_refuses_enabling_without_db_or_channel() {
        let on = ModLogSettings {
            enabled: true,
            channel: Some(ChannelId::new(5)),
            ..ModLogSettings::off(GuildId::new(1))
        };
        let no_channel = ModLogSettings {
            channel: None,
            ..on.clone()
        };
        assert_eq!(
            no_channel.validate(true),
            Err(ModLogSettingsError::NoChannel)
        );
        assert_eq!(on.validate(false), Err(ModLogSettingsError::NoDatabase));
        assert_eq!(on.validate(true), Ok(()));
        // Disabled validates with neither.
        let off = ModLogSettings::off(GuildId::new(1));
        assert_eq!(off.validate(false), Ok(()));
        assert_eq!(off.validate(true), Ok(()));
    }
}
