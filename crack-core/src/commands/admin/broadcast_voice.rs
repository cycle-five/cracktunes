use crate::{poise_ext::ContextExt, Context, Error};
use serenity::all::CreateMessage;

/// Broadcast a message to all guilds where the bot is currently in a voice channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn broadcast_voice(
    ctx: Context<'_>,
    #[rest]
    #[description = "The message to broadcast"]
    message: String,
) -> Result<(), Error> {
    let guilds = ctx.data().guild_settings_map.read().await.clone();

    for (guild_id, _settings) in guilds.iter() {
        match ctx.get_active_channel_id(*guild_id).await {
            Some(channel_id) => {
                let _ = channel_id
                    .send_message(ctx.http(), CreateMessage::new().content(message.clone()))
                    .await;
            },
            None => tracing::warn!("No active channel for guild_id: {}", guild_id),
        };
    }

    Ok(())
}
