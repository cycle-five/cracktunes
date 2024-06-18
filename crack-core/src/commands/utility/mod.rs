pub mod invite;
pub mod ping;
pub mod version;

pub use invite::*;
pub use ping::*;
pub use version::*;

use crate::{CommandResult, Context, CrackedMessage, Error};

/// Get information about the servers this bot is in.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, prefix_command, owners_only)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}

use poise::serenity_prelude::Mentionable;

/// Shows how long TTS Bot has been online
#[poise::command(
    category = "Utility",
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn uptime(ctx: Context<'_>) -> CommandResult {
    let seconds = ctx
        .data()
        .start_time
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mention = {
        let current_user = ctx.cache().current_user();
        current_user.mention().to_string()
    };

    let msg = CrackedMessage::Uptime { mention, seconds };

    crate::utils::send_reply(&ctx, msg, true).await?;

    Ok(())
}

/// Get all the utility commands.
pub fn utility_commands() -> [crate::Command; 5] {
    [invite(), ping(), version(), servers(), uptime()]
}
