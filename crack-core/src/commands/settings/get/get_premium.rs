use crate::guild::operations::GuildSettingsOperations;
use crate::poise_ext::PoiseContextExt;
use crate::CrackedError;
use crate::CrackedMessage;
use crate::{Context, Error};

/// Get the current `premium` setting for the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    aliases("get_premium_status")
)]
pub async fn premium(ctx: Context<'_>) -> Result<(), Error> {
    premium_internal(ctx).await
}

/// Get the current `premium` setting for the guild.
pub async fn premium_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let res = data.get_premium(guild_id).await.unwrap_or(false);

    ctx.send_reply(CrackedMessage::Premium(res), true)
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
