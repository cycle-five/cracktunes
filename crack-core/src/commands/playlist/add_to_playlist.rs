use sqlx::SqlitePool;

// Import necessary modules and libraries
use crate::{commands::MyAuxMetadata, db, utils::send_embed_response_str, Context, Error};
//use sqlx::PgPool;

/// Adds a song to a playlist
#[poise::command(prefix_command, slash_command)]
async fn add_to_playlist(
    ctx: Context<'_>,
    #[description = "Track to add to playlist"] track: String,
) -> Result<(), Error> {
    let _ = track;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(ctx.guild_id().unwrap()).unwrap();
    let queue = call.lock().await.queue().clone();
    let cur_track = queue.current().unwrap();
    let metadata = match cur_track.typemap().read().await.get::<MyAuxMetadata>() {
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
    // let guild_id = ctx.guild_id.unwrap().0 as i64;
    // let channel_id = ctx.channel_id.0 as i64;
    let user_id = ctx.author().id.get() as i64;
    let playlist_name = format!("{}-0", user_id);
    // Database pool to execute queries
    let db_pool: SqlitePool = ctx.data().database_pool.clone().unwrap();

    // Check if the playlist exists
    // TODO: Add the SQL query and logic here
    db::playlist::Playlist::create(&db_pool, &playlist_name, user_id).await?;

    // // Check if the track exists
    // // TODO: Add the SQL query and logic here

    // // Add the track to the playlist
    // // TODO: Add the SQL query and logic here

    // // Send a feedback message to the user
    // if operation_successful {
    //     msg.reply(
    //         ctx,
    //         format!(
    //             "Track {} has been added to playlist {}",
    //             track_id, playlist_name
    //         ),
    //     )
    //     .await?;
    // } else {
    //     msg.reply(
    //         ctx,
    //         format!(
    //             "Failed to add track {} to playlist {}",
    //             track_id, playlist_name
    //         ),
    //     )
    //     .await?;
    // }

    Ok(())
}
