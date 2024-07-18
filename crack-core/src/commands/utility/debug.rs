use crate::commands::{uptime_internal, CrackedError};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::{ContextExt, MessageInterfaceCtxExt};
use crate::{Context, Error};
use chrono::Duration;
use poise::serenity_prelude as serenity;
use serenity::ChannelId;
use serenity::Mentionable;
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
    pub calling_user: String,
}

impl<'ctx> fmt::Display for BotStatus<'ctx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            r#"
            BotStatus {{
              name: {},
              play_mode: {:?},
              queue_len: {},
              current_channel: {:?},
              uptime: {:?}
            }}"#,
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
    let user_id = ctx.get_user_id();
    let mention_caller = user_id.mention().to_string();
    let bot_name = ctx.cache().current_user().mention().to_string();

    // Get the voice channel we're in if any.
    let call = manager.get(guild_id);
    let mut vc_status = match call {
        Some(call) => {
            let handler = call.lock().await;
            let channel_id = handler.current_channel();
            let _is_connected = handler.current_connection().is_some();
            let queue = handler.queue();
            let track = queue.current().clone();

            match track {
                Some(track) => BotStatus {
                    play_mode: track.get_info().await.unwrap_or_default().playing,
                    current_channel: channel_id.map(|id| serenity::ChannelId::new(id.0.into())),
                    queue_len: queue.clone().len(),
                    ..Default::default()
                },
                None => Default::default(),
            }
        },
        _ => Default::default(),
    };
    let uptime = match uptime_internal(ctx).await {
        CrackedMessage::Uptime { seconds, .. } => Duration::seconds(seconds as i64),
        _ => Duration::zero(),
    };
    vc_status.uptime = uptime;
    vc_status.name = Cow::Owned(bot_name);
    vc_status.calling_user = mention_caller;

    let msg = vc_status.to_string();
    ctx.send_reply(CrackedMessage::Other(msg), true).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_print_bot_status() {
        let bot_status = BotStatus {
            name: Cow::Owned("bot".to_string()),
            play_mode: PlayMode::Play,
            queue_len: 1,
            current_channel: Some(ChannelId::new(1)),
            uptime: Duration::seconds(1),
            calling_user: "user".to_string(),
        };

        let msg = bot_status.to_string();

        assert!(msg.contains("bot"));
    }
}
