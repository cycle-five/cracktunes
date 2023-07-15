use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    utils::create_response_poise,
    Context, Error,
};

/// Clear the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue().current_queue();

    verify(queue.len() > 1, CrackedError::QueueEmpty)?;

    handler.queue().modify_queue(|v| {
        v.drain(1..);
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    create_response_poise(ctx, CrackedMessage::Clear).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
