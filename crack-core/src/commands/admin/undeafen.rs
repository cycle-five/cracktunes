use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Undeafen a user.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn undeafen(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            if let Err(e) = guild
                .edit_member(&ctx, user.clone().id, EditMember::new().deafen(false))
                .await
            {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to undeafen user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
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
