use crate::{
    commands::MyAuxMetadata,
    db::{metadata::aux_metadata_from_db, playlist::Playlist, Metadata},
    utils::{build_tracks_embed_metadata, send_embed_response_poise},
    Context, CrackedError, Error,
};

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "get")]
pub async fn get_playlist(ctx: Context<'_>, #[rest] playlist: String) -> Result<(), Error> {
    let (aux_metadata, playlist_name): (Vec<MyAuxMetadata>, String) =
        get_playlist_(ctx, playlist).await?;
    let embed = build_tracks_embed_metadata(playlist_name, aux_metadata.as_slice(), 0).await;

    // Send the embed
    send_embed_response_poise(&ctx, embed).await?;

    Ok(())
}

/// Get a playlist by name or id
pub async fn get_playlist_(
    ctx: Context<'_>,
    playlist: String,
) -> Result<(Vec<MyAuxMetadata>, String), Error> {
    let pool = ctx
        .data()
        .database_pool
        .as_ref()
        .ok_or(CrackedError::NoDatabasePool)?;

    let metadata: Vec<Metadata> = match playlist.parse::<i32>() {
        // Try to parse the playlist as an ID
        Ok(playlist_id) => Playlist::get_track_metadata_for_playlist(pool, playlist_id).await?,
        Err(_) => {
            let user_id = ctx.author().id.get() as i64;

            Playlist::get_track_metadata_for_playlist_name(pool, playlist.clone(), user_id).await?
        },
    };
    // Assuming you have a way to fetch the user_id of the command issuer
    let aux_metadata = metadata
        .iter()
        .flat_map(|m| match aux_metadata_from_db(m) {
            Ok(aux) => Some(MyAuxMetadata::Data(aux.clone())),
            Err(e) => {
                tracing::error!("Error converting metadata to aux metadata: {}", e);
                None
            },
        })
        .collect::<Vec<_>>();
    // playlist.print_playlist(ctx).await?;
    Ok((aux_metadata, playlist))
}
