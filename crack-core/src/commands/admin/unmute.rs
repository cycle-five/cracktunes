use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;
use serenity::builder::EditMember;

/// Unmute a user.
/// TODO: Add a way to unmute a user by their ID.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    guild_only,
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn unmute(
    ctx: Context<'_>,
    #[description = "User of unmute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    unmute_impl(ctx, user).await.map(|_| ())
}

/// Unmute a user
/// impl for other internal use.
#[cfg(not(tarpaulin_include))]
pub async fn unmute_impl(ctx: Context<'_>, user: serenity::model::user::User) -> Result<(), Error> {
    let id = user.id;
    let mention = user.mention();
    let guild_id = ctx
        .guild_id()
        .ok_or(CrackedError::Other("Guild ID not found"))?;
    if let Err(e) = guild_id
        .edit_member(&ctx, user.clone().id, EditMember::new().mute(false))
        .await
    {
        // Handle error, send error message
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Failed to unmute user: {}", e)),
            true,
        )
        .await
    } else {
        // Send success message
        send_reply(&ctx, CrackedMessage::UserUnmuted { id, mention }, true).await
    }?;
    Ok(())
}
