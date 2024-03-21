use crate::{db::playlist::Playlist, Context, Error};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "list")]
pub async fn list_playlists(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.get() as i64;
    let pool = ctx.data().database_pool.as_ref().unwrap();
    let playlists = get_playlists_by_user_id(user_id, pool).await;

    let embed = crate::utils::build_tracks_embed_metadata(&aux_metadata, 1).await;

    // Send the embed
    crate::utils::send_embed_response_poise(ctx, embed).await?;

    Ok(())
}

/// Get the playlists for a given user_id.
pub async fn get_playlists_by_user_id(user_id: UserId, pool: Pool) -> Vec<AuxMetadata> {
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
    tracing::warn!("aux_metadata: {:?}", len(aux_metadata));
    aux_metadata
    // playlist.print_playlist(ctx).await?;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::aux_metadata_to_db_structures;
    use crate::db::metadata::aux_metadata_from_db;
    use crate::db::metadata::AuxMetadata;
    use crate::db::metadata::Metadata;
    use crate::db::playlist::Playlist;

    #[tokio::test]
    async fn test_get_playlists_by_user_id() {
        let user_id = 1;
        let pool = Pool::new();
        let playlists = get_playlists_by_user_id(user_id, pool).await;
        assert_eq!(playlists.len(), 0);
    }
}
