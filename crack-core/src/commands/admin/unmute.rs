use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::Message;
use serenity::builder::EditMember;

/// Unmute a user.
/// TODO: Add a way to unmute a user by their ID.
#[poise::command(prefix_command, owners_only, guild_only, ephemeral)]
pub async fn unmute(
    ctx: Context<'_>,
    #[description = "User of unmute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    unmute_impl(ctx, user).await.map(|_| ())
}

pub async fn unmute_impl(
    ctx: Context<'_>,
    user: serenity::model::user::User,
) -> Result<Message, Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or(CrackedError::Other("Guild ID not found"))?;
    if let Err(e) = guild_id
        .edit_member(ctx, user.clone().id, EditMember::new().mute(false))
        .await
    {
        // Handle error, send error message
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to unmute user: {}", e)),
        )
        .await
    } else {
        // Send success message
        send_response_poise(
            ctx,
            CrackedMessage::UserUnmuted {
                user: user.name.clone(),
                user_id: user.clone().id,
            },
        )
        .await
    }
}
