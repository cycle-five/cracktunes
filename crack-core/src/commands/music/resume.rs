use crate::{
    messaging::message::CrackedMessage,
    utils::send_reply,
    {Context, Error},
};
use crack_types::errors::{verify, CrackedError};

/// Resume the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Music", slash_command, prefix_command, guild_only)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    resume_internal(ctx).await
}

/// Internal function to resume the current track.
pub async fn resume_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let songbird = ctx.data().songbird.clone();
    let call = songbird.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.resume(), CrackedError::FailedResume)?;

    send_reply(&ctx, CrackedMessage::Resume, true).await?;

    Ok(())
}
