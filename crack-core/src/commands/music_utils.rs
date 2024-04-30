use crate::connection::get_voice_channel_for_user;
use crate::handlers::{IdleHandler, TrackEndHandler};
use crate::messaging::message::CrackedMessage;
use crate::utils::send_embed_response_poise;
use crate::CrackContext;
use crate::CrackedError;
use crate::{Context, Error};
use poise::serenity_prelude::Mentionable;
use poise::CreateReply;
use serenity::all::{ChannelId, CreateEmbed, GuildId};
use songbird::{Call, Event, TrackEvent};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::sync::Mutex;

/// Set the global handlers.
#[cfg(not(tarpaulin_include))]
pub async fn set_global_handlers(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<String, Error> {
    let data = ctx.data();

    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;

    let mut handler = call.lock().await;
    // unregister existing events and register idle notifier
    handler.remove_all_global_events();

    let guild_settings_map = data.guild_settings_map.read().unwrap().clone();

    let _ = guild_settings_map.get(&guild_id).map(|guild_settings| {
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
    });

    handler.add_global_event(
        Event::Track(TrackEvent::End),
        TrackEndHandler {
            guild_id,
            cache: ctx.serenity_context().cache.clone(),
            http: ctx.serenity_context().http.clone(),
            call: call.clone(),
            data: ctx.data().clone(),
        },
    );

    let text = CrackedMessage::Summon {
        mention: channel_id.mention(),
    }
    .to_string();

    Ok(text)
}

/// Get the call handle for songbird.
// FIXME: Does this need to take the GuildId?
#[cfg(not(tarpaulin_include))]
pub async fn get_call_with_fail_msg(
    ctx: Context<'_>,
    guild_id: GuildId,
) -> Result<Arc<Mutex<Call>>, Error> {
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    match manager.get(guild_id) {
        Some(call) => Ok(call),
        None => {
            // try to join a voice channel if not in one just yet
            //match summon_short(ctx).await {
            // TODO: Don't just return an error on failure, do something smarter.
            let channel_id = {
                let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?;
                get_voice_channel_for_user(&guild.clone(), &ctx.author().id)?
            };
            match manager.join(guild_id, channel_id).await {
                Ok(call) => {
                    let text = set_global_handlers(ctx, call.clone(), guild_id, channel_id).await?;

                    let msg = ctx
                        .send(CreateReply::default().content(text).ephemeral(true))
                        .await?
                        .into_message()
                        .await?;
                    ctx.add_msg_to_cache(guild_id, msg);
                    Ok(call)
                },
                Err(_) => {
                    // FIXME: Do something smarter here also.
                    let embed = CreateEmbed::default()
                        .description(format!("{}", CrackedError::NotConnected));
                    send_embed_response_poise(ctx, embed).await?;
                    Err(CrackedError::NotConnected.into())
                },
            }
        },
    }
}
