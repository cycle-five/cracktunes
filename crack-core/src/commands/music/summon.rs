use crate::commands::cmd_check_music;
use crate::handlers::voice::register_voice_handlers;
use crate::handlers::IdleHandler;
use crate::messaging::interface::send_joining_channel;
use crate::{
    connection::get_voice_channel_for_user_summon, errors::CrackedError, handlers::TrackEndHandler,
    Context, ContextExt, Error,
};
use ::serenity::all::{Channel, ChannelId, Guild, Mentionable, UserId};
use songbird::Call;
use songbird::{Event, TrackEvent};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Summon the bot to a voice channel.
#[poise::command(
    slash_command,
    prefix_command,
    aliases("join", "come here", "comehere", "come", "here"),
    guild_only,
    category = "Music",
    check = "cmd_check_music"
)]
pub async fn summon(ctx: Context<'_>) -> Result<(), Error> {
    summon_internal(ctx, None, None).await
}

#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn summon_channel(
    ctx: Context<'_>,
    #[description = "Channel to summon the bot to"] channel: Option<Channel>,
    #[description = "Channel Id of the channel to summon the bot to"] channel_id_str: Option<
        String,
    >,
) -> Result<(), Error> {
    summon_internal(ctx, channel, channel_id_str).await
}

pub async fn summon_internal(
    ctx: Context<'_>,
    channel: Option<Channel>,
    channel_id_str: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?.clone();

    let user_id = ctx.get_user_id();

    let channel_id =
        get_channel_id_for_summon(channel, channel_id_str, guild.clone(), user_id).await?;

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
        None => manager.join(guild_id, channel_id).await.map_err(|e| {
            tracing::error!("Error joining channel: {:?}", e);
            CrackedError::JoinChannelError(e)
        }),
    }?;

    let buffer = {
        // // Open the data lock in write mode, so keys can be inserted to it.
        // let mut data = ctx.data().write().await;

        // data.insert::<Vec<u8>>(Arc::new(RwLock::new(Vec::new())));
        let data = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        data.clone()
    };

    call.lock().await.remove_all_global_events();
    register_voice_handlers(buffer, call.clone(), ctx.serenity_context().clone()).await?;
    let mut handler = call.lock().await;
    let guild_settings_map = ctx.data().guild_settings_map.read().await.clone();
    let guild_settings = guild_settings_map
        .get(&guild_id)
        .expect("Guild settings not found?!?");

    let timeout = guild_settings.timeout;
    if timeout > 0 {
        let premium = guild_settings.premium;

        handler.add_global_event(
            Event::Periodic(Duration::from_secs(5), None),
            IdleHandler {
                http: ctx.serenity_context().http.clone(),
                manager: manager.clone(),
                channel_id,
                guild_id: Some(guild_id),
                limit: timeout as usize,
                count: Default::default(),
                no_timeout: Arc::new(AtomicBool::new(premium)),
            },
        );
    }

    handler.add_global_event(
        Event::Track(TrackEvent::End),
        TrackEndHandler {
            guild_id,
            http: ctx.serenity_context().http.clone(),
            cache: ctx.serenity_context().cache.clone(),
            call: call.clone(),
            data: ctx.data().clone(),
        },
    );

    send_joining_channel(ctx, channel_id).await
}

async fn get_channel_id_for_summon(
    channel: Option<Channel>,
    channel_id_str: Option<String>,
    guild: Guild,
    user_id: UserId,
) -> Result<ChannelId, Error> {
    if let Some(channel) = channel {
        return Ok(channel.id());
    }

    match channel_id_str {
        Some(id) => {
            tracing::warn!("channel_id_str: {:?}", id);
            match id.parse::<u64>() {
                Ok(id) => Ok(ChannelId::new(id)),
                Err(_) => get_voice_channel_for_user_summon(&guild, &user_id),
            }
        },
        None => get_voice_channel_for_user_summon(&guild, &user_id),
    }
}
