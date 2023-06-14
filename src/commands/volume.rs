use self::serenity::{
    builder::CreateEmbed,
    model::application::interaction::application_command::ApplicationCommandInteraction, Context,
};
use crate::{errors::ParrotError, utils::create_embed_response};
use poise::serenity_prelude as serenity;
use songbird::tracks::TrackHandle;

pub async fn volume(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    tracing::info!("volume");
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let args = interaction.data.options.clone();
    let to_set = match args.first() {
        Some(arg) => Some(arg.value.as_ref().unwrap().as_i64().unwrap() as isize),
        None => {
            let handler = call.lock().await;
            let track_handle: TrackHandle = handler.queue().current().expect("No track playing");
            let volume = track_handle.get_info().await.unwrap().volume;
            let mut embed = CreateEmbed::default();
            embed.description(format!("Current volume is {}%", volume * 100.0));
            create_embed_response(&ctx.http, interaction, embed).await?;
            return Ok(());
        }
    };

    let handler = call.lock().await;

    let new_volume = to_set.unwrap() as f32 / 100.0;

    let track_handle: TrackHandle = handler.queue().current().expect("No track playing");

    let old_volume = track_handle.get_info().await.unwrap().volume;

    track_handle.set_volume(new_volume).unwrap();

    let embed = create_volume_embed(old_volume, new_volume);

    create_embed_response(&ctx.http, interaction, embed).await
}

pub fn create_volume_embed(old: f32, new: f32) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.description(format!("Volume changed from {}% to {}%", old, new));
    embed
}
