use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Create text channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create_text_channel(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of text channel to create"]
    channel_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    match guild
        .create_channel(
            &ctx,
            CreateChannel::new(channel_name.clone())
                .kind(serenity::model::channel::ChannelType::Text),
        )
        .await
    {
        Err(e) => {
            // Handle error, send error message
            send_response_poise(
                ctx,
                CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                true,
            )
            .await?;
        }
        Ok(channel) => {
            // Send success message
            send_response_poise(
                ctx,
                CrackedMessage::TextChannelCreated {
                    channel_name: channel.name.clone(),
                    channel_id: channel.id,
                },
                true,
            )
            .await?;
        }
    }
    Ok(())
}

/// Create text channel.
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn create_category(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of the category to create"]
    category_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    match guild
        .create_channel(
            &ctx,
            CreateChannel::new(category_name.clone())
                .kind(serenity::model::channel::ChannelType::Category),
        )
        .await
    {
        Err(e) => {
            // Handle error, send error message
            send_response_poise(
                ctx,
                CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                true,
            )
            .await?;
        }
        Ok(channel) => {
            // Send success message
            send_response_poise(
                ctx,
                CrackedMessage::TextChannelCreated {
                    channel_name: channel.name.clone(),
                    channel_id: channel.id,
                },
                true,
            )
            .await?;
        }
    }
    Ok(())
}
