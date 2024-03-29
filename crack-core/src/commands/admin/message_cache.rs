use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise_text;
use crate::Context;
use crate::Error;

/// Get the message cache.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn message_cache(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let cache_str = {
        let mut message_cache = ctx.data().guild_msg_cache_ordered.lock().unwrap().clone();
        message_cache
            .entry(guild_id)
            .or_default()
            .time_ordered_messages
            .len()
            .to_string()
    };

    tracing::warn!("message_cache: {}", cache_str);

    let msg = send_response_poise_text(ctx, CrackedMessage::Other(cache_str)).await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
