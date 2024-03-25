use crate::errors::CrackedError;
use crate::utils::{build_playlist_list_embed, send_embed_response_poise};
use crate::{db::playlist::Playlist, Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "list")]
pub async fn list_playlists(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get() as i64;
    let pool = ctx
        .data()
        .database_pool
        .as_ref()
        .ok_or(CrackedError::NoDatabasePool)?;
    let playlists = Playlist::get_playlists_by_user_id(pool, user_id).await?;

    let embed = build_playlist_list_embed(&playlists, 1).await;

    // Send the embed
    send_embed_response_poise(ctx, embed).await?;

    Ok(())
}
