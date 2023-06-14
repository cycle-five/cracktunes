use self::serenity::{
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{
    errors::{verify, CrackedError},
    messaging::message::ParrotMessage,
    utils::create_response,
    Error,
};
use poise::serenity_prelude as serenity;

pub async fn pause(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), Error> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let queue = handler.queue();

    verify(!queue.is_empty(), CrackedError::NothingPlaying)?;
    verify(queue.pause(), CrackedError::Other("Failed to pause"))?;

    create_response(&ctx.http, interaction, ParrotMessage::Pause).await
}
