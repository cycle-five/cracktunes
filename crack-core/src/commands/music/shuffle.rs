use crate::{
    commands::cmd_check_music,
    errors::verify,
    handlers::track_end::update_queue_messages,
    messaging::message::CrackedMessage,
    music::{move_track, shuffle_behind_current, PlaybackOwner},
    poise_ext::ContextExt,
    utils::send_reply,
    Context, CrackedError, Error,
};

/// Move a song in the queue to a different position.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn movesong(
    ctx: Context<'_>,
    #[description = "Index song is currently at"] at: u32,
    #[description = "Index song will be moved to"] to: u32,
) -> Result<(), Error> {
    movesong_internal(ctx, at as usize, to as usize).await
}

/// Move a song in the queue to a different position, internal function.
#[cfg(not(tarpaulin_include))]
pub async fn movesong_internal(ctx: Context<'_>, at: usize, to: usize) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let call = ctx.get_call().await?;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let len = handler.queue().current_queue().len();
    verify(
        at > 0 && at < len,
        CrackedError::Other("Index for `at` out of bounds"),
    )?;
    verify(
        to > 0 && to < len,
        CrackedError::Other("Index for `to` out of bounds"),
    )?;

    move_track(&guard, &handler, at, to);

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_reply(&ctx, CrackedMessage::SongMoved { at, to }, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}

/// Shuffle the current queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn shuffle(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let call = ctx.get_call().await?;

    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    shuffle_behind_current(&guard, &handler);

    // refetch the queue after modification
    let queue = handler.queue().current_queue();
    drop(handler);

    send_reply(&ctx, CrackedMessage::Shuffle, true).await?;
    update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    Ok(())
}
