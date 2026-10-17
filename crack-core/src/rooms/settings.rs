//! Per-server settings for temporary voice rooms.

use crate::messaging::messages::ROOM_NAME_FALLBACK;
use serenity::all::{ChannelId, GuildId};

/// The empty-room grace periods a server can pick, in seconds.
pub const GRACE_CHOICES: [u16; 6] = [0, 10, 30, 60, 120, 300];
pub const DEFAULT_TEMPLATE: &str = "{name}'s room";
/// Discord's channel name limit, also the cap for templates.
pub const ROOM_NAME_MAX: usize = 100;
const DEFAULT_GRACE_SECS: u16 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomSettings {
    pub guild: GuildId,
    pub enabled: bool,
    pub hub: Option<ChannelId>,
    pub name_template: String,
    /// 0 means no limit.
    pub default_limit: u8,
    pub empty_grace_secs: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    NoDatabase,
    NoHub,
    BadTemplate,
    BadLimit,
    BadGrace,
}

impl RoomSettings {
    /// The settings of a server that never configured rooms.
    pub fn off(guild: GuildId) -> Self {
        Self {
            guild,
            enabled: false,
            hub: None,
            name_template: DEFAULT_TEMPLATE.to_string(),
            default_limit: 0,
            empty_grace_secs: DEFAULT_GRACE_SECS,
        }
    }

    /// Check a config before saving it. `has_db` is whether the bot has a
    /// database; enabling needs one, and a hub.
    pub fn validate(&self, has_db: bool) -> Result<(), SettingsError> {
        if self.enabled {
            if !has_db {
                return Err(SettingsError::NoDatabase);
            }
            if self.hub.is_none() {
                return Err(SettingsError::NoHub);
            }
        }
        let template = self.name_template.trim();
        if template.is_empty()
            || template.chars().count() > ROOM_NAME_MAX
            || template.chars().any(char::is_control)
        {
            return Err(SettingsError::BadTemplate);
        }
        if self.default_limit > 99 {
            return Err(SettingsError::BadLimit);
        }
        if !GRACE_CHOICES.contains(&self.empty_grace_secs) {
            return Err(SettingsError::BadGrace);
        }
        Ok(())
    }
}

/// Fill `{name}` in the template, drop control characters, trim and cap at
/// [`ROOM_NAME_MAX`] characters. Never empty.
pub fn render_room_name(template: &str, display_name: &str) -> String {
    let name: String = template
        .replace("{name}", display_name)
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(ROOM_NAME_MAX)
        .collect();
    let name = name.trim().to_string();
    if name.is_empty() {
        ROOM_NAME_FALLBACK.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> RoomSettings {
        RoomSettings {
            enabled: true,
            hub: Some(ChannelId::new(5)),
            ..RoomSettings::off(GuildId::new(1))
        }
    }

    #[test]
    fn render_room_name_fills_the_template() {
        assert_eq!(render_room_name("{name}'s room", "Saucey"), "Saucey's room");
    }

    #[test]
    fn render_room_name_caps_at_100_chars_on_a_char_boundary() {
        let long = "é".repeat(300);
        assert_eq!(render_room_name("{name}", &long).chars().count(), 100);
    }

    #[test]
    fn render_room_name_strips_control_chars_and_never_returns_empty() {
        assert_eq!(render_room_name("{name}", "\u{0}\n\t"), "Room");
    }

    #[test]
    fn render_room_name_keeps_mentions_as_text() {
        assert_eq!(render_room_name("{name}", "@everyone"), "@everyone");
    }

    #[test]
    fn validate_refuses_enabling_without_db_or_hub() {
        let no_hub = RoomSettings { hub: None, ..on() };
        assert_eq!(no_hub.validate(true), Err(SettingsError::NoHub));
        assert_eq!(on().validate(false), Err(SettingsError::NoDatabase));
        assert_eq!(on().validate(true), Ok(()));
        let off = RoomSettings::off(GuildId::new(1));
        assert_eq!(off.validate(false), Ok(()));
    }

    #[test]
    fn validate_refuses_out_of_range_values() {
        let base = RoomSettings::off(GuildId::new(1));
        let limit = RoomSettings {
            default_limit: 100,
            ..base.clone()
        };
        assert_eq!(limit.validate(true), Err(SettingsError::BadLimit));
        let grace = RoomSettings {
            empty_grace_secs: 45,
            ..base.clone()
        };
        assert_eq!(grace.validate(true), Err(SettingsError::BadGrace));
        for bad in ["", "   ", &"x".repeat(101), "bell\u{7}"] {
            let s = RoomSettings {
                name_template: bad.to_string(),
                ..base.clone()
            };
            assert_eq!(s.validate(true), Err(SettingsError::BadTemplate), "{bad:?}");
        }
        let edge = RoomSettings {
            name_template: "x".repeat(100),
            default_limit: 99,
            ..base
        };
        assert_eq!(edge.validate(true), Ok(()));
    }
}
