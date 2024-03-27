use super::get_playlist::get_playlist_;
use crate::errors::CrackedError;
use crate::utils::{build_playlist_list_embed, send_embed_response_poise};
use crate::{db::playlist::Playlist, Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "play")]
pub async fn play_playlist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist name or id"]
    playlist: String,
) -> Result<(), Error> {
    use crate::commands::{queue_aux_metadata, send_response_poise, CrackedMessage};

    let (aux_metadata, playlist_name) = get_playlist_(ctx, playlist).await?;
    // let user_id = ctx.author().id.get() as i64;
    // let pool = ctx
    //     .data()
    //     .database_pool
    //     .as_ref()
    //     .ok_or(CrackedError::NoDatabasePool)?;
    // let playlists = Playlist::get_playlists_by_user_id(pool, user_id).await?;
    send_response_poise(ctx, CrackedMessage::PlaylistQueuing(playlist_name), true).await?;
    let _ = queue_aux_metadata(ctx, aux_metadata).await?;

    // let embed = build_playlist_list_embed(&playlists, 0).await;

    // // Send the embed
    // send_embed_response_poise(ctx, embed).await?;
    send_response_poise(ctx, CrackedMessage::PlaylistQueued, true).await?;

    Ok(())
}
