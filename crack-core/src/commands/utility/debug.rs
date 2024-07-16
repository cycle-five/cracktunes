use crate::commands::{uptime_internal, CrackedError};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::{ContextExt, MessageInterfaceCtxExt};
use crate::{Context, Error};
use chrono::Duration;
use poise::serenity_prelude as serenity;
use serenity::ChannelId;
use songbird::tracks::PlayMode;
use std::borrow::Cow;
use std::fmt;

/// The status of the bot.
#[derive(Debug, Clone, Default)]
pub struct BotStatus<'ctx> {
    pub name: Cow<'ctx, String>,
    pub play_mode: PlayMode,
    pub queue_len: usize,
    pub current_channel: Option<ChannelId>,
    pub uptime: Duration,
}

impl<'ctx> fmt::Display for BotStatus<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BotStatus {{\n\tname: {},\n\tplay_mode: {:?},\n\tqueue_len: {},\n\tcurrent_channel: {:?},\n\tuptime: {:?}\n}}",
            self.name, self.play_mode, self.queue_len, self.current_channel, self.uptime
        )
    }
}

#[poise::command(
    category = "Utility",
    slash_command,
    prefix_command,
    guild_only,
    owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn debug(ctx: Context<'_>) -> Result<(), Error> {
    debug_internal(ctx).await
}

#[cfg(not(tarpaulin_include))]
pub async fn debug_internal(ctx: Context<'_>) -> Result<(), Error> {
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
