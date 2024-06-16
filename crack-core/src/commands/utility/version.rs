use crate::guild::operations::GuildSettingsOperations;
use crate::{
    messaging::{message::CrackedMessage, messages::UNKNOWN},
    utils::send_reply,
    Context, Error,
};

/// Get the build version of this bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, prefix_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    version_internal(ctx).await
}

/// Get the build version of this bot, internal function.
pub async fn version_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let reply_with_embed = ctx.data().get_reply_with_embed(guild_id).await;
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| UNKNOWN);
    let hash = option_env!("GIT_HASH").unwrap_or_else(|| UNKNOWN);
    let _ = send_reply(
        &ctx,
        CrackedMessage::Version {
            current: current.to_owned(),
            hash: hash.to_owned(),
        },
        reply_with_embed,
    )
    .await?;
    Ok(())
}
