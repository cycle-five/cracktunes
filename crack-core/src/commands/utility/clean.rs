use crate::{
    //commands::sub_help as help,
    errors::CrackedError,
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context,
    Error,
};

const CHAT_CLEANUP_SECONDS: u64 = 15; // 60 * 60 * 24 * 7;

/// Clean up old messages from the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Utility",
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "MANAGE_MESSAGES",
    required_bot_permissions = "MANAGE_MESSAGES",
    //subcommands("help")
)]
pub async fn clean(ctx: Context<'_>) -> Result<(), Error> {
    clean_internal(ctx).await
}

/// Clean up old messages from the bot, internal fucntion.
pub async fn clean_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let time_ordered_messages = {
        &mut data
            .id_cache_map
            .get_mut(&guild_id.into())
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
            msg.1.delete(ctx.http(), None).await?;
        } else {
            time_ordered_messages.insert(msg.0, msg.1);
            break;
        }
    }

    status_msg.delete(ctx.http(), None).await?;
    send_reply(&ctx, CrackedMessage::Clean(deleted), true).await?;
    Ok(())
}
