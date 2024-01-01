use crate::{
    db::playlist::Playlist, messaging::message::CrackedMessage, utils::send_response_poise,
    Context, Error,
};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "get")]
pub async fn get_playlist(ctx: Context<'_>, playlist: String) -> Result<(), Error> {
    let pool = ctx.data().database_pool.as_ref().unwrap();
    let metadata: Vec<crate::db::Metadata> = match playlist.parse::<i32>() {
        // Try to parse the playlist as an ID
        Ok(playlist_id) => {
            let user_id = ctx.author().id.get() as i64;
            Playlist::get_track_metadata_for_playlist(pool, playlist_id).await?
        }
        Err(_) => {
            let user_id = ctx.author().id.get() as i64;

            Playlist::get_track_metadata_for_playlist_name(pool, playlist, user_id).await?
        }
    };
    // Assuming you have a way to fetch the user_id of the command issuer
    let my_aux_metadata = metadata
        .iter()
        .map(|m| crate::utils::aux_metadata_from_db(m))
        .collect::<Vec<_>>();
    // playlist.print_playlist(ctx).await?;
    let _ = crate::utils::build_tracks_embed_metadata(metadata, page);

    Ok(())
}
