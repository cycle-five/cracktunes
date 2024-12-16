pub mod admin;
#[cfg(feature = "crack-bf")]
pub mod bf;
#[cfg(feature = "crack-gpt")]
pub mod chatgpt;
pub mod help;
pub mod music;
pub mod music_utils;
#[cfg(feature = "crack-osint")]
pub mod osint;
pub mod permissions;
//pub mod playlist;
pub mod register;
pub mod settings;
pub mod utility;

//pub use admin::commands;
#[cfg(feature = "crack-bf")]
pub use bf::*;
#[cfg(feature = "crack-gpt")]
pub use chatgpt::*;
pub use help::sub_help;
pub use music::*;
pub use music_utils::*;
#[cfg(feature = "crack-osint")]
pub use osint::*;
pub use permissions::*;
pub use register::*;
pub use settings::*;
pub use utility::*;

pub use crate::errors::CrackedError;
use dashmap;
use serenity::all::Message;

pub type MessageResult = Result<Message, CrackedError>;
pub type EmptyResult = Result<(), crate::Error>;

use crate::{Context, Error};
use std::borrow::Cow;

pub trait ConvertToEmptyResult {
    fn convert(self) -> EmptyResult;
}

impl ConvertToEmptyResult for MessageResult {
    fn convert(self) -> EmptyResult {
        self.map(|_| ()).map_err(|e| e.into())
    }
}

/// Return all the commands that are available in the bot.
pub fn all_commands() -> Vec<crate::Command> {
    vec![
        register(),
        #[cfg(feature = "crack-bf")]
        bf(),
        #[cfg(feature = "crack-osint")]
        osint(),
        #[cfg(feature = "crack-gpt")]
        chat(),
    ]
    .into_iter()
    .chain(help::help_commands())
    .chain(music::music_commands())
    // .chain(music::game_commands())
    .chain(utility::utility_commands())
    .chain(settings::commands())
    //.chain(admin::commands())
    //.chain(playlist::commands())
    .collect()
}

/// Return all the commands that are available in the bot.
pub fn commands_to_register() -> Vec<crate::Command> {
    vec![
        register(),
        #[cfg(feature = "crack-bf")]
        bf(),
        #[cfg(feature = "crack-osint")]
        osint(),
        #[cfg(feature = "crack-gpt")]
        chat(),
    ]
    .into_iter()
    .chain(help::help_commands())
    .chain(music::music_commands())
    // .chain(music::game_commands())
    .chain(utility::utility_commands())
    .chain(settings::commands())
    //.chain(admin::commands())
    //.chain(playlist::commands())
    .collect()
}

pub fn all_command_names() -> Vec<Cow<'static, str>> {
    all_commands().into_iter().map(|c| c.name).collect()
}

pub fn all_commands_map() -> dashmap::DashMap<Cow<'static, str>, crate::Command> {
    all_commands()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect::<dashmap::DashMap<_, _>>()
}

// use poise::serenity_prelude as serenity;
// /// Collects all commands into a [`Vec<serenity::CreateCommand>`] builder, which can be used
// /// to register the commands on Discord
// ///
// /// Also see [`register_application_commands_buttons`] for a ready to use register command
// ///
// /// ```rust,no_run
// /// # use poise::serenity_prelude as serenity;
// /// # async fn foo(ctx: poise::Context<'_, (), ()>) -> Result<(), serenity::Error> {
// /// let commands = &ctx.framework().options().commands;
// /// let create_commands = poise::builtins::create_application_commands_minus_help(commands);
// ///
// /// serenity::Command::set_global_commands(ctx, create_commands).await?;
// /// # Ok(()) }
// /// ```
// pub fn create_application_commands_minus_help<U, E>(
//     commands: &[poise::Command<U, E>],
// ) -> Vec<serenity::CreateCommand> {
//     /// We decided to extract context menu commands recursively, despite the subcommand hierarchy
//     /// not being preserved. Because it's more confusing to just silently discard context menu
//     /// commands if they're not top-level commands.
//     /// https://discord.com/channels/381880193251409931/919310428344029265/947970605985189989
//     fn recursively_add_context_menu_commands<U, E>(
//         builder: &mut Vec<serenity::CreateCommand>,
//         command: &poise::Command<U, E>,
//     ) {
//         if let Some(context_menu_command) = command.create_as_context_menu_command() {
//             builder.push(context_menu_command);
//         }
//         for subcommand in &command.subcommands {
//             if subcommand.name != "help" {
//                 recursively_add_context_menu_commands(builder, subcommand);
//             }
//         }
//     }

//     let mut commands_builder = Vec::with_capacity(commands.len());
//     for command in commands {
//         if let Some(slash_command) = command.create_as_slash_command() {
//             commands_builder.push(slash_command);
//         }
//         recursively_add_context_menu_commands(&mut commands_builder, command);
//     }
//     commands_builder
// }

/// Interactively register bot commands.
#[poise::command(
    category = "Admin",
    guild_only,
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    register_application_commands_buttons_cracked(ctx).await?;
    Ok(())
}
