use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_response_poise, Context,
    Error,
};

const CHAT_CLEANUP_SECONDS: u64 = 15; // 60 * 60 * 24 * 7;

/// Clean up old messages from the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn clean(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let time_ordered_messages = {
        let mut message_cache = ctx.data().guild_msg_cache_ordered.lock().unwrap();
        &mut message_cache
            .get_mut(&guild_id)
            .ok_or(CrackedError::Other("No messages in cache"))?
            .time_ordered_messages
            .clone()
    };
    let reply_handle = ctx.say("Cleaning up old messages...").await?;
    let mut status_msg = reply_handle.into_message().await?;
    let mut deleted = 0;
    let message_ttl = chrono::Duration::try_seconds(CHAT_CLEANUP_SECONDS as i64)
        .ok_or("Chat cleanup seconds not a number??!?")?;
    tracing::warn!("message_ttl: {:?}", message_ttl);
    while let Some(msg) = time_ordered_messages.pop_last() {
        let now = chrono::Utc::now();
        let diff = now - msg.0;
        tracing::warn!("diff {}", msg.1.id);
        if diff > message_ttl {
            deleted += 1;
            status_msg
                .edit(
                    &ctx.serenity_context(),
                    serenity::builder::EditMessage::default().content(format!(
                        "Deleting message {}\nDeleted so far: {}",
                        msg.1.id, deleted
                    )),
                )
                .await?;
            tracing::warn!("Deleting message {}", msg.1.id);
            msg.1.delete(&ctx.serenity_context()).await?;
        } else {
            time_ordered_messages.insert(msg.0, msg.1);
            break;
        }
    }

    status_msg.delete(&ctx.serenity_context()).await?;
    send_response_poise(ctx, CrackedMessage::Clean(deleted), true).await?;
    Ok(())
}
