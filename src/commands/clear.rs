use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    is_prefix,
    messaging::message::CrackedMessage,
    utils::{count_command, create_response, get_interaction},
    Context, Error,
};

/// Clear the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    count_command("clear", is_prefix(ctx));
    let mut interaction = get_interaction(ctx).unwrap();

    let guild_id = interaction.guild_id.unwrap();
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

    create_response(
        &ctx.serenity_context().http,
        &mut interaction,
        CrackedMessage::Clear,
    )
    .await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
