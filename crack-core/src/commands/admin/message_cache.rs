use crate::commands::invite_tracker::kv_iter_to_string;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise_text;
use crate::Context;
use crate::Error;

/// Get the message cache.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn message_cache(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let message_cache = ctx.data().guild_msg_cache_ordered.lock().unwrap().clone();
    let cache_str = kv_iter_to_string(
        message_cache
            .get(&guild_id)
            .unwrap()
            .time_ordered_messages
            .iter()
            .map(|(key, value)| (key, value.content.clone())),
    );

    tracing::warn!("message_cache: {}", cache_str);

    let msg = send_response_poise_text(ctx, CrackedMessage::Other(cache_str)).await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
