use serenity::all::GuildId;
use serenity::all::User;
use serenity::all::UserId;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Unban command
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn unban(
    ctx: Context<'_>,
    #[description = "User to unban"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    unban_helper(ctx, guild_id, user).await
}

/// Unban a user from the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn unban_by_user_id(
    ctx: Context<'_>,
    #[description = "UserId to unban"] user_id: UserId,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    unban_helper(ctx, guild_id, user_id.to_user(ctx).await?).await
}

/// Unban a user from the server.
#[cfg(not(tarpaulin_include))]
pub async fn unban_helper(ctx: Context<'_>, guild_id: GuildId, user: User) -> Result<(), Error> {
    let guild = guild_id.to_partial_guild(&ctx).await?;
    if let Err(e) = guild.unban(&ctx, user.id).await {
        // Handle error, send error message
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to unban user: {}", e)),
            true,
        )
        .await
        .map(|m| ctx.data().add_msg_to_cache(guild_id, m))
        .map(|_| ())
    } else {
        // Send success message
        send_response_poise(
            ctx,
            CrackedMessage::UserUnbanned {
                user: user.name.clone(),
                user_id: user.id,
            },
            true,
        )
        .await
        .map(|m| ctx.data().add_msg_to_cache(guild_id, m))
        .map(|_| ())
    }
    .map_err(Into::into)
}
