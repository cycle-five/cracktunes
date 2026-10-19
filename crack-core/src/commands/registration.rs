//! How the bot's slash commands reach Discord: everywhere at startup, or only
//! where a server opted in with `/register`.

use super::register::{create_application_commands_cracked, create_application_commands_filtered};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// The one command that is global in every mode.
pub const REGISTER_COMMAND_NAME: &str = "register";

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

    /// The commands registered globally (visible in every server).
    pub fn global_commands<U, E>(
        self,
        commands: &[poise::Command<U, E>],
    ) -> Vec<serenity::CreateCommand<'static>> {
        match self {
            Self::Global => create_application_commands_cracked(commands),
            Self::Manual => {
                create_application_commands_filtered(commands, |c| c.name == REGISTER_COMMAND_NAME)
            },
        }
    }

    /// The commands registered in a server: everything, or in `Manual` mode
    /// everything but `/register` (which is already global).
    pub fn guild_commands<U, E>(
        self,
        commands: &[poise::Command<U, E>],
    ) -> Vec<serenity::CreateCommand<'static>> {
        match self {
            Self::Global => create_application_commands_cracked(commands),
            Self::Manual => {
                create_application_commands_filtered(commands, |c| c.name != REGISTER_COMMAND_NAME)
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::commands::{all_commands, commands_for};
    use crate::BotConfig;
    use poise::serenity_prelude::Permissions;

    /// The one field of a command's wire body these tests look at.
    #[derive(Deserialize)]
    struct SentCommand {
        name: String,
    }

    /// The names of the commands as Discord would receive them.
    pub(crate) fn sent_names(cmds: &[serenity::CreateCommand<'_>]) -> Vec<String> {
        cmds.iter()
            .map(|c| {
                let body = serde_json::to_string(c).expect("a command serializes");
                serde_json::from_str::<SentCommand>(&body)
                    .expect("a command body has a name")
                    .name
            })
            .collect()
    }

    fn register_of(cmds: &[crate::Command]) -> &crate::Command {
        cmds.iter()
            .find(|c| c.name == REGISTER_COMMAND_NAME)
            .expect("register is a command")
    }

    #[test]
    fn register_is_for_mods_in_manual_mode() {
        let cmds = commands_for(CommandRegistration::Manual);
        let register = register_of(&cmds);
        assert_eq!(register.required_permissions, Permissions::MANAGE_GUILD);
        assert_eq!(
            register.default_member_permissions,
            Permissions::MANAGE_GUILD
        );
    }

    #[test]
    fn register_is_unchanged_in_global_mode() {
        let cmds = commands_for(CommandRegistration::Global);
        let register = register_of(&cmds);
        assert_eq!(register.required_permissions, Permissions::ADMINISTRATOR);
        assert_eq!(register.default_member_permissions, Permissions::empty());
    }

    #[test]
    fn both_modes_have_the_same_commands() {
        let names = |cmds: Vec<crate::Command>| -> Vec<String> {
            cmds.into_iter().map(|c| c.name.to_string()).collect()
        };
        let all = names(all_commands());
        assert_eq!(names(commands_for(CommandRegistration::Manual)), all);
        assert_eq!(names(commands_for(CommandRegistration::Global)), all);
    }

    #[test]
    fn register_constant_names_the_command() {
        assert_eq!(crate::commands::register().name, REGISTER_COMMAND_NAME);
    }

    #[test]
    fn manual_registers_only_register_globally() {
        let cmds = commands_for(CommandRegistration::Manual);
        assert_eq!(
            sent_names(&CommandRegistration::Manual.global_commands(&cmds)),
            ["register"]
        );
    }

    #[test]
    fn manual_guild_set_is_everything_but_register() {
        let cmds = commands_for(CommandRegistration::Manual);
        let mut expected = sent_names(&create_application_commands_cracked(&cmds));
        assert!(expected.iter().any(|n| n == "register"));
        expected.retain(|n| n != "register");
        assert!(!expected.is_empty());
        assert_eq!(
            sent_names(&CommandRegistration::Manual.guild_commands(&cmds)),
            expected
        );
    }

    #[test]
    fn global_mode_sends_what_it_always_did() {
        let cmds = commands_for(CommandRegistration::Global);
        let wire = |c: &[serenity::CreateCommand<'_>]| serde_json::to_string(c).unwrap();
        let always = wire(&create_application_commands_cracked(&cmds));
        assert_eq!(
            wire(&CommandRegistration::Global.global_commands(&cmds)),
            always
        );
        assert_eq!(
            wire(&CommandRegistration::Global.guild_commands(&cmds)),
            always
        );
    }

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
