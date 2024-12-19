use crate::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};
use serenity::{
    all::{Channel, Message, User},
    http::MessagePagination,
    nonmax::NonMaxU8,
};

#[poise::command(prefix_command, owners_only, ephemeral, hide_in_help)]
pub async fn print_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_settings_map = ctx.data().guild_settings_map.read().await.clone(); //.unwrap().clone();

    for (guild_id, settings) in guild_settings_map.iter() {
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
            true,
        )
        .await?;
    }

    // let guild_settings_map = ctx.serenity_context().data.read().await;
    //let guild_settings_map = guild_settings_map.get::<GuildSettingsMap>().unwrap();
    let guild_settings_map = ctx.data().guild_settings_map.read().await.clone();

    for (guild_id, settings) in guild_settings_map.iter() {
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Settings for guild {}: {:?}", guild_id, settings)),
            true,
        )
        .await?;
    }
    Ok(())
}

/// Get the messages from a channel wtih optional user filtering.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, guild_only, ephemeral, hide_in_help)]
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
    let n: NonMaxU8 = NonMaxU8::new(100).ok_or(anyhow::anyhow!("Invalid number of messages"))?;
    let n_usize = n.get() as usize;
    let first_step = ctx
        .serenity_context()
        .http
        .get_messages(channel.id(), None, Some(n))
        .await?;
    if first_step.len() != n_usize {
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
        if next_step.len() != n_usize {
            messages.extend(next_step);
            break;
        } else {
            messages.extend(next_step);
            last_id = messages.last().unwrap().id;
        }
    }
    if filter_user.is_none() {
        Ok(messages)
    } else {
        let id = filter_user.unwrap().id;
        Ok(messages.into_iter().filter(|m| m.author.id == id).collect())
    }
}

/// Get the print_settings commands
pub fn commands() -> Vec<crate::Command> {
    vec![print_settings(), get_channel_messages()]
        .into_iter()
        .collect()
}
