use songbird::tracks::TrackHandle;

use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    messaging::message::CrackedMessage,
    music::{queue::stop_queue, PlaybackOwner},
    poise_ext::ContextExt,
    utils::send_reply,
    Context, Error,
};

/// Stop the current track and clear the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    stop_internal(ctx).await?;
    Ok(())
}

/// The return vector from this should be empty.
#[cfg(not(tarpaulin_include))]
pub async fn stop_internal(ctx: Context<'_>) -> Result<Vec<TrackHandle>, Error> {
    let (call, guild_id) = ctx.get_call_guild_id().await?;
    let _ = ctx.data().set_autoplay(guild_id, false).await;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    // Do we want to return an error here or just pritn and return/?
    verify(!handler.queue().is_empty(), CrackedError::NothingPlaying)?;
    stop_queue(&guard, &handler);

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);
    // `stop_queue` fires `TrackEvent::End`, and the global track-end handler
    // awaits `lock_queue` for autopause -- so the guard goes before the reply
    // below, not after it. See `stop_queue`.
    drop(guard);

    send_reply(&ctx, CrackedMessage::Stop, true).await?;
    Ok(queue)
}
