use crate::{db::playlist::Playlist, Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "get")]
pub async fn get_playlist(ctx: Context<'_>, playlist: String) -> Result<(), Error> {
    let pool = ctx.data().database_pool.as_ref().unwrap();
    let metadata: Vec<crate::db::Metadata> = match playlist.parse::<i32>() {
        // Try to parse the playlist as an ID
        Ok(playlist_id) => Playlist::get_track_metadata_for_playlist(pool, playlist_id).await?,
        Err(_) => {
            let user_id = ctx.author().id.get() as i64;

            Playlist::get_track_metadata_for_playlist_name(pool, playlist, user_id).await?
        }
    };
    // Assuming you have a way to fetch the user_id of the command issuer
    let aux_metadata = metadata
        .iter()
        .flat_map(|m| match crate::db::metadata::aux_metadata_from_db(m) {
            Ok(aux) => Some(aux),
            Err(e) => {
                tracing::error!("Error converting metadata to aux metadata: {}", e);
                None
            }
        })
        .collect::<Vec<_>>();
    // playlist.print_playlist(ctx).await?;
    let embed = crate::utils::build_tracks_embed_metadata(&aux_metadata, 1).await;

    // Send the embed
    crate::utils::send_embed_response_poise(ctx, embed).await?;

    Ok(())
}
