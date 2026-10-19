//! How the bot's slash commands reach Discord: everywhere at startup, or only
//! where a server opted in with `/register`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The environment variable that overrides the configured mode.
pub const COMMAND_REGISTRATION_ENV: &str = "COMMAND_REGISTRATION";

/// How the slash commands are registered with Discord.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandRegistration {
    /// Every command is registered globally at startup.
    #[default]
    Global,
    /// Only `/register` is global; each server opts in to the rest.
    Manual,
}

/// A value that is neither `global` nor `manual`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCommandRegistration(pub String);

impl fmt::Display for UnknownCommandRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{COMMAND_REGISTRATION_ENV} must be `global` or `manual`, not `{}`",
            self.0
        )
    }
}

impl std::error::Error for UnknownCommandRegistration {}

impl FromStr for CommandRegistration {
    type Err = UnknownCommandRegistration;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.trim();
        if value.eq_ignore_ascii_case("global") {
            Ok(Self::Global)
        } else if value.eq_ignore_ascii_case("manual") {
            Ok(Self::Manual)
        } else {
            Err(UnknownCommandRegistration(value.to_string()))
        }
    }
}

impl CommandRegistration {
    /// Parse an env value. Unset or blank means "no override".
    pub fn from_env_value(raw: Option<&str>) -> Result<Option<Self>, UnknownCommandRegistration> {
        match raw {
            Some(value) if !value.trim().is_empty() => value.parse().map(Some),
            _ => Ok(None),
        }
    }

    /// The override from `COMMAND_REGISTRATION`, if one is set.
    pub fn from_env() -> Result<Option<Self>, UnknownCommandRegistration> {
        Self::from_env_value(std::env::var(COMMAND_REGISTRATION_ENV).ok().as_deref())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::BotConfig;

    #[test]
    fn parses_both_modes() {
        assert_eq!("global".parse(), Ok(CommandRegistration::Global));
        assert_eq!("manual".parse(), Ok(CommandRegistration::Manual));
        assert_eq!(" Manual\n".parse(), Ok(CommandRegistration::Manual));
        assert_eq!("MANUAL".parse(), Ok(CommandRegistration::Manual));
    }

    #[test]
    fn unknown_value_is_an_error() {
        let err = "manaul".parse::<CommandRegistration>().unwrap_err();
        let text = err.to_string();
        for needle in ["COMMAND_REGISTRATION", "manaul", "global", "manual"] {
            assert!(text.contains(needle), "{needle} missing from {text}");
        }
    }

    #[test]
    fn blank_env_means_unset() {
        assert_eq!(CommandRegistration::from_env_value(None), Ok(None));
        assert_eq!(CommandRegistration::from_env_value(Some("")), Ok(None));
        assert_eq!(CommandRegistration::from_env_value(Some("  ")), Ok(None));
        assert_eq!(
            CommandRegistration::from_env_value(Some("manual")),
            Ok(Some(CommandRegistration::Manual))
        );
        assert!(CommandRegistration::from_env_value(Some("x")).is_err());
    }

    #[test]
    fn config_defaults_to_global() {
        assert_eq!(
            BotConfig::default().command_registration,
            CommandRegistration::Global
        );
        let empty: BotConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.command_registration, CommandRegistration::Global);
        let manual: BotConfig =
            serde_json::from_str(r#"{"command_registration":"manual"}"#).unwrap();
        assert_eq!(manual.command_registration, CommandRegistration::Manual);
    }
}
