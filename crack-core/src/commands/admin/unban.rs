use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;

/// Unban a user from the server.
/// TODO: Add a way to unban a user by their ID.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn unban(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.unban(&ctx, user.clone()).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to unban user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserUnbanned {
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
