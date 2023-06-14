use self::serenity::{
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{
    errors::{verify, ParrotError},
    messaging::message::ParrotMessage,
    utils::create_response,
};
use poise::serenity_prelude as serenity;

pub async fn resume(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), ParrotError::NothingPlaying)?;
    verify(queue.resume(), ParrotError::Other("Failed resuming track"))?;

    create_response(&ctx.http, interaction, ParrotMessage::Resume).await
}
