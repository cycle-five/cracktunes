use crate::{
    commands::cmd_check_music,
    db::aux_metadata_to_db_structures,
    db::{metadata::Metadata, MetadataAnd, Playlist},
    errors::CrackedError,
    poise_ext::ContextExt as _,
    utils::TrackData,
    Context, CrackedMessage, Error,
};
use sqlx::PgPool;

/// Adds a song to a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only,
    rename = "addto"
)]
pub async fn add_to_playlist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist to add current track to"]
    playlist: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = ctx.data().songbird.clone();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let queue = call.lock().await.queue().clone();
    let cur_track = queue.current().ok_or(CrackedError::NothingPlaying)?;

    let data = cur_track.data::<TrackData>();
    let metadata_opt = data.aux_metadata.read().await.clone();
    let metadata = metadata_opt.ok_or(CrackedError::NoMetadata)?;

    // // Extract playlist name and track ID from the arguments
    let guild_id_i64 = guild_id.get() as i64;
    let channel_id = ctx.channel_id().get() as i64;
    let track = match metadata.track.clone() {
        Some(track) => track,
        None => metadata.title.clone().ok_or(CrackedError::NoTrackName)?,
    };
    let user_id = ctx.author().id.get() as i64;
    // Database pool to execute queries
    let db_pool: PgPool = ctx.get_db_pool()?;

    // Get playlist if exists, other create it.
    let target_pl = match Playlist::get_playlist_by_name(&db_pool, playlist.clone(), user_id).await
    {
        Ok(playlist) => Ok(playlist),
        Err(e) => {
            tracing::error!("Error getting playlist: {:?}", e);
            tracing::info!("Creating playlist: {:?}", playlist);
            Playlist::create(&db_pool, &playlist, user_id).await
        },
    }?;

    let MetadataAnd::Track(in_metadata, _) =
        aux_metadata_to_db_structures(&metadata, guild_id_i64, channel_id)?;

    let metadata = Metadata::get_or_create(&db_pool, &in_metadata).await?;

    let res = Playlist::add_track(
        &db_pool,
        target_pl.id,
        metadata.id,
        guild_id_i64,
        channel_id,
    )
    .await?;

    let operation_successfull = res.rows_affected() > 0;

    // Send a feedback message to the user
    if operation_successfull {
        ctx.reply(CrackedMessage::PlaylistAddSuccess { track, playlist })
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    } else {
        ctx.reply(CrackedMessage::PlaylistAddFailure { track, playlist })
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }
}
