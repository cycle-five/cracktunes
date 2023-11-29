use crate::{
    commands::MyAuxMetadata,
    db::{self, Metadata, Playlist},
    utils::send_embed_response_str,
    Context, Error,
};
use songbird::input::AuxMetadata;
use sqlx::PgPool;

/// Adds a song to a playlist
#[poise::command(prefix_command, slash_command)]
pub async fn add_to_playlist(
    ctx: Context<'_>,
    #[description = "Track to add to playlist"] track: String,
) -> Result<(), Error> {
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

    let metadata = Metadata::create(&db_pool, in_metadata).await?;

    let res = Playlist::add_track(
        &db_pool,
        playlist.id.try_into().unwrap(),
        metadata.id.try_into().unwrap(),
        guild_id,
        channel_id,
    )
    .await?;

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

fn aux_metadata_to_db_structures(
    metadata: &AuxMetadata,
    guild_id: i64,
    channel_id: i64,
) -> Result<(db::Metadata, db::PlaylistTrack), Error> {
    let track = metadata.track.clone();
    let title = metadata.title.clone();
    let artist = metadata.artist.clone();
    let album = metadata.album.clone();
    let date = metadata
        .date
        .clone()
        .map(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").unwrap_or_default());
    let duration = metadata
        .duration
        .clone()
        .map(|x| ::chrono::Duration::from_std(x).unwrap_or(chrono::Duration::zero()));
    let channel = metadata.channel.clone();
    let channels = metadata.channels.clone().map(|d| i16::from(d));
    let start_time = metadata
        .start_time
        .clone()
        .map(|d| ::chrono::Duration::from_std(d).unwrap_or(chrono::Duration::zero()));
    let sample_rate = metadata.sample_rate.clone().map(|d| i64::from(d) as i32);
    let thumbnail = metadata.thumbnail.clone();
    let source_url = metadata.source_url.clone();

    let metadata = db::Metadata {
        id: 0,
        track,
        title,
        artist,
        album,
        date,
        duration,
        channel,
        channels,
        start_time,
        sample_rate,
        source_url,
        thumbnail,
    };

    let db_track = db::PlaylistTrack {
        id: 0,
        playlist_id: 0,
        guild_id: Some(guild_id),
        metadata_id: 0,
        channel_id: Some(channel_id),
    };

    Ok((metadata, db_track))
}
