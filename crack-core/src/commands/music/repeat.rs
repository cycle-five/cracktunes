use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, messaging::messages::FAIL_LOOP,
    utils::send_reply, Context, Error,
};
use songbird::tracks::{LoopState, TrackHandle};

/// Toggle looping of the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn repeat(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let track = handler.queue().current().unwrap();
    drop(handler);

    let was_looping = track.get_info().await.unwrap().loops == LoopState::Infinite;
    let toggler = if was_looping {
        TrackHandle::disable_loop
    } else {
        TrackHandle::enable_loop
    };

    let _ = match toggler(&track) {
        Ok(_) if was_looping => send_reply(&ctx, CrackedMessage::LoopDisable, true).await,
        Ok(_) if !was_looping => send_reply(&ctx, CrackedMessage::LoopEnable, true).await,
        _ => Err(CrackedError::Other(FAIL_LOOP)),
    }?;
    Ok(())
}
