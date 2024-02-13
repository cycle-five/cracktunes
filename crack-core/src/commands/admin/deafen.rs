use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Deafen a user.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn deafen(
    ctx: Context<'_>,
    #[rest]
    #[description = "User to deafen"]
    user: serenity::model::user::User,
) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().deafen(true))
                .await
            {
                // Handle error, send error message
                send_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to deafen user: {}", e)),
                    true,
                )
                .await?;
            } else {
                // Send success message
                send_response_poise(
                    ctx,
                    CrackedMessage::UserMuted {
                        user: user.name.clone(),
                        user_id: user.clone().id,
                    },
                    true,
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
