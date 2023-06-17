use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::ParrotMessage,
    utils::{create_response_poise_text, get_guild_id},
    Context, Error,
};

#[poise::command(slash_command, prefix_command)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    queue.stop();

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    create_response_poise_text(&ctx, ParrotMessage::Stop).await?;
    update_queue_messages(
        &ctx.serenity_context().http,
        &ctx.serenity_context().data,
        &queue,
        guild_id,
    )
    .await;
    Ok(())
}
