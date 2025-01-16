pub mod ban;
pub mod deafen;
pub mod unban;

pub use ban::*;
pub use deafen::*;
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
    subcommands("ban", "unban", "deafen", "undeafen"),
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn user(
    ctx: Context<'_>,
    // #[flag]
    // #[description = "Show the help menu."]
    // help: bool,
) -> Result<(), Error> {
    //crate::commands::help::wrapper(ctx).await
    ctx.say("We're porting!")
        .await
        .map(|_| ())
        .map_err(Into::into)
}

#[must_use]
pub fn user_commands() -> [Command; 4] {
    [ban(), unban(), deafen(), undeafen()]
}
