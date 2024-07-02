use songbird::tracks::TrackHandle;

use crate::{
    commands::cmd_check_music,
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    messaging::message::CrackedMessage,
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
    ctx.data().set_autoplay(guild_id, false).await;

    let handler = call.lock().await;
    let queue = handler.queue();
    // Do we want to return an error here or just pritn and return/?
    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    queue.stop();

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_reply(&ctx, CrackedMessage::Stop, true).await?;
    Ok(queue)
}
