use crate::{
    commands::{cmd_check_music, sub_help as help},
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    poise_ext::PoiseContextExt,
    utils::get_track_metadata,
    Context, Error,
};
use serenity::all::Message;
use songbird::{tracks::TrackHandle, Call};
use std::cmp::min;
use tokio::sync::MutexGuard;

/// Skip the current track, or a number of tracks.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    check = "cmd_check_music",
    subcommands("help"),
    guild_only
)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Number of tracks to skip"] num_tracks: Option<usize>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = match manager.get(guild_id) {
        Some(call) => call,
        None => {
            tracing::warn!(
                "Not in voice channel: manager.get({}) returned None",
                guild_id
            );
            return Ok(());
        },
    };

    let to_skip = num_tracks.unwrap_or(1);

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    let tracks_to_skip = min(to_skip, queue.len());

    handler.queue().modify_queue(|v| {
        v.drain(1..tracks_to_skip);
    });

    force_skip_top_track(&handler).await?;
    let msg = create_skip_response(ctx, &handler, tracks_to_skip).await?;
    ctx.data().add_msg_to_cache(guild_id, msg).await;
    Ok(())
}

/// Send the response to discord for skipping a track.
// Why don't we need to defer here?
#[cfg(not(tarpaulin_include))]
pub async fn create_skip_response(
    ctx: Context<'_>,
    handler: &MutexGuard<'_, Call>,
    tracks_to_skip: usize,
) -> Result<Message, CrackedError> {
    let send_msg = match handler.queue().current() {
        Some(track) => {
            let metadata = get_track_metadata(&track).await;
            CrackedMessage::SkipTo {
                title: metadata.title.as_ref().unwrap().to_owned(),
                url: metadata.source_url.as_ref().unwrap().to_owned(),
            }
        },
        None => {
            if tracks_to_skip > 1 {
                CrackedMessage::SkipAll
            } else {
                CrackedMessage::Skip
            }
        },
    };
    ctx.send_reply(send_msg, true)
        .await?
        .into_message()
        .await
        .map_err(|e| e.into())
}

/// Downvote and skip song causing it to *not* be used in music recommendations.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn downvote(ctx: Context<'_>) -> Result<(), Error> {
    use crate::commands::get_call_with_fail_msg;

    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;

    let call = get_call_with_fail_msg(ctx).await?;

    let handler = call.lock().await;
    let queue = handler.queue();
    let metadata = get_track_metadata(&queue.current().unwrap()).await;

    let source_url = &metadata.source_url.ok_or("ASDF").unwrap();
    let res1 = ctx.data().downvote_track(guild_id, source_url);

    let res2 = force_skip_top_track(&handler);

    tracing::warn!("downvoting track: {}", source_url);

    tokio::join!(res1, res2).0?;

    Ok(())
}

/// Do the actual skipping of the top track.
#[cfg(not(tarpaulin_include))]
pub async fn force_skip_top_track(
    handler: &MutexGuard<'_, Call>,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // this is an odd sequence of commands to ensure the queue is properly updated
    // apparently, skipping/stopping a track takes a while to remove it from the queue
    // also, manually removing tracks doesn't trigger the next track to play
    // so first, stop the top song, manually remove it and then resume playback
    handler.queue().current().unwrap().stop().ok();
    let _ = handler.queue().dequeue(0);
    handler.queue().resume().ok();

    Ok(handler.queue().current_queue())
}
