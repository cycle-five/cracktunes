use crate::{
    commands::cmd_check_music,
    errors::CrackedError,
    messaging::message::CrackedMessage,
    music::ops::{self, OpCx},
    utils::send_reply,
    Context, Error,
};

/// Stop the current track and clear the queue.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let cx = OpCx::from_ctx(&ctx)?;
    // A guild a game owns refuses here (GameInProgress), the same refusal
    // GP_BLOCKED_COMMANDS gives earlier and more kindly.
    let done = ops::stop(&cx).await.map_err(CrackedError::from)?;
    // The lease is gone by now: `stop_queue` fires `TrackEvent::End`, and the
    // track-end handler awaits `lock_queue`, so the reply must not hold it.
    send_reply(&ctx, CrackedMessage::Stop, true).await?;
    // Idempotent with the track end `stop_queue` fires: both land on Finished.
    done.settle_now(&cx).await;
    Ok(())
}
