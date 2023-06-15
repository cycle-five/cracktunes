use self::serenity::{
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{
    errors::CrackedError, messaging::message::ParrotMessage, messaging::messages::FAIL_LOOP,
    utils::create_response, Error,
};
use poise::serenity_prelude as serenity;
use songbird::tracks::{LoopState, TrackHandle};

pub async fn repeat(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), Error> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
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
            create_response(&ctx.http, interaction, ParrotMessage::LoopDisable).await
        }
        Ok(_) if !was_looping => {
            create_response(&ctx.http, interaction, ParrotMessage::LoopEnable).await
        }
        _ => Err(CrackedError::Other(FAIL_LOOP).into()),
    }
}
