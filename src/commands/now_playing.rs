use self::serenity::{
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{
    errors::CrackedError,
    utils::{create_embed_response, create_now_playing_embed},
    Error,
};
use poise::serenity_prelude as serenity;

pub async fn now_playing(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), Error> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;

    let embed = create_now_playing_embed(&track).await;
    create_embed_response(&ctx.http, interaction, embed).await
}
