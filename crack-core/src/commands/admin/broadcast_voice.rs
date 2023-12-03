use serenity::builder::CreateMessage;

use crate::utils::get_current_voice_channel_id;
use crate::Context;
use crate::Error;

/// Broadcast a message to all guilds where the bot is currently in a voice channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn broadcast_voice(
    ctx: Context<'_>,
    #[rest]
    #[description = "The message to broadcast"]
    message: String,
) -> Result<(), Error> {
    let data = ctx.data();
    let http = ctx.http();
    let serenity_ctx = ctx.serenity_context().clone();
    let guilds = data.guild_settings_map.read().unwrap().clone();

    for (guild_id, _settings) in guilds.iter() {
        let message = message.clone();

        let channel_id_opt = get_current_voice_channel_id(&serenity_ctx, *guild_id).await;

        if let Some(channel_id) = channel_id_opt {
            channel_id
                .send_message(&http, CreateMessage::new().content(message.clone()))
                .await
                .unwrap();
        }
    }

    Ok(())
}
