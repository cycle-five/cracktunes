use crate::{
    errors::CrackedError,
    messaging::message::CrackedMessage,
    messaging::messages::FAIL_LOOP,
    utils::{create_response, get_interaction},
    Context, Error,
};
use songbird::tracks::{LoopState, TrackHandle};

/// Toggle looping of the current track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn repeat(ctx: Context<'_>) -> Result<(), Error> {
    let mut interaction = get_interaction(ctx).unwrap();
    let guild_id = interaction.guild_id.unwrap();
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

    match toggler(&track) {
        Ok(_) if was_looping => {
            create_response(
                &ctx.serenity_context().http,
                &mut interaction,
                CrackedMessage::LoopDisable,
            )
            .await
        }
        Ok(_) if !was_looping => {
            create_response(
                &ctx.serenity_context().http,
                &mut interaction,
                CrackedMessage::LoopEnable,
            )
            .await
        }
        _ => Err(CrackedError::Other(FAIL_LOOP).into()),
    }
}
