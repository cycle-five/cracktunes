use std::sync::Arc;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::EditMember;
use serenity::all::{Context as SerenityContext, GuildId};

/// Mute a user.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "User to mute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let crack_msg = mute_impl(
        Arc::new(ctx.serenity_context().clone()),
        user,
        guild_id,
        true,
    )
    .await?;
    send_response_poise(ctx, crack_msg).await.map(|_| ())
}

/// Unmute a user.
pub async fn mute_impl(
    ctx: Arc<SerenityContext>,
    user: serenity::model::user::User,
    guild_id: GuildId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
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
        Ok(CrackedMessage::UserMuted {
            user: user.name.clone(),
            user_id: user.clone().id,
        })

        // send_response_poise(
        //     ctx,
        //     CrackedMessage::UserMuted {
        //         user: user.name.clone(),
        //         user_id: user.clone().id,
        //     },
        // )
        // .await
    }
}
