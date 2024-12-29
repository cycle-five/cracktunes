use crate::{
    commands::cmd_check_music, messaging::message::CrackedMessage, utils::send_reply, Context,
    Error,
};
use crack_types::messaging::messages::FAIL_LOOP;
use crack_types::CrackedError;
use songbird::tracks::{LoopState, TrackHandle};

/// Toggle looping of the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn repeat(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    repeat_internal(ctx).await
}

/// Internal repeat function.
#[cfg(not(tarpaulin_include))]
pub async fn repeat_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let songbird = ctx.data().songbird.clone();
    let call = songbird.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let handler = call.lock().await;
    let track = match handler.queue().current() {
        Some(track) => track,
        None => return Err(Box::new(CrackedError::NothingPlaying)),
    };
    drop(handler);

    let was_looping = track.get_info().await?.loops == LoopState::Infinite;
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
