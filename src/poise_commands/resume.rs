use crate::{
    errors::{verify, CrackedError},
    messaging::message::ParrotMessage,
    utils::{create_response_poise, get_guild_id},
    {Context, Error},
};

#[poise::command(slash_command, prefix_command)]
pub async fn resume(
    ctx: Context<'_>,
    #[description = "Resume the currently playing track"] send_reply: Option<bool>,
) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying.into())?;
    verify(
        queue.resume(),
        CrackedError::Other("Failed resuming track").into(),
    )?;

    if send_reply.unwrap_or_else(|| true) {
        return create_response_poise(&ctx, ParrotMessage::Resume).await;
    }
    return Ok(());
}
