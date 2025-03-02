use crate::utils::{build_playlist_list_embed, send_embed_response_poise};
use crate::{
    commands::{cmd_check_music, sub_help as help},
    db::playlist::Playlist,
    Context, Error,
};
use crack_types::CrackedError;

/// List your saved playlists.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    rename = "list",
    check = "cmd_check_music",
    subcommands("help")
)]
pub async fn list_playlists(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get() as i64;
    let data = ctx.data();
    let pool = data
        .database_pool
        .as_ref()
        .ok_or(CrackedError::NoDatabasePool)?;
    let playlists = Playlist::get_playlists_by_user_id(pool, user_id).await?;

    let embed = build_playlist_list_embed(&playlists, 0).await;

    // Send the embed
    send_embed_response_poise(ctx, embed).await?;

    Ok(())
}
