pub mod invite;
pub mod ping;
pub mod version;

pub use invite::*;
pub use ping::*;
pub use version::*;

use crate::Context;
use crate::Error;
/// Get information about the servers this bot is in.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, owners_only, category = "Utility")]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}

/// Get all the utility commands.
pub fn utility_commands() -> [crate::Command; 4] {
    [invite(), ping(), version(), servers()]
}
