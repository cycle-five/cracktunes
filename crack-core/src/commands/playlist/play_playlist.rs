use super::get_playlist::get_playlist_;
use crate::commands::queue_aux_metadata;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
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
    // Check for playlist Id
    let (aux_metadata, playlist_name) = get_playlist_(ctx, playlist).await?;

    let msg =
        send_response_poise(ctx, CrackedMessage::PlaylistQueuing(playlist_name), true).await?;

    queue_aux_metadata(ctx, aux_metadata.as_slice(), msg).await?;

    send_response_poise(ctx, CrackedMessage::PlaylistQueued, true).await?;

    Ok(())
}
