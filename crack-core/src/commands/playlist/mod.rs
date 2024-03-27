pub mod add_to_playlist;
pub mod create_playlist;
pub mod delete_playlist;
pub mod get_playlist;
pub mod list_playlists;
pub mod play_playlist;

pub use add_to_playlist::add_to_playlist as addto;
pub use create_playlist::create_playlist as create;
pub use delete_playlist::delete_playlist as delete;
pub use get_playlist::get_playlist as get;
pub use list_playlists::list_playlists as list;
pub use play_playlist::play_playlist as play;

use crate::{Context, Error};

/// Playlist commands.
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("addto", "create", "delete", "get", "list", "play"),
    aliases("pl")
)]
#[cfg(not(tarpaulin_include))]
pub async fn playlist(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Playlist command called");

    ctx.say("You found the playlist command").await?;

    Ok(())
}
