use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Create voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn create_voice_channel(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of channel to create"]
    channel_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    if let Err(e) = guild
        .create_channel(
            &ctx,
            CreateChannel::new(channel_name.clone())
                .kind(serenity::model::channel::ChannelType::Voice),
        )
        .await
    {
        // Handle error, send error message
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to create channel: {}", e)),
            true,
        )
        .await?;
    } else {
        // Send success message
        send_response_poise(
            ctx,
            CrackedMessage::VoiceChannelCreated {
                channel_name: channel_name.clone(),
            },
            true,
        )
        .await?;
    }
    Ok(())
}
