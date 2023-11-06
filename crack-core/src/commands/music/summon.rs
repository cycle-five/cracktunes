use std::sync::Arc;
use std::time::Duration;

use self::serenity::{model::id::ChannelId, Mentionable};
use crate::handlers::IdleHandler;
use crate::{
    connection::get_voice_channel_for_user,
    errors::CrackedError,
    handlers::TrackEndHandler,
    messaging::message::CrackedMessage,
    // handlers::{IdleHandler, TrackEndHandler},
    // handlers::TrackEndHandler,
    // messaging::message::CrackedMessage,
    utils::get_user_id,
    Context,
    Error,
};
use poise::{serenity_prelude as serenity, CreateReply};
use songbird::Call;
use songbird::{Event, TrackEvent};
use tokio::sync::Mutex;
// use std::{sync::Arc, time::Duration};

/// Summon the bot to a voice channel.
#[poise::command(
    slash_command,
    prefix_command,
    aliases("join", "come here", "comehere", "come", "here"),
    guild_only
)]
pub async fn summon(
    ctx: Context<'_>,
    #[description = "Channel id to join"] channel_id_str: Option<String>,
    #[description = "Send a reply to the user"] send_reply: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let guild = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .unwrap()
        .clone();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let user_id = get_user_id(&ctx);

    let channel_id = match channel_id_str {
        Some(id) => {
            tracing::warn!("channel_id_str: {:?}", id);
            match id.parse::<u64>() {
                Ok(id) => ChannelId::new(id),
                Err(_) => match get_voice_channel_for_user(&guild, &user_id) {
                    Some(channel_id) => channel_id,
                    None => {
                        if send_reply.unwrap_or(true) {
                            ctx.say("You are not in a voice channel!").await?;
                        }
                        return Err(CrackedError::WrongVoiceChannel.into());
                    }
                },
            }
        }
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

    let call: Arc<Mutex<Call>> = match manager.get(guild.id) {
        Some(call) => {
            let handler = call.lock().await;
            let has_current_connection = handler.current_connection().is_some();

            if has_current_connection && send_reply.unwrap_or(true) {
                // bot is in another channel
                let bot_channel_id: ChannelId = handler.current_channel().unwrap().0.into();
                Err(CrackedError::AlreadyConnected(bot_channel_id.mention()))
            } else {
                Ok(call.clone())
            }
        }
        None => {
            tracing::error!("No call found for guild: {:?}", guild.id);
            Err(CrackedError::Other("No call found for guild"))
        }
    }?;

    // // join the channel
    // let result = manager.join(guild.id, channel_id).await;
    // let _call = result.map_err(|e| {
    //     tracing::error!("Error joining channel: {:?}", e);
    //     CrackedError::JoinChannelError(e)
    // })?;
    // call.lock().await.join(channel_id).await?;
    let buffer = {
        // // Open the data lock in write mode, so keys can be inserted to it.
        // let mut data = ctx.data().write().await;

        // // So, we have to insert the same type to it.
        // data.insert::<Vec<u8>>(Arc::new(RwLock::new(Vec::new())));
        let data = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        data.clone()
    };

    use crate::handlers::voice::register_voice_handlers;

    // FIXME
    // use crate::handlers::voice::register_voice_handlers;

    let _ = register_voice_handlers(buffer, call.clone()).await;
    {
        let mut handler = call.lock().await;
        // unregister existing events and register idle notifier
        handler.remove_all_global_events();

        let guild_settings_map = ctx.data().guild_settings_map.lock().unwrap().clone();

        let _ = guild_settings_map.get(&guild_id).map(|guild_settings| {
            let timeout = guild_settings.timeout;
            if timeout > 0 {
                handler.add_global_event(
                    Event::Periodic(Duration::from_secs(1), None),
                    IdleHandler {
                        http: ctx.serenity_context().http.clone(),
                        manager: manager.clone(),
                        channel_id,
                        guild_id: Some(guild_id),
                        limit: timeout as usize,
                        count: Default::default(),
                    },
                );
            }
        });

        handler.add_global_event(
            Event::Track(TrackEvent::End),
            TrackEndHandler {
                guild_id: guild.id,
                http: ctx.serenity_context().http.clone(),
                call: call.clone(),
                data: ctx.data().clone(),
            },
        );

        if send_reply.unwrap_or(true) {
            let text = CrackedMessage::Summon {
                mention: channel_id.mention(),
            }
            .to_string();
            ctx.send(CreateReply::new().content(text).ephemeral(true))
                .await?;
        }
    }

    Ok(())
}
