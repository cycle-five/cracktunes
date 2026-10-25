use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::{poise_ext::PoiseContextExt, CommandResult, Context};
use crack_types::discord::ToRef;
use poise::serenity_prelude as serenity;
use serenity::{User, UserId};

/// Unban a user from the guild.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_bot_permissions = "BAN_MEMBERS",
    required_permissions = "BAN_MEMBERS",
    ephemeral
)]
pub async fn unban(
    ctx: Context<'_>,
    #[description = "User to unban."] user: User,
) -> CommandResult {
    let user_id = user.id;
    unban_internal(ctx, user_id).await
}

/// Unban a user from the guild, by user id.
#[poise::command(
    category = "Admin",
    prefix_command,
    slash_command,
    required_bot_permissions = "BAN_MEMBERS",
    required_permissions = "BAN_MEMBERS",
    ephemeral
)]
pub async fn unban_by_user_id(
    ctx: Context<'_>,
    #[description = "UserId to unban"] user_id: UserId,
) -> CommandResult {
    unban_internal(ctx, user_id).await
}

/// Unban a user from the guild, by user id.
pub async fn unban_internal(ctx: Context<'_>, user_id: UserId) -> CommandResult {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let guild = guild_id.to_partial_guild(&ctx).await?;
    // Fails for an id Discord does not know, before anything is attempted.
    user_id.to_user(&ctx).await?;
    let msg = if let Err(e) = guild_id.unban(ctx.http(), user_id, None).await {
        CrackedMessage::Other(format!("Failed to unban user: {}", e))
    } else {
        CrackedMessage::UserUnbanned {
            id: user_id.to_ref(),
        }
    };

    let _ = ctx.send_reply(msg, true).await?;
    Ok(())
}
