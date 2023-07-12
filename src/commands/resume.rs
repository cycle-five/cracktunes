use crate::{
    errors::{verify, CrackedError},
    messaging::message::CrackedMessage,
    metrics::COMMAND_EXECUTIONS,
    utils::{create_response_poise_text, get_guild_id},
    {Context, Error},
};

/// Resume the current track.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn resume(
    ctx: Context<'_>,
    #[description = "Resume the currently playing track"] send_reply: Option<bool>,
) -> Result<(), Error> {
    COMMAND_EXECUTIONS.with_label_values(&["resume"]).inc();
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.resume(), CrackedError::Other("Failed resuming track"))?;

    if send_reply.unwrap_or(true) {
        create_response_poise_text(&ctx, CrackedMessage::Resume).await?
    }
    Ok(())
}
