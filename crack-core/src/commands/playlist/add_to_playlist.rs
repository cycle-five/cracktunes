use crate::{
    commands::MyAuxMetadata,
    db::{metadata::Metadata, Playlist},
    utils::send_embed_response_str,
    Context, Error,
};
use sqlx::PgPool;

/// Adds a song to a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "add")]
pub async fn add_to_playlist(
    ctx: Context<'_>,
    #[description = "Track to add to playlist"] track: String,
) -> Result<(), Error> {
    use crate::db::aux_metadata_to_db_structures;

    let _ = track;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(ctx.guild_id().unwrap()).unwrap();
    let queue = call.lock().await.queue().clone();
    let cur_track = queue.current().unwrap();
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
    let guild_id = ctx.guild_id().unwrap().get() as i64;
    let channel_id = ctx.channel_id().get() as i64;
    let _track = metadata.track.clone().unwrap();
    let user_id = ctx.author().id.get() as i64;
    let playlist_name = format!("{}-0", user_id);
    // Database pool to execute queries
    let db_pool: PgPool = ctx.data().database_pool.clone().unwrap();

    // Check if the playlist exists
    // TODO: Add the SQL query and logic here
    let playlist = match Playlist::create(&db_pool, &playlist_name, user_id).await {
        Ok(playlist) => Ok(playlist),
        Err(_) => Playlist::get_playlist_by_name(&db_pool, playlist_name.clone(), user_id).await,
    }?;

    // // Check if the track exists
    // // TODO: Add the SQL query and logic here

    // // Add the track to the playlist
    // // TODO: Add the SQL query and logic here

    let (in_metadata, _playlist_track) =
        aux_metadata_to_db_structures(metadata, guild_id, channel_id)?;

    let metadata = Metadata::create(&db_pool, &in_metadata).await?;

    let res = Playlist::add_track(&db_pool, playlist.id, metadata.id, guild_id, channel_id).await?;

    let operation_successfull = res.rows_affected() > 0;

    // Send a feedback message to the user
    if operation_successfull {
        ctx.reply(format!(
            "Track {} has been added to playlist {}",
            _track, playlist_name
        ))
        .await
        .map(|_| ())
        .map_err(|e| e.into())
    } else {
        ctx.reply(format!(
            "Failed to add track {} to playlist {}",
            _track, playlist_name
        ))
        .await
        .map(|_| ())
        .map_err(|e| e.into())
    }
}
