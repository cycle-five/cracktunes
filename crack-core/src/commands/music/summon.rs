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
use ::serenity::all::{Channel, Guild, GuildId, UserId};
use ::serenity::builder::CreateEmbed;
use poise::{serenity_prelude as serenity, CreateReply, ReplyHandle};
use songbird::{Call, Songbird};
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
    #[description = "Channel to join"] channel: Option<Channel>,
    #[description = "Channel id to join"] channel_id_str: Option<String>,
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

    let channel_id =
        get_channel_id_for_summon(channel, channel_id_str, guild.clone(), user_id).await?;

    let call: Arc<Mutex<Call>> = match manager.get(guild.id) {
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
        }
        None => manager.join(guild.id, channel_id).await.map_err(|e| {
            tracing::error!("Error joining channel: {:?}", e);
            CrackedError::JoinChannelError(e)
        }),
    }?;

    let _ = register_handlers(ctx, call, manager, guild_id, channel_id).await;

    Ok(())
}

use crate::handlers::voice::register_voice_handlers;

/// Register the handlers for the voice channel.
pub async fn register_handlers(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    manager: Arc<Songbird>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<ReplyHandle, Error> {
    // // join the channel
    // let result = manager.join(guild.id, channel_id).await?;
    let buffer = {
        // // Open the data lock in write mode, so keys can be inserted to it.
        // let mut data = ctx.data().write().await;

        // // So, we have to insert the same type to it.
        // data.insert::<Vec<u8>>(Arc::new(RwLock::new(Vec::new())));
        let data = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        data.clone()
    };

    // FIXME
    // use crate::handlers::voice::register_voice_handlers;

    let _ = register_voice_handlers(buffer, call.clone()).await;
    let text = {
        let mut handler = call.lock().await;
        // unregister existing events and register idle notifier
        handler.remove_all_global_events();

        let guild_settings_map = ctx.data().guild_settings_map.read().unwrap().clone();

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
                guild_id,
                http: ctx.serenity_context().http.clone(),
                call: call.clone(),
                data: ctx.data().clone(),
            },
        );

        CrackedMessage::Summon {
            mention: channel_id.mention(),
        }
        .to_string()
    };
    ctx.send(CreateReply::new().content(text).ephemeral(true))
        .await
        .map_err(|err| err.into())
}

pub fn get_channel_id(ctx: Context<'_>, guild: Guild, channel_id: Option<ChannelId>) -> ChannelId {
    match channel_id {
        Some(channel_id) => channel_id,
        None => guild
            .voice_states
            .get(&ctx.author().id)
            .and_then(|voice_state| voice_state.channel_id)
            .unwrap(),
    }
}

use crate::utils::send_embed_response_poise;
/// Get the call handle for songbird
/// FIXME: Does this need to take the GuildId?
#[inline]
pub async fn get_call_with_fail_msg(
    ctx: Context<'_>,
    guild: Guild,
    channel_id: Option<ChannelId>,
) -> Result<Arc<Mutex<Call>>, Error> {
    let guild_id = guild.id;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::Other(
            "Songbird Voice client was not initialized.",
        ))?
        .clone();
    tracing::warn!("manager: {:?}", manager);
    match manager.get(guild_id) {
        Some(call) => {
            // let channel_id = get_channel_id(ctx, guild, channel_id).await;
            // let _ = register_handlers(ctx, call.clone(), manager, guild_id, channel_id).await;
            Ok(call)
        }
        None => {
            // try to join a voice channel if not in one just yet
            // FIXME:
            let channel_id = get_channel_id(ctx, guild, channel_id);
            let guild_chan = channel_id
                .to_channel_cached(ctx)
                .expect("Channel found")
                .guild()
                .expect("Chanel in a guild");

            if guild_chan.kind == serenity::model::channel::ChannelType::Voice {
                match manager.join(guild_id, channel_id).await {
                    Ok(call) => {
                        // let call = manager.get(guild_id).unwrap();
                        let _ = register_handlers(ctx, call.clone(), manager, guild_id, channel_id)
                            .await;
                        Ok(call)
                    }
                    Err(_) => {
                        let embed = CreateEmbed::default()
                            .description(format!("{}", CrackedError::NotConnected));
                        send_embed_response_poise(ctx, embed).await?;
                        Err(CrackedError::NotConnected.into())
                    }
                }
            } else {
                Err(CrackedError::Other("Not a voice channel").into())
            }
        }
    }
}

async fn get_channel_id_for_summon(
    channel: Option<Channel>,
    channel_id_str: Option<String>,
    guild: Guild,
    user_id: UserId,
) -> Result<ChannelId, Error> {
    if let Some(channel) = channel {
        let guild_chan = channel.clone().guild().ok_or(CrackedError::Other(
            "Channel is not in a guild, which is required for music.",
        ))?;
        if guild_chan.kind == serenity::model::channel::ChannelType::Voice {
            return Ok(channel.id());
        } else {
            return Err(CrackedError::Other("Channel is not a voice channel").into());
        }
    }

    match channel_id_str {
        Some(id) => {
            tracing::warn!("channel_id_str: {:?}", id);
            match id.parse::<u64>() {
                Ok(id) => Ok(ChannelId::new(id)),
                Err(_) => match get_voice_channel_for_user(&guild, &user_id) {
                    Some(channel_id) => Ok(channel_id),
                    None => get_voice_channel_for_user_with_error(&guild, &user_id),
                },
            }
        }
        None => get_voice_channel_for_user_with_error(&guild, &user_id),
    }
}

fn get_voice_channel_for_user_with_error(
    guild: &Guild,
    user_id: &UserId,
) -> Result<ChannelId, Error> {
    match get_voice_channel_for_user(guild, user_id) {
        Some(channel_id) => Ok(channel_id),
        None => {
            // ctx.say("You are not in a voice channel!").await?;
            tracing::warn!(
                "User {} is not in a voice channel in guild {}",
                user_id,
                guild.id
            );
            Err(CrackedError::WrongVoiceChannel.into())
        }
    }
}
