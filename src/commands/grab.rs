use crate::{errors::ParrotError, utils::create_embed_response};
use serenity::{
    builder::CreateEmbed, client::Context,
    model::application::interaction::application_command::ApplicationCommandInteraction,
};
use songbird::tracks::TrackHandle;

pub async fn grab(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    tracing::info!("grab");
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();
    let user = interaction.user.create_dm_channel(&ctx.http).await?;

    return Ok(());

    // let embed = create_volume_embed(old_volume, new_volume);

    // create_embed_response(&ctx.http, interaction, embed).await
}

pub fn create_volume_embed(old: f32, new: f32) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.description(format!("Volume changed from {}% to {}%", old, new));
    embed
}
