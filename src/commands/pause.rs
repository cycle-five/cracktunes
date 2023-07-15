use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    utils::create_response_poise_text,
    {Context, Error},
};

/// Pause the current track.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn pause(
    ctx: Context<'_>,
    #[description = "Pause the currently playing track"] send_reply: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.pause(), CrackedError::Other("Failed to pause"))?;

    if send_reply.unwrap_or(true) {
        return create_response_poise_text(&ctx, CrackedMessage::Pause).await;
    }
    Ok(())
}
