use crate::poise_ext::PoiseContextExt;
use crate::{Context, CrackedMessage, Error};

pub mod get;
pub mod prefix;
pub mod print_settings;
pub mod set;
pub mod toggle;

use crate::commands::help;
pub use get::get;
pub use prefix::*;
pub use print_settings::*;
pub use set::set;
pub use toggle::*;

use super::CrackedError;

/// Settings commands
#[poise::command(
    prefix_command,
    slash_command,
    subcommands(
        "set",
        "get",
        "toggle",
        "add_prefix",
        "get_prefixes",
        "clear_prefixes",
        "print_settings",
    ),
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn settings(
    ctx: Context<'_>,
    #[flag]
    #[description = "Shows the help menu for this command"]
    help: bool,
) -> Result<(), Error> {
    if help {
        help::wrapper(ctx).await?;
    }

    ctx.send_reply(CrackedMessage::CommandFound(String::from("settings")), true)
        .await?;

    Ok(())
}

/// Reload the settings for the current guild.
#[poise::command(prefix_command, owners_only, guild_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn reload(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let _ = ctx.data().reload_guild_settings(guild_id).await;

    ctx.send_reply(CrackedMessage::SettingsReload, true).await?;
    Ok(())
}

pub fn commands() -> Vec<crate::Command> {
    //vec![settings(), set::set(), get::get()]
    vec![settings()].into_iter().collect()
}

pub fn sub_commands() -> Vec<crate::Command> {
    vec![]
        .into_iter()
        .chain(set::commands())
        .chain(get::commands())
        .chain(toggle::commands())
        .chain(prefix::commands())
        .chain(print_settings::commands())
        .collect()
}
