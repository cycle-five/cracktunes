use crate::{
    errors::ParrotError,
    utils::{create_embed_response, create_now_playing_embed},
};
use serenity::{
    client::Context,
    model::application::interaction::application_command::ApplicationCommandInteraction,
};

pub async fn grab(
    ctx: &Context,
    interaction: &mut ApplicationCommandInteraction,
) -> Result<(), ParrotError> {
    tracing::info!("grab");
    let guild_id = interaction.guild_id.unwrap();
    let manager = songbird::get(ctx).await.unwrap();
    let call = manager.get(guild_id).unwrap();
    let channel = interaction.user.create_dm_channel(&ctx.http).await?;
    let handler = call.lock().await;

    match handler.queue().current() {
        Some(track_handle) => {
            // let track = track_handle.get_info().await.unwrap();
            let embed = create_now_playing_embed(&track_handle).await;
            create_embed_response(&ctx.http, interaction, embed).await?;
        }
        None => {
            channel
                .say(&ctx.http, "Nothing playing!")
                .await
                .expect("Error sending message");
        }
    }

    interaction
        .delete_original_interaction_response(&ctx.http)
        .await?;

    return Ok(());
}
