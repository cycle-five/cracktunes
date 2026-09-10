use crate::poise_ext::ContextExt;
use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    music::{drain_after_current, PlaybackOwner, QueueGuard},
    poise_ext::PoiseContextExt,
    utils::get_track_handle_metadata,
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
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Number of tracks to skip"] num_tracks: Option<u32>,
) -> Result<(), Error> {
    let (call, guild_id) = ctx.get_call_guild_id().await?;
    let to_skip = num_tracks.unwrap_or(1) as usize;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;

    let tracks_to_skip = min(to_skip, queue.len());

    drain_after_current(&guard, &handler, tracks_to_skip.saturating_sub(1));

    force_skip_top_track(&guard, &handler).await?;
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
            let metadata = get_track_handle_metadata(&track).await?;
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
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn downvote(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;

    let call = get_call_or_join_author(ctx).await?;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let queue = handler.queue();
    let metadata = get_track_handle_metadata(&queue.current().unwrap()).await?;

    let source_url = &metadata.source_url.ok_or("ASDF").unwrap();
    let res1 = ctx.data().downvote_track(guild_id, source_url).await?;
    let res2 = force_skip_top_track(&guard, &handler).await?;

    tracing::warn!("downvoted track: {:#?}", res1);
    tracing::warn!("refetched queue: {:#?}", res2);

    Ok(())
}

/// Do the actual skipping of the top track.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[cfg(not(tarpaulin_include))]
pub async fn force_skip_top_track(
    guard: &QueueGuard,
    handler: &MutexGuard<'_, Call>,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let _ = guard;
    // this is an odd sequence of commands to ensure the queue is properly updated
    // apparently, skipping/stopping a track takes a while to remove it from the queue
    // also, manually removing tracks doesn't trigger the next track to play
    // so first, stop the top song, manually remove it and then resume playback
    handler.queue().current().unwrap().stop().ok();
    let _ = handler.queue().dequeue(0);
    handler.queue().resume().ok();

    Ok(handler.queue().current_queue())
}
