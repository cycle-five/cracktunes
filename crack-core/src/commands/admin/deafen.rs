use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Deafen a user.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn deafen(
    ctx: Context<'_>,
    #[rest]
    #[description = "User to deafen"]
    user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    if let Err(e) = guild_id
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
    Ok(())
}
