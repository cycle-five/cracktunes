use serenity::builder::CreateChannel;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;

/// Create text channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn create_text_channel(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of text channel to create"]
    channel_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    match guild_id
        .create_channel(
            ctx.http(),
            CreateChannel::new(channel_name.clone())
                .kind(serenity::model::channel::ChannelType::Text),
        )
        .await
    {
        Err(e) => {
            // Handle error, send error message
            send_reply(
                &ctx,
                CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                true,
            )
            .await?;
        },
        Ok(channel) => {
            // Send success message
            send_reply(
                &ctx,
                CrackedMessage::TextChannelCreated {
                    channel_name: channel.name,
                    channel_id: channel.id,
                },
                true,
            )
            .await?;
        },
    }
    Ok(())
}

/// Create category.
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn create_category(
    ctx: Context<'_>,
    #[rest]
    #[description = "Name of the category to create"]
    category_name: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    match guild_id
        .create_channel(
            ctx.http(),
            CreateChannel::new(category_name.clone())
                .kind(serenity::model::channel::ChannelType::Category),
        )
        .await
    {
        Err(e) => {
            // Handle error, send error message
            send_reply(
                &ctx,
                CrackedMessage::Other(format!("Failed to create channel: {}", e)),
                true,
            )
            .await?;
        },
        Ok(channel) => {
            // Send success message
            send_reply(
                &ctx,
                CrackedMessage::TextChannelCreated {
                    channel_name: channel.name,
                    channel_id: channel.id,
                },
                true,
            )
            .await?;
        },
    }
    Ok(())
}
