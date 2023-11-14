use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::UserId;

/// Kick command to kick a user from the server based on their ID
#[poise::command(prefix_command, ephemeral, owners_only)]
pub async fn kick(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.kick(&ctx, user_id).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to kick user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(ctx, CrackedMessage::UserKicked { user_id }).await?;
            }
        }
        None => {
            create_response_poise(
                ctx,
                CrackedMessage::Other("This command can only be used in a guild.".to_string()),
            )
            .await?;
        }
    }
    Ok(())
}
