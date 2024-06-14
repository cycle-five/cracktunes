use super::get_playlist::get_playlist_;
use crate::commands::queue_aux_metadata;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};

/// Queue a playlist on the bot.
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

    let handle = send_reply(ctx, CrackedMessage::PlaylistQueuing(playlist_name), true).await?;
    let msg = handle.into_message().await?;

    queue_aux_metadata(ctx, aux_metadata.as_slice(), msg).await?;

    send_reply(ctx, CrackedMessage::PlaylistQueued, true).await?;

    Ok(())
}
