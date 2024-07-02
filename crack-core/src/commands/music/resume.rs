use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    utils::send_reply,
    {Context, Error},
};

/// Resume the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Music", slash_command, prefix_command, guild_only)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.resume(), CrackedError::Other("Failed resuming track"))?;

    send_reply(&ctx, CrackedMessage::Resume, false).await?;

    Ok(())
}
