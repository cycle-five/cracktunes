use crate::{
    commands::{cmd_check_music, sub_help as help},
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    utils::send_response_poise,
    Context, Error,
};

/// Clear the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music",
    subcommands("help")
)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    clear_internal(ctx).await
}

/// Clear the queue, internal.
pub async fn clear_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let queue = handler.queue().current_queue();

    verify(queue.len() > 1, CrackedError::QueueEmpty)?;

    handler.queue().modify_queue(|v| {
        v.drain(1..);
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_response_poise(ctx, CrackedMessage::Clear, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
