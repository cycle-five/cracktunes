use crate::commands::{cmd_check_music, do_join, help, sub_help as help, uptime_internal};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::{
    connection::get_voice_channel_for_user_summon, errors::CrackedError, poise_ext::ContextExt,
    Context, Error,
};
use ::serenity::all::{Channel, ChannelId, Mentionable};
use chrono::Duration;
use songbird::tracks::PlayMode;
use songbird::Call;
use std::borrow::Cow;
use std::sync::Arc;
use std::{fmt, fmt::Display};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default)]
pub struct BotStatus<'ctx> {
    pub name: Cow<'ctx, String>,
    pub play_mode: PlayMode,
    //pub queue: Vec<TrackHandle>,
    pub queue_len: usize,
    pub current_channel: Option<ChannelId>,
    pub uptime: Duration,
}

impl<'ctx> Display for BotStatus<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BotStatus {{\n\tplay_mode: {:?},\n\tqueue_len: {},\n\tcurrent_channel: {:?},\n\tuptime: {:?}\n\t}}",
            self.play_mode, self.queue_len, self.current_channel, self.uptime
        )
    }
}

use poise::serenity_prelude as serenity;

#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    owners_only
)]
pub async fn debug(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let _guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?.clone();
    let _user_id = ctx.get_user_id();

    // Get the voice channel we're in if any.
    let call = manager.get(guild_id);
    let mut vc_status = match call {
        Some(call) => {
            let handler = call.lock().await;
            let channel_id = handler.current_channel();
            let _is_connected = handler.current_connection().is_some();
            let queue = handler.queue();
            let track = queue.current().clone();
            let cur_user = ctx.cache().current_user().clone();
            // let name = cur_user.name.clone();
            let bot_name: Cow<'_, String> = Cow::Owned(cur_user.name.clone());

            let bot_status = match track {
                Some(track) => BotStatus {
                    name: bot_name.into(),
                    play_mode: track.get_info().await.unwrap_or_default().playing,
                    current_channel: channel_id.map(|id| serenity::ChannelId::new(id.0.into())),
                    queue_len: queue.clone().len(),
                    uptime: Duration::zero(),
                },
                None => Default::default(),
            };
            bot_status
        },
        _ => Default::default(),
    };
    let uptime = match uptime_internal(ctx).await {
        CrackedMessage::Uptime { seconds, .. } => Duration::seconds(seconds as i64),
        _ => Duration::zero(),
    };
    vc_status.uptime = uptime;

    let msg = vc_status.to_string();
    ctx.send_reply(CrackedMessage::Other(msg), true).await?;

    // let handler = call.lock().await;
    // let channel_id = handler.current_channel();
    // let is_connected = handler.current_connection().is_some();
    // let queue = handler.queue().clone();

    // let mut bot_status = BotStatus {
    //     is_playing: handler.is_playing(),
    //     is_paused: handler.is_paused(),
    //     is_stopped: handler.is_stopped(),
    //     current_channel: channel_id.map(|c| c.0),
    // };

    Ok(())
}

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
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?.clone();
    let user_id = ctx.get_user_id();

    let channel_id = match parse_channel_id(channel, channel_id_str)? {
        Some(id) => id,
        None => get_voice_channel_for_user_summon(&guild, &user_id)?,
    };

    let _call: Arc<Mutex<Call>> = match manager.get(guild_id) {
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

    // set_global_handlers(ctx, call, guild_id, channel_id).await?;

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
