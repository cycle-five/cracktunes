use crate::connection::get_voice_channel_for_user;
use crate::guild::operations::GuildSettingsOperations;
use crate::handlers::{IdleHandler, TrackEndHandler};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::CrackedError;
use crate::{Context, Error};
use poise::serenity_prelude::Mentionable;
use serenity::all::{ChannelId, GuildId};
use songbird::{Call, Event, TrackEvent};
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::sync::Mutex;

/// Set the global handlers for the bot in a call.
#[cfg(not(tarpaulin_include))]
pub async fn set_global_handlers(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<(), CrackedError> {
    // use crate::handlers::voice::register_voice_handlers;

    let data = ctx.data();
    // let manager = songbird::get(ctx.serenity_context())
    //     .await
    //     .ok_or(CrackedError::NoSongbird)?;

    // This is the temp buffer to hold voice data for processing
    // let buffer = {
    //     // // Open the data lock in write mode, so keys can be inserted to it.
    //     // let mut data = ctx.data().write().await;
    //     // data.insert::<Vec<u8>>(Arc::new(RwLock::new(Vec::new())));
    //     let data = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    //     data.clone()
    // };

    // unregister existing events and register idle notifier
    //call.lock().await.remove_all_global_events();
    //register_voice_handlers(buffer, call.clone(), ctx.serenity_context().clone()).await?;

    let mut handler = call.lock().await;

    handler.remove_all_global_events();

    let guild_settings = match data.get_guild_settings(guild_id).await {
        Some(settings) => settings,
        None => {
            drop(handler);
            return Err(CrackedError::NoGuildSettings);
        },
    };

    let timeout = guild_settings.timeout;
    if timeout > 0 {
        let premium = guild_settings.premium;
        handler.add_global_event(
            Event::Periodic(Duration::from_secs(60), None),
            IdleHandler {
                serenity_ctx: Arc::new(ctx.serenity_context().clone()),
                guild_id,
                channel_id,
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
            cache: ctx.serenity_context().cache.clone(),
            http: ctx.serenity_context().http.clone(),
            call: call.clone(),
            data: ctx.data().clone(),
        },
    );

    drop(handler);

    Ok(())
}

/// Get the call handle for songbird.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument]
pub async fn get_call_or_join_author(ctx: Context<'_>) -> Result<Arc<Mutex<Call>>, CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NoSongbird)?;

    // Return the call if it already exists
    if let Some(call) = manager.get(guild_id) {
        return Ok(call);
    }
    // Otherwise, try to join the channel of the user who sent the message.
    let channel_id = {
        let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?;
        get_voice_channel_for_user(&guild.clone(), &ctx.author().id)?
    };

    let call: Arc<Mutex<Call>> = do_join(ctx, &manager, guild_id, channel_id).await?;

    Ok(call)
}

/// Join a voice channel.
pub async fn do_join(
    ctx: Context<'_>,
    manager: &songbird::Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<Arc<Mutex<Call>>, Error> {
    tracing::warn!("Joining channel: {:?}", channel_id);
    let call = match manager.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(err) => {
            let call = match manager.get(guild_id) {
                Some(call) => call,
                None => {
                    tracing::warn!("Error joining channel: {:?}", err);
                    let str = err.to_string().clone();
                    let my_err = CrackedError::JoinChannelError(err);
                    let msg = CrackedMessage::CrackedRed(str.clone());
                    ctx.send_reply_embed(msg).await?;
                    return Err(Box::new(my_err));
                },
            };
            call
        },
    };
    //let call = tokio::time::timeout(Duration::from_secs(5), call).await?;
    // match call {
    //     // If we successfully joined the channel, set the global handlers.
    //     // TODO: This should probably be a separate function.
    //        Ok(call) => {
    set_global_handlers(ctx, call.clone(), guild_id, channel_id).await?;
    let msg = CrackedMessage::Summon {
        mention: channel_id.mention(),
    };
    ctx.send_reply_embed(msg).await?;
    Ok(call)
    //     },
    //     Err(err) => {
    //         // FIXME: Do something smarter here also.
    //         let str = err.to_string().clone();
    //         let my_err = CrackedError::JoinChannelError(err);
    //         let msg = CrackedMessage::CrackedRed(str.clone());
    //         ctx.send_reply_embed(msg).await?;
    //         Err(Box::new(my_err))
    //     },
    // }
}
