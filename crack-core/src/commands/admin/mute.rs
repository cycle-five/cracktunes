use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Mute a user.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "User to mute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().mute(true))
                .await
            {
                // Handle error, send error message
                send_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to mute user: {}", e)),
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
