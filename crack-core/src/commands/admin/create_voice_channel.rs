use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;

/// Create voice channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create_voice_channel(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of channel to create"]
    channel_name: String,
) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild
                .create_channel(
                    &ctx,
                    CreateChannel::new(channel_name.clone())
                        .kind(serenity::model::channel::ChannelType::Voice),
                )
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::VoiceChannelCreated {
                        channel_name: channel_name.clone(),
                    },
                )
                .await?;
            }
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    }
    Ok(())
}
