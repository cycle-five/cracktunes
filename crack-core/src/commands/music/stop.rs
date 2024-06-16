use crate::{
    errors::{verify, CrackedError},
    guild::operations::GuildSettingsOperations,
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context, Error,
};

/// Stop the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    ctx.data().set_autoplay(guild_id, false).await;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    // Do we want to return an error here or just pritn and return/?
    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    queue.stop();

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    send_reply(&ctx, CrackedMessage::Stop, true).await?;
    Ok(())
}
