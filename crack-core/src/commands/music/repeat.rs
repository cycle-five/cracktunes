use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, messaging::messages::FAIL_LOOP,
    utils::send_response_poise, Context, Error,
};
use songbird::tracks::{LoopState, TrackHandle};

/// Toggle looping of the current track.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn repeat(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let track = handler.queue().current().unwrap();

    let was_looping = track.get_info().await.unwrap().loops == LoopState::Infinite;
    let toggler = if was_looping {
        TrackHandle::disable_loop
    } else {
        TrackHandle::enable_loop
    };

    let msg = match toggler(&track) {
        Ok(_) if was_looping => send_response_poise(ctx, CrackedMessage::LoopDisable).await,
        Ok(_) if !was_looping => send_response_poise(ctx, CrackedMessage::LoopEnable).await,
        _ => Err(CrackedError::Other(FAIL_LOOP).into()),
    }?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}
