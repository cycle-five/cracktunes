use crate::{
    commands::cmd_check_music,
    errors::CrackedError,
    messaging::message::CrackedMessage,
    music::ops::{self, OpCx, Target},
    utils::send_reply,
    Context, Error,
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
    let cx = OpCx::from_ctx(&ctx)?;
    let done = ops::move_track(&cx, Target::Index(at), to)
        .await
        .map_err(CrackedError::from)?;
    send_reply(&ctx, CrackedMessage::SongMoved { at, to }, true).await?;
    done.settle_now(&cx).await;
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
    let cx = OpCx::from_ctx(&ctx)?;
    let done = ops::shuffle(&cx).await.map_err(CrackedError::from)?;
    send_reply(&ctx, done.outcome().message(), true).await?;
    done.settle_now(&cx).await;
    Ok(())
}
