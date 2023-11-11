use serenity::all::User;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;

/// Ban a user from the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn ban(
    ctx: Context<'_>,
    user: User,
    dmd: Option<u8>,
    reason: Option<String>,
) -> Result<(), Error> {
    let dmd = dmd.unwrap_or(0);
    let reason = reason.unwrap_or("No reason provided".to_string());
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.ban_with_reason(&ctx, user.clone(), dmd, reason).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to ban user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserBanned {
                        user: user.name.clone(),
                        user_id: user.clone().id,
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
