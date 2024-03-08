use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Undeafen a user.
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn undeafen(
    ctx: Context<'_>,
    #[description = "User to undeafen"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    if let Err(e) = guild
        .edit_member(&ctx, user.clone().id, EditMember::new().deafen(false))
        .await
    {
        // Handle error, send error message
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to undeafen user: {}", e)),
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
    Ok(())
}
