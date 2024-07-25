pub mod ban;
pub mod unban;

pub use ban::*;
pub use unban::*;

pub use crate::poise_ext::ContextExt;
pub use crate::utils;

use crate::{Command, Context, Error};

/// User admin commands.
#[poise::command(
    category = "Admin",
    required_bot_permissions = "BAN_MEMBERS",
    required_permissions = "BAN_MEMBERS",
    prefix_command,
    slash_command,
    subcommands("ban", "unban"),
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn user(
    ctx: Context<'_>,
    // #[flag]
    // #[description = "Show the help menu."]
    // help: bool,
) -> Result<(), Error> {
    return crate::commands::help::wrapper(ctx).await;
}

pub fn user_commands() -> [Command; 3] {
    [ban(), unban(), unban_by_user_id()]
}
