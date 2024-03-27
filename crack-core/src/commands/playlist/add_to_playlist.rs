use crate::{
    commands::MyAuxMetadata,
    db::aux_metadata_to_db_structures,
    db::{metadata::Metadata, Playlist},
    errors::CrackedError,
    utils::send_embed_response_str,
    Context, Error,
};
use sqlx::PgPool;

/// Adds a song to a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only, rename = "addto")]
pub async fn add_to_playlist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist to add current track to"]
    playlist: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let queue = call.lock().await.queue().clone();
    let cur_track = queue.current().ok_or(CrackedError::NothingPlaying)?;
    let typemap = cur_track.typemap().read().await;
    let metadata = match typemap.get::<MyAuxMetadata>() {
        Some(MyAuxMetadata::Data(meta)) => meta,
        None => {
            return send_embed_response_str(
                ctx,
                "Failed to get metadata for the current track".to_string(),
            )
            .await
            .map(|_| ())
        }
    };

    // // Extract playlist name and track ID from the arguments
    let guild_id_i64 = guild_id.get() as i64;
    let channel_id = ctx.channel_id().get() as i64;
    let track = match metadata.track.clone() {
        Some(track) => track,
        None => metadata.title.clone().ok_or(CrackedError::NoTrackName)?,
    };
    //.unwrap_or(metadata.title.clone().ok_or(CrackedError::NoTrackName)?)
    let user_id = ctx.author().id.get() as i64;
    // Database pool to execute queries
    let db_pool: PgPool = ctx
        .data()
        .database_pool
        .clone()
        .ok_or(CrackedError::NoDatabasePool)?;
    let playlist_name = playlist;

    // Get playlist if exists, other create it.
    let playlist =
        match Playlist::get_playlist_by_name(&db_pool, playlist_name.clone(), user_id).await {
            Ok(playlist) => Ok(playlist),
            Err(e) => {
                tracing::error!("Error getting playlist: {:?}", e);
                tracing::info!("Creating playlist: {:?}", playlist_name);
                Playlist::create(&db_pool, &playlist_name, user_id).await
            }
        }?;

    let (in_metadata, _playlist_track) =
        aux_metadata_to_db_structures(metadata, guild_id_i64, channel_id)?;

    let metadata = Metadata::get_or_create(&db_pool, &in_metadata).await?;

    let res =
        Playlist::add_track(&db_pool, playlist.id, metadata.id, guild_id_i64, channel_id).await?;

    let operation_successfull = res.rows_affected() > 0;

    // Send a feedback message to the user
    if operation_successfull {
        ctx.reply(format!(
            "Track {} has been added to playlist {}",
            track, playlist_name
        ))
        .await
        .map(|_| ())
        .map_err(|e| e.into())
    } else {
        ctx.reply(format!(
            "Failed to add track {} to playlist {}",
            track, playlist_name
        ))
        .await
        .map(|_| ())
        .map_err(|e| e.into())
    }
}
