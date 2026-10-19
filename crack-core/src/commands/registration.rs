//! How the bot's slash commands reach Discord: everywhere at startup, or only
//! where a server opted in with `/register`.

use super::register::{create_application_commands_cracked, create_application_commands_filtered};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

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

/// A Discord call that failed while syncing commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandsError(pub String);

impl fmt::Display for CommandsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandsError {}

impl From<serenity::Error> for CommandsError {
    fn from(err: serenity::Error) -> Self {
        Self(err.to_string())
    }
}

/// The only door the command sync uses, so tests can record what would be sent.
#[serenity::async_trait]
pub trait CommandsDiscord: Send + Sync {
    /// Replace the application's global commands.
    async fn set_global(
        &self,
        commands: &[serenity::CreateCommand<'static>],
    ) -> Result<(), CommandsError>;
    /// How many commands the application has in this server.
    async fn guild_command_count(&self, guild: serenity::GuildId) -> Result<usize, CommandsError>;
    /// Replace the application's commands in this server.
    async fn set_guild(
        &self,
        guild: serenity::GuildId,
        commands: &[serenity::CreateCommand<'static>],
    ) -> Result<(), CommandsError>;
}

/// The real Discord.
pub struct LiveCommandsDiscord {
    pub http: Arc<serenity::Http>,
}

#[serenity::async_trait]
impl CommandsDiscord for LiveCommandsDiscord {
    async fn set_global(
        &self,
        commands: &[serenity::CreateCommand<'static>],
    ) -> Result<(), CommandsError> {
        serenity::Command::set_global_commands(&self.http, commands).await?;
        Ok(())
    }

    async fn guild_command_count(&self, guild: serenity::GuildId) -> Result<usize, CommandsError> {
        Ok(guild.get_commands(&self.http).await?.len())
    }

    async fn set_guild(
        &self,
        guild: serenity::GuildId,
        commands: &[serenity::CreateCommand<'static>],
    ) -> Result<(), CommandsError> {
        guild.set_commands(&self.http, commands).await?;
        Ok(())
    }
}

/// What a startup refresh of the opted-in servers did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GuildRefresh {
    pub checked: usize,
    pub refreshed: usize,
    pub failed: usize,
}

/// Puts this mode's global set on Discord; `Ok` carries how many commands that was.
pub async fn sync_global<U, E>(
    api: &dyn CommandsDiscord,
    mode: CommandRegistration,
    commands: &[poise::Command<U, E>],
) -> Result<usize, CommandsError> {
    let global = mode.global_commands(commands);
    api.set_global(&global).await?;
    Ok(global.len())
}

/// Manual mode: for each server that already has commands (it opted in), write
/// the current server set, so a new build reaches it without another `/register`.
/// A failure is logged and counted, and the loop goes on. Global mode does nothing.
pub async fn refresh_guilds<U, E>(
    api: &dyn CommandsDiscord,
    mode: CommandRegistration,
    commands: &[poise::Command<U, E>],
    guilds: &[serenity::GuildId],
) -> GuildRefresh {
    let mut result = GuildRefresh::default();
    if mode != CommandRegistration::Manual {
        return result;
    }
    let set = mode.guild_commands(commands);
    for &guild in guilds {
        result.checked += 1;
        match api.guild_command_count(guild).await {
            Ok(0) => {},
            Ok(_) => match api.set_guild(guild, &set).await {
                Ok(()) => result.refreshed += 1,
                Err(e) => {
                    tracing::warn!("Failed to refresh commands in guild {guild}: {e}");
                    result.failed += 1;
                },
            },
            Err(e) => {
                tracing::warn!("Failed to count commands in guild {guild}: {e}");
                result.failed += 1;
            },
        }
    }
    result
}

#[cfg(test)]
mod test {
    use super::serenity;
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

    // ---- startup sync and guild refresh ----

    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        SetGlobal(Vec<String>),
        GuildCount(serenity::GuildId),
        SetGuild(serenity::GuildId, Vec<String>),
    }

    #[derive(Default)]
    struct FakeCommandsDiscord {
        calls: Mutex<Vec<Call>>,
        counts: HashMap<serenity::GuildId, usize>,
        fail_count: HashSet<serenity::GuildId>,
        fail_set: HashSet<serenity::GuildId>,
        fail_global: bool,
    }

    impl FakeCommandsDiscord {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[serenity::async_trait]
    impl CommandsDiscord for FakeCommandsDiscord {
        async fn set_global(
            &self,
            commands: &[serenity::CreateCommand<'static>],
        ) -> Result<(), CommandsError> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetGlobal(sent_names(commands)));
            if self.fail_global {
                return Err(CommandsError("global refused".into()));
            }
            Ok(())
        }

        async fn guild_command_count(
            &self,
            guild: serenity::GuildId,
        ) -> Result<usize, CommandsError> {
            self.calls.lock().unwrap().push(Call::GuildCount(guild));
            if self.fail_count.contains(&guild) {
                return Err(CommandsError("count refused".into()));
            }
            Ok(self.counts.get(&guild).copied().unwrap_or(0))
        }

        async fn set_guild(
            &self,
            guild: serenity::GuildId,
            commands: &[serenity::CreateCommand<'static>],
        ) -> Result<(), CommandsError> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::SetGuild(guild, sent_names(commands)));
            if self.fail_set.contains(&guild) {
                return Err(CommandsError("set refused".into()));
            }
            Ok(())
        }
    }

    fn gid(n: u64) -> serenity::GuildId {
        serenity::GuildId::new(n)
    }

    fn without_register(mut names: Vec<String>) -> Vec<String> {
        names.retain(|n| n != REGISTER_COMMAND_NAME);
        names
    }

    #[tokio::test]
    async fn global_mode_syncs_everything_and_refreshes_nothing() {
        let cmds = commands_for(CommandRegistration::Global);
        let all = create_application_commands_cracked(&cmds);
        let api = FakeCommandsDiscord::default();

        let n = sync_global(&api, CommandRegistration::Global, &cmds)
            .await
            .expect("sync succeeds");
        assert_eq!(n, all.len());
        assert_eq!(api.calls(), [Call::SetGlobal(sent_names(&all))]);

        let api = FakeCommandsDiscord::default();
        let result =
            refresh_guilds(&api, CommandRegistration::Global, &cmds, &[gid(1), gid(2)]).await;
        assert!(api.calls().is_empty());
        assert_eq!(result, GuildRefresh::default());
    }

    #[tokio::test]
    async fn manual_mode_syncs_only_register() {
        let cmds = commands_for(CommandRegistration::Manual);
        let api = FakeCommandsDiscord::default();
        let n = sync_global(&api, CommandRegistration::Manual, &cmds)
            .await
            .expect("sync succeeds");
        assert_eq!(n, 1);
        assert_eq!(api.calls(), [Call::SetGlobal(vec!["register".to_string()])]);
    }

    #[tokio::test]
    async fn refresh_writes_guild_set_only_where_commands_exist() {
        let cmds = commands_for(CommandRegistration::Manual);
        let expected = without_register(sent_names(&create_application_commands_cracked(&cmds)));
        let (g1, g2) = (gid(1), gid(2));
        let api = FakeCommandsDiscord {
            counts: HashMap::from([(g1, 0), (g2, 3)]),
            ..Default::default()
        };
        let result = refresh_guilds(&api, CommandRegistration::Manual, &cmds, &[g1, g2]).await;
        assert_eq!(
            api.calls(),
            [
                Call::GuildCount(g1),
                Call::GuildCount(g2),
                Call::SetGuild(g2, expected),
            ]
        );
        assert_eq!(
            result,
            GuildRefresh {
                checked: 2,
                refreshed: 1,
                failed: 0
            }
        );
    }

    #[tokio::test]
    async fn refresh_continues_past_a_failing_guild() {
        let cmds = commands_for(CommandRegistration::Manual);
        let expected = without_register(sent_names(&create_application_commands_cracked(&cmds)));
        let (g1, g2, g3) = (gid(1), gid(2), gid(3));
        let api = FakeCommandsDiscord {
            counts: HashMap::from([(g2, 3), (g3, 2)]),
            fail_count: HashSet::from([g1]),
            fail_set: HashSet::from([g3]),
            ..Default::default()
        };
        let result = refresh_guilds(&api, CommandRegistration::Manual, &cmds, &[g1, g2, g3]).await;
        assert_eq!(
            api.calls(),
            [
                Call::GuildCount(g1),
                Call::GuildCount(g2),
                Call::SetGuild(g2, expected.clone()),
                Call::GuildCount(g3),
                Call::SetGuild(g3, expected),
            ]
        );
        assert_eq!(
            result,
            GuildRefresh {
                checked: 3,
                refreshed: 1,
                failed: 2
            }
        );
    }

    #[tokio::test]
    async fn sync_global_reports_failure() {
        let cmds = commands_for(CommandRegistration::Global);
        let api = FakeCommandsDiscord {
            fail_global: true,
            ..Default::default()
        };
        let result = sync_global(&api, CommandRegistration::Global, &cmds).await;
        assert_eq!(result, Err(CommandsError("global refused".into())));
    }
}
