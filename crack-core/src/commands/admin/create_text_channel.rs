use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;

/// Create text channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create_text_channel(ctx: Context<'_>, channel_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            match guild
                .create_channel(
                    &ctx,
                    CreateChannel::new(channel_name.clone())
                        .kind(serenity::model::channel::ChannelType::Voice),
                )
                .await
            {
                Err(e) => {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                    )
                    .await?;
                }
                Ok(channel) => {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::TextChannelCreated {
                            channel_name: channel.name.clone(),
                            channel_id: channel.id,
                        },
                    )
                    .await?;
                }
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
