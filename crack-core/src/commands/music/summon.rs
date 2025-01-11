use crate::commands::{cmd_check_music, do_join, help, sub_help as help};
use crate::{connection::get_voice_channel_for_user_summon, poise_ext::ContextExt, Context, Error};
use ::serenity::all::{Channel, ChannelId, Mentionable};
use crack_types::CrackedError;
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Summon the bot to your voice channel.
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    aliases("join", "come here", "comehere", "come", "here"),
    guild_only
)]
pub async fn summon(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    summon_internal(ctx, None, None).await
}

/// Summon a bot to a specific voice channel.
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    check = "cmd_check_music",
    subcommands("help"),
    guild_only
)]
pub async fn summonchannel(
    ctx: Context<'_>,
    #[description = "Channel to summon the bot to."] channel: Option<Channel>,
    #[description = "Channel Id of the channel to summon the bot to."] channel_id_str: Option<
        String,
    >,
) -> Result<(), Error> {
    summon_internal(ctx, channel, channel_id_str).await
}

/// Internal method to handle summonging the bot to a voice channel.
pub async fn summon_internal(
    ctx: Context<'_>,
    channel: Option<Channel>,
    channel_id_str: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let manager = ctx.data().songbird.clone();
    let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?.clone();
    let user_id = ctx.get_user_id();

    let channel_id = match parse_channel_id(channel, channel_id_str)? {
        Some(id) => id,
        None => get_voice_channel_for_user_summon(&guild, &user_id)?,
    };

    let call: Arc<Mutex<Call>> = match manager.get(guild_id) {
        Some(call) => {
            let handler = call.lock().await;
            let has_current_connection = handler.current_connection().is_some();
            let chan_id = handler.current_channel().map(|c| ChannelId::new(c.get()));

            match (has_current_connection, chan_id) {
                (true, Some(chan_id)) => {
                    // bot is in another channel
                    return Err(CrackedError::AlreadyConnected(chan_id.mention()).into());
                },
                _ => call.clone(),
            }
        },
        None => do_join(ctx, &manager, guild_id, channel_id).await?,
    };
    let chan_id = call
        .lock()
        .await
        .current_channel()
        .map(|c| ChannelId::new(c.get()));
    if let Some(c) = chan_id {
        tracing::info!("joined channel: {c}");
    } else {
        tracing::warn!("Not in channel after join?!?");
    }
    Ok(())
}

/// Internal method to parse the channel id.
fn parse_channel_id(
    channel: Option<Channel>,
    channel_id_str: Option<String>,
) -> Result<Option<ChannelId>, Error> {
    if let Some(channel) = channel {
        return Ok(Some(channel.id()));
    }

    match channel_id_str {
        Some(id) => {
            tracing::warn!("channel_id_str: {:?}", id);
            match id.parse::<u64>() {
                Ok(id) => Ok(Some(ChannelId::new(id))),
                Err(e) => Err(e.into()),
            }
        },
        None => Ok(None),
    }
}

#[cfg(test)]
mod test {
    use crate::commands::music::summon::parse_channel_id;
    use serenity::model::id::ChannelId;

    #[test]
    fn test_parse_channel_id() {
        let channel = None;

        assert_eq!(parse_channel_id(channel, None).unwrap(), None);
        assert_eq!(
            parse_channel_id(None, Some("123".to_string())).unwrap(),
            Some(ChannelId::new(123))
        );
        assert!(
            parse_channel_id(None, Some("abc".to_string())).is_err()
        );
        assert_eq!(parse_channel_id(None, None).unwrap(), None);
    }
}
