use crate::{errors::ParrotError, utils::create_embed_response};
use serenity::{
    builder::CreateEmbed, client::Context,
    model::application::interaction::application_command::ApplicationCommandInteraction,
};

pub async fn volume(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let args = interaction.data.options.clone();
    let to_set = match args.first() {
        Some(arg) => Some(arg.value.as_ref().unwrap().as_i64().unwrap() as isize),
        None => None,
    };

    let handler = call.lock().await;

    let res = handler
        .queue()
        .current()
        .expect("No track playing")
        .set_volume(to_set.unwrap() as f32 / 100.0)
        .unwrap();
    // let new_volume = match to_set {
    //     Some(to_set) => {
    //         let new_volume = to_set as f32 / 100.0;
    //         handler
    //             .queue()
    //             .current()
    //             .set_volume(new_volume)
    //             .map_err(TrackError::Finished)
    //     }
    //     None => {
    //         return Ok(());
    //     }
    // };

    // let old_volume = handler.volume() as usize * 100;

    let embed = create_volume_embed(0, to_set.unwrap() as usize);

    create_embed_response(&ctx.http, interaction, embed).await
}

pub fn create_volume_embed(old_volume: usize, new_volume: usize) -> CreateEmbed {
    let mut embed = CreateEmbed::default();
    embed.description(format!(
        "Volume changed from {}% to {}%",
        old_volume, new_volume
    ));
    embed
}
