use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    utils::send_response_poise_text,
    {Context, Error},
};

/// Resume the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn resume(
    ctx: Context<'_>,
    #[description = "Resume the currently playing track"] _send_reply: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.resume(), CrackedError::Other("Failed resuming track"))?;

    // FIXME: Do we want to do the send_reply parameter?
    let msg = send_response_poise_text(ctx, CrackedMessage::Resume).await?;

    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
