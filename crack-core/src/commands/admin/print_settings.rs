use serenity::all::Channel;
use serenity::all::Message;
use serenity::all::User;
use serenity::http::MessagePagination;

use crate::guild::settings::GuildSettingsMap;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
// pub fn get_reply_handle(ctx: Context) -> ReplyHandle {
//     ctx.reply_handle()
// }
/// Get the current bot settings for this guild.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn print_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_settings_map = ctx.data().guild_settings_map.read().unwrap().clone(); //.unwrap().clone();

    for (guild_id, settings) in guild_settings_map.iter() {
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }

    let guild_settings_map = ctx.serenity_context().data.read().await;

    for (guild_id, settings) in guild_settings_map.get::<GuildSettingsMap>().unwrap().iter() {
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
        )
        .await?;
    }
    Ok(())
}

#[poise::command(prefix_command, owners_only, guild_only, ephemeral)]
pub async fn get_channel_messages(
    ctx: Context<'_>,
    #[description = "Channel to get messages from"] channel: Channel,
    #[description = "User to filter messages from (or mentions, or text contains)"]
    filter_user: Option<User>,
) -> Result<(), Error> {
    // let messages = channel(&ctx.serenity_context().http, |retriever| {
    //     retriever.limit(100);
    //     if let Some(user) = filter_user {
    //         retriever.after(user.id);
    //     }
    //     retriever
    // })?;
    let messages = get_messages(ctx, channel, filter_user).await?;
    tracing::warn!("messages: {:?}", messages);
    ctx.say(format!("messages: {:?}", messages.len())).await?;

    Ok(())
}

async fn get_messages(
    ctx: Context<'_>,
    channel: Channel,
    filter_user: Option<User>,
) -> Result<Vec<Message>, Error> {
    let n = 100;
    let first_step = ctx
        .serenity_context()
        .http
        .get_messages(channel.id(), None, Some(n))
        .await?;
    if first_step.len() != n as usize {
        return Ok(first_step);
    }
    let mut messages = first_step;
    let mut last_id = messages.last().unwrap().id;
    loop {
        let next_step = ctx
            .serenity_context()
            .http
            .get_messages(
                channel.id(),
                Some(MessagePagination::Before(last_id)),
                Some(n),
            )
            .await?;
        if next_step.len() != n as usize {
            messages.extend(next_step);
            break;
        } else {
            messages.extend(next_step);
            last_id = messages.last().unwrap().id;
        }
    }
    if filter_user.is_none() {
        return Ok(messages);
    } else {
        let id = filter_user.unwrap().id;
        Ok(messages.into_iter().filter(|m| m.author.id == id).collect())
    }
}
