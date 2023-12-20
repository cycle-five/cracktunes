pub mod add_to_playlist;
pub mod create_playlist;
pub mod delete_playlist;

pub use add_to_playlist::*;
pub use create_playlist::*;
pub use delete_playlist::*;

use crate::{Context, Error};

/// Playlist commands.
#[poise::command(prefix_command, slash_command, subcommands("add", "create", "delete"))]
#[cfg(not(tarpaulin_include))]
pub async fn playlist(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Playlist command called");

    ctx.say("You found the playlist command").await?;

    Ok(())
}
