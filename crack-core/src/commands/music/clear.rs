use crate::{
    commands::{cmd_check_music, help},
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context, Error,
};

/// Clear the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn clear(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    clear_internal(ctx).await
}

/// Clear the queue, internal.
pub async fn clear_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = ctx.data().songbird.clone();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let queue = handler.queue().current_queue();

    verify(queue.len() > 1, CrackedError::QueueEmpty)?;

    handler.queue().modify_queue(|v| {
        v.drain(1..).for_each(|x| {
            let _ = x.stop();
            drop(x);
        });
    });

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);
    assert!(queue.len() == 1);

    send_reply(&ctx, CrackedMessage::Clear, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
