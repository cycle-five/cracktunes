use self::serenity::{model::id::ChannelId, Mentionable};
use crate::{
    connection::get_voice_channel_for_user,
    errors::CrackedError,
    handlers::{IdleHandler, TrackEndHandler},
    messaging::message::ParrotMessage,
    utils::{get_guild_id, get_user_id},
    Context, Error,
};
use poise::serenity_prelude as serenity;
use songbird::{Event, TrackEvent};
use std::time::Duration;

/// Summon the bot to a voice channel.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn summon(
    ctx: Context<'_>,
    #[description = "Channel id to join"] channel_id_str: Option<String>,
    #[description = "Send a reply to the user"] send_reply: Option<bool>,
) -> Result<(), Error> {
    let guild_id = get_guild_id(&ctx).unwrap();
    let guild = ctx.serenity_context().cache.guild(guild_id).unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let user_id = get_user_id(&ctx);

    let channel_id = match channel_id_str {
        Some(id) => ChannelId(id.parse::<u64>().unwrap()),
        None => match get_voice_channel_for_user(&guild, &user_id) {
            Some(channel_id) => channel_id,
            None => {
                if send_reply.unwrap_or(true) {
                    ctx.say("You are not in a voice channel!").await?;
                }
                return Err(CrackedError::WrongVoiceChannel.into());
            }
        },
    };

    if let Some(call) = manager.get(guild.id) {
        let handler = call.lock().await;
        let has_current_connection = handler.current_connection().is_some();

        if has_current_connection && send_reply.unwrap_or(true) {
            // bot is in another channel
            let bot_channel_id: ChannelId = handler.current_channel().unwrap().0.into();
            return Err(CrackedError::AlreadyConnected(bot_channel_id.mention()).into());
        }
    }

    // join the channel
    manager.join(guild.id, channel_id).await.1?;

    // unregister existing events and register idle notifier
    if let Some(call) = manager.get(guild.id) {
        let mut handler = call.lock().await;

        handler.remove_all_global_events();

        handler.add_global_event(
            Event::Periodic(Duration::from_secs(1), None),
            IdleHandler {
                http: ctx.serenity_context().http.clone(),
                manager: manager.clone(),
                channel_id,
                guild_id: Some(guild_id),
                limit: 60 * 10,
                count: Default::default(),
            },
        );

        handler.add_global_event(
            Event::Track(TrackEvent::End),
            TrackEndHandler {
                guild_id: guild.id,
                call: call.clone(),
                ctx_data: ctx.serenity_context().data.clone(),
            },
        );
    }

    if send_reply.unwrap_or(true) {
        ctx.say(
            ParrotMessage::Summon {
                mention: channel_id.mention(),
            }
            .to_string(),
        )
        .await?;
    }

    Ok(())
}
