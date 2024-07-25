use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};

use poise::serenity_prelude as serenity;
use serenity::{CacheHttp, EditMember, GuildId, Mentionable};

/// Mute a user.
#[poise::command(
    category = "Admin",
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
    let crack_msg = mute_internal(&ctx, user, guild_id, true).await?;
    send_reply(&ctx, crack_msg, true)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Mute a user, internal function.
pub async fn mute_internal(
    cache_http: &impl CacheHttp,
    user: serenity::User,
    guild_id: GuildId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
    let mention = user.mention();
    let id = user.id;
    if let Err(e) = guild_id
        .edit_member(cache_http, user.clone().id, EditMember::new().mute(mute))
        .await
    {
        Ok(CrackedMessage::Other(format!("Failed to mute user: {}", e)))
    } else {
        // Send success message
        Ok(CrackedMessage::UserMuted { mention, id })
    }
}
