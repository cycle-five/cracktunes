use crate::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

/// Clean up old messages from the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn clean(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut message_cache = ctx.data().guild_msg_cache_ordered.lock().unwrap().clone();
    while let Some(msg) = message_cache
        .entry(guild_id)
        .or_default()
        .time_ordered_messages
        .pop_last()
    {
        let now = chrono::Utc::now();
        let diff = now - msg.0;
        if diff > chrono::Duration::seconds(10 * 60) {
            msg.1.delete(&ctx.serenity_context()).await?;
        } else {
            message_cache
                .entry(guild_id)
                .or_default()
                .time_ordered_messages
                .insert(msg.0, msg.1);
            break;
        }
    }

    send_response_poise(ctx, CrackedMessage::Clean).await?;
    Ok(())
}
