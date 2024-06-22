use crate::commands::{cmd_check_music, do_join, help, set_global_handlers, sub_help as help};
use crate::{
    connection::get_voice_channel_for_user_summon, errors::CrackedError, poise_ext::ContextExt,
    Context, Error,
};
use ::serenity::all::{Channel, ChannelId, Mentionable};
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
pub async fn summon_channel(
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
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
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

            if has_current_connection {
                // bot is in another channel
                let bot_channel_id: ChannelId = handler.current_channel().unwrap().0.into();
                Err(CrackedError::AlreadyConnected(bot_channel_id.mention()))
            } else {
                Ok(call.clone())
            }
        },
        None => do_join(ctx, &manager, guild_id, channel_id)
            .await
            .map_err(Into::into),
    }?;

    set_global_handlers(ctx, call, guild_id, channel_id).await?;

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
        assert_eq!(
            parse_channel_id(None, Some("abc".to_string())).is_err(),
            true
        );
        assert_eq!(parse_channel_id(None, None).unwrap(), None);
    }
}
