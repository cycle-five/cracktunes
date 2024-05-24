use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;
use serenity::all::EditMember;
use serenity::all::{Context as SerenityContext, GuildId};
use std::sync::Arc;

/// Mute a user.
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "User to mute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let crack_msg = mute_internal(
        Arc::new(ctx.serenity_context().clone()),
        user,
        guild_id,
        true,
    )
    .await?;
    send_response_poise(ctx, crack_msg, true)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Unmute a user.
pub async fn mute_internal(
    ctx: Arc<SerenityContext>,
    user: serenity::model::user::User,
    guild_id: GuildId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
    let mention = user.mention();
    let id = user.id;
    if let Err(e) = guild_id
        .edit_member(&ctx, user.clone().id, EditMember::new().mute(mute))
        .await
    {
        // Handle error, send error message
        // send_response_poise(
        //     ctx,
        //     CrackedMessage::Other(format!("Failed to mute user: {}", e)),
        // )
        // .await
        Ok(CrackedMessage::Other(format!("Failed to mute user: {}", e)))
    } else {
        // Send success message
        Ok(CrackedMessage::UserMuted { mention, id })
    }
}
