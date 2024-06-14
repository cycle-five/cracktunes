use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::CrackedError;
use crate::Error;

/// Get the message cache.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn message_cache(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let cache_str = {
        let mut message_cache = ctx.data().guild_msg_cache_ordered.lock().await.clone();
        message_cache
            .entry(guild_id)
            .or_default()
            .time_ordered_messages
            .len()
            .to_string()
    };

    tracing::warn!("message_cache: {}", cache_str);

    send_reply(ctx, CrackedMessage::Other(cache_str), false).await?;

    Ok(())
}
