use crate::{
    commands::{cmd_check_music, help},
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    music::{clear_from, PlaybackOwner},
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

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();

    verify(queue.len() > 1, CrackedError::QueueEmpty)?;

    clear_from(&guard, &handler, 1);
    // The guard is held only for the mutation, not across the Discord round
    // trips below (`send_reply`, `update_queue_messages`) -- see lease.rs.
    drop(guard);

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);
    debug_assert!(queue.len() == 1);

    send_reply(&ctx, CrackedMessage::Clear, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
