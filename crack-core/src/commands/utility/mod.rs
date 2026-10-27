pub mod clean;
mod debug;
pub mod get_letter_league_players;
pub mod invite;
pub mod ping;
#[expect(
    dead_code,
    reason = "saychan and saychanid are commented out of utility_commands(); only a glob re-export kept them alive"
)]
mod say;
#[expect(
    dead_code,
    reason = "smoketest is commented out of utility_commands(); only a glob re-export kept it alive"
)]
mod smoketest;
pub mod version;

pub use clean::clean;
pub use debug::debug;
pub use invite::invite;
pub use ping::ping;
pub use version::version;

use crate::{CommandResult, Context, CrackedMessage, Error};
use poise::serenity_prelude::Mentionable;

/// Get information about the servers this bot is in.
#[poise::command(category = "Utility", slash_command, prefix_command, owners_only)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}

/// Shows how long TTS Bot has been online
#[poise::command(
    category = "Utility",
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES"
)]
pub async fn uptime(ctx: Context<'_>) -> CommandResult {
    let now = std::time::SystemTime::now();
    let seconds = now.duration_since(ctx.data().start_time)?.as_secs();
    let mention = {
        let current_user = ctx.cache().current_user();
        current_user.mention().to_string()
    };

    let msg = CrackedMessage::Uptime { mention, seconds };

    crate::utils::send_reply(&ctx, msg, true).await?;

    Ok(())
}

pub async fn uptime_internal(ctx: Context<'_>) -> CrackedMessage {
    let now = std::time::SystemTime::now();
    let seconds = now
        .duration_since(ctx.data().start_time)
        .unwrap_or_default()
        .as_secs();
    let mention = {
        let current_user = ctx.cache().current_user();
        current_user.mention().to_string()
    };

    Into::into(CrackedMessage::Uptime { mention, seconds })
}

/// Get all the utility commands.
pub fn utility_commands() -> [crate::Command; 6] {
    [
        clean(),
        debug(),
        invite(),
        ping(),
        // servers(),
        // saychan(),
        // saychanid(),
        // smoketest(),
        uptime(),
        version(),
    ]
}
