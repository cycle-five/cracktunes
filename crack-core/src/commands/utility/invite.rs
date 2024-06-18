use crate::{
    commands::CrackedError,
    messaging::messages::{INVITE_LINK_TEXT, INVITE_TEXT, INVITE_URL},
    Context, Error,
};
use poise::serenity_prelude::GuildId;

/// Invite link for the bot
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, prefix_command)]
pub async fn invite(ctx: Context<'_>) -> Result<(), Error> {
    invite_internal(ctx).await
}

/// Testable internal function for invite.
pub async fn invite_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: GuildId = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let reply_handle = ctx
        .reply(format!(
            "{} [{}]({})",
            INVITE_TEXT, INVITE_LINK_TEXT, INVITE_URL
        ))
        .await?;

    let msg = reply_handle.into_message().await?;

    ctx.data().add_msg_to_cache(guild_id, msg).await;

    Ok(())
}
