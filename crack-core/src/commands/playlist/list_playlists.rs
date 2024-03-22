use crate::utils::{build_playlist_list_embed, send_embed_response_poise};
use serenity::all::UserId;
use sqlx::PgPool;

use crate::{db::playlist::Playlist, Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "list")]
pub async fn list_playlists(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get();
    let user_id: UserId = UserId::new(user_id);
    let pool = ctx.data().database_pool.as_ref().unwrap();
    let playlists = get_playlists_by_user_id(user_id, pool).await?;

    let embed = build_playlist_list_embed(&playlists, 1).await;

    // Send the embed
    send_embed_response_poise(ctx, embed).await?;

    Ok(())
}

/// Get the playlists for a given user_id.
pub async fn get_playlists_by_user_id(
    user_id: UserId,
    pool: &PgPool,
) -> Result<Vec<Playlist>, Error> {
    let user_id_i64 = user_id.get() as i64;
    let playlists = Playlist::get_playlists_by_user_id(pool, user_id_i64).await;
    // let metadata: Vec<crate::db::Metadata> = match playlist.parse::<i32>() {
    //     Ok(playlist_id) => Playlist::get_track_metadata_for_playlist(pool, playlist_id).await?,
    // };
    // // Assuming you have a way to fetch the user_id of the command issuer
    // let aux_metadata = metadata
    //     .iter()
    //     .flat_map(|m| match crate::db::metadata::aux_metadata_from_db(m) {
    //         Ok(aux) => Some(aux),
    //         Err(e) => {
    //             tracing::error!("Error converting metadata to aux metadata: {}", e);
    //             None
    //         }
    //     })
    //     .collect::<Vec<_>>();
    tracing::warn!("res: {:?}", playlists.is_ok());
    Ok(playlists?)
    // playlist.print_playlist(ctx).await?;
}
