use super::get_playlist::get_playlist_;
use crate::commands::{queue_aux_metadata, send_response_poise, CrackedMessage};
use crate::{Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "play")]
pub async fn play_playlist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist name or id"]
    playlist: String,
) -> Result<(), Error> {
    let (aux_metadata, playlist_name) = get_playlist_(ctx, playlist).await?;

    send_response_poise(ctx, CrackedMessage::PlaylistQueuing(playlist_name), true).await?;

    queue_aux_metadata(ctx, aux_metadata).await?;

    send_response_poise(ctx, CrackedMessage::PlaylistQueued, true).await?;

    Ok(())
}
