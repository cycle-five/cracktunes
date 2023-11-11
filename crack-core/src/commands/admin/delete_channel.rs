use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;
/// Delete channel.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_channel(ctx: Context<'_>, channel_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let channel = guild
                .channels(&ctx)
                .await?
                .into_iter()
                .find(|(_channel_id, guild_chan)| guild_chan.name == channel_name);
            if let Some((channel_id, guild_chan)) = channel {
                if let Err(e) = guild_chan.delete(&ctx).await {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to delete channel: {}", e)),
                    )
                    .await?;
                } else {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::ChannelDeleted {
                            channel_id,
                            channel_name: channel_name.clone(),
                        },
                    )
                    .await?;
                }
            } else {
                create_response_poise(ctx, CrackedMessage::Other("Channel not found.".to_string()))
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
