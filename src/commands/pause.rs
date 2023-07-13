use crate::{
    errors::{verify, CrackedError},
    is_prefix,
    messaging::message::CrackedMessage,
    utils::{count_command, create_response_poise_text, get_guild_id},
    {Context, Error},
};

/// Pause the current track.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn pause(
    ctx: Context<'_>,
    #[description = "Pause the currently playing track"] send_reply: Option<bool>,
) -> Result<(), Error> {
    count_command("pause", is_prefix(ctx));
    let guild_id = get_guild_id(&ctx).unwrap();
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
