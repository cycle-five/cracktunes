use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    utils::send_reply,
    {Context, Error},
};

/// Pause the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.pause(), CrackedError::Other("Failed to pause"))?;

    send_reply(&ctx, CrackedMessage::Pause, true).await?;
    Ok(())
}
