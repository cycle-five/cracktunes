use crate::connection::get_voice_channel_for_user;
use crate::guild::operations::GuildSettingsOperations;
use crate::handlers::{IdleHandler, TrackEndHandler};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::CrackedError;
use crate::{Context, Error};
// use crack_testing::ReplyHandleWrapper;
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
) {
    let data = ctx.data();
    let mut handler = call.lock().await;

    handler.remove_all_global_events();

    let guild_settings = data
        .get_or_create_guild_settings(guild_id, None, None)
        .await;

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
    // let ctx_owned = ctx.clone();
    tracing::warn!("Joining channel: {:?}", channel_id);
    let call = match manager.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(err) => match manager.get(guild_id) {
            Some(call) => call,
            None => {
                tracing::warn!("Error joining channel: {:?}", err);
                // let str = err.to_string().clone();
                let my_err = CrackedError::JoinChannelError(err);
                // let crack_msg = CrackedMessage::CrackedRed(str.clone());
                // let msg = PoiseContextExt::send_reply_embed(ctx, crack_msg).await?;
                // //ctx.defer().await;
                // //msg.delete_after(ctx, Duration::from_secs(10)).await;
                // let msg_or_reply =
                //     MessageOrReplyHandle::from(ReplyHandleWrapper { handle: msg.into() });
                // ctx.data().push_latest_msg(guild_id, msg_or_reply).await;
                return Err(Box::new(my_err));
            },
        },
    };
    set_global_handlers(ctx, call.clone(), guild_id, channel_id).await;
    let msg = CrackedMessage::Summon {
        mention: channel_id.mention(),
    };
    match ctx.send_reply_embed(msg).await {
        Ok(_) => (),
        Err(err) => {
            tracing::warn!("Error sending reply: {:?}", err);
        },
    };
    Ok(call)
}
