use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;
use serenity::all::GuildId;
use serenity::all::User;
use serenity::all::UserId;

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
    let id = user.id;
    let mention = user.mention();
    if let Err(e) = guild.unban(&ctx, user.id).await {
        // Handle error, send error message
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Failed to unban user: {}", e)),
            true,
        )
        .await
        .map(|_| ())
    } else {
        // Send success message
        send_reply(&ctx, CrackedMessage::UserUnbanned { id, mention }, true)
            .await
            .map(|_| ())
    }
    .map_err(Into::into)
}
