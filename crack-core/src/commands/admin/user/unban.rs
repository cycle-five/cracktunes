use crate::commands::sub_help as help;
use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{CommandResult, Context};
use poise::serenity_prelude as serenity;
use serenity::{Mentionable, User, UserId};

/// Unban a user from the guild.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    subcommands("help"),
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
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
    subcommands("help"),
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn unban_by_user_id(
    ctx: Context<'_>,
    #[description = "UserId to unban"] user_id: UserId,
) -> CommandResult {
    unban_internal(ctx, user_id).await
}

/// Unban a user from the guild, by user id.
#[cfg(not(tarpaulin_include))]
pub async fn unban_internal(ctx: Context<'_>, user_id: UserId) -> CommandResult {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    let user = user_id.to_user(&ctx).await?;
    let mention = user.mention();
    if let Err(e) = guild.unban(&ctx, user_id).await {
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
        send_reply(
            &ctx,
            CrackedMessage::UserUnbanned {
                id: user_id,
                mention,
            },
            true,
        )
        .await
        .map(|_| ())
    }
    .map_err(Into::into)
}
