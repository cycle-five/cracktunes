use crate::connection::get_voice_channel_for_user;
use crate::guild::operations::GuildSettingsOperations;
use crate::handlers::{IdleHandler, TrackEndHandler};
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::CrackedError;
use crate::{Context, Data, Error};
// use crack_testing::ReplyHandleWrapper;
use poise::serenity_prelude::{Context as SerenityContext, Mentionable};
use serenity::all::{ChannelId, GenericChannelId, GuildId};
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
    channel_id: GenericChannelId,
) {
    set_global_handlers_with(
        ctx.serenity_context(),
        ctx.data(),
        call,
        guild_id,
        channel_id,
    )
    .await
}

/// The same, from outside a command -- a `/gp` game being resumed after a
/// restart joins voice from the guild-create handler, where there is no poise
/// context to hand over.
pub async fn set_global_handlers_with(
    serenity_ctx: &SerenityContext,
    data: Arc<Data>,
    call: Arc<Mutex<Call>>,
    guild_id: GuildId,
    channel_id: GenericChannelId,
) {
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
                serenity_ctx: Arc::new(serenity_ctx.clone()),
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
            cache: serenity_ctx.cache.clone(),
            http: serenity_ctx.http.clone(),
            call: call.clone(),
            data,
        },
    );

    //drop(handler);
}

/// Get the call handle for songbird.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument(skip(ctx))]
pub async fn get_call_or_join_author(ctx: Context<'_>) -> Result<Arc<Mutex<Call>>, CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = ctx.data().songbird.clone();
    // Return the call if it already exists AND actually carries a connection.
    // Otherwise, try to join the channel of the user who sent the message.
    //
    // 🪤 `Songbird::get` alone was the bug: a connectionless Call short-
    // circuited the join here, so `/play` and `/downvote` bypassed
    // `ensure_can_join` and enqueued into a driver connected to nothing --
    // a "Queued" embed and silence, with no error anywhere.
    if let Some(call) = connected_call(&manager, guild_id, None).await {
        Ok(call)
    } else {
        let channel_id = {
            let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?;
            get_voice_channel_for_user(&guild.clone(), &ctx.author().id)?
        };

        do_join(ctx, &manager, guild_id, channel_id)
            .await
            .map_err(|err| err.into())
    }
    // // Return the call if it already exists
    // if let Some(call) = manager.get(guild_id) {
    //     return Ok(call);
    // }
    // // Otherwise, try to join the channel of the user who sent the message.
    // let channel_id = {
    //     let guild = ctx.guild().ok_or(CrackedError::NoGuildCached)?;
    //     get_voice_channel_for_user(&guild.clone(), &ctx.author().id)?
    // };

    // let call: Arc<Mutex<Call>> = do_join(ctx, &manager, guild_id, channel_id).await?;

    // Ok(call)
}

/// The guild's Call, but only if it actually carries a voice connection --
/// and, when `expect` is given, only if that connection is to that channel.
///
/// 🪤 `Songbird::get` is not a connection test. songbird registers the Call
/// when a join is *attempted*, before the gateway handshake, so a join that
/// timed out still hands one back. Three call sites used to treat that as
/// success: `do_join` registered handlers and sent a "Summoned" embed for a
/// bot in no voice channel, and `summon_internal` and `get_call_or_join_author`
/// skipped the join -- and therefore the permission gate -- entirely.
///
/// This is the only `Songbird::get` **on the join path**, which is what
/// `join_order_guard_tests` enforces, over `music_utils.rs` and `summon.rs`.
/// It is NOT the only one in the crate: ~20 other sites read the Call to
/// answer "are we playing?" for `/queue`, `/volume`, `/clear` and friends.
/// Those want a registered Call and are mostly harmless, but several would
/// also be better off asking this question -- tracked separately.
///
/// 🪤 `expect` matters because "connected" is not "connected to the channel
/// you asked for". A concurrent `/gp` resume can land a join on another
/// channel between a failed `manager.join` and this check, and without the
/// comparison `do_join` would announce "Summoned <#B>" for a bot in A.
pub(crate) async fn connected_call(
    manager: &songbird::Songbird,
    guild_id: GuildId,
    expect: Option<ChannelId>,
) -> Option<Arc<Mutex<Call>>> {
    let call = manager.get(guild_id)?;
    let (connected, here) = {
        let handler = call.lock().await;
        (
            handler.current_connection().is_some(),
            handler.current_channel(),
        )
    };
    match (connected, here, expect) {
        (false, _, _) => None,
        (true, _, None) => Some(call),
        (true, Some(here), Some(want)) => (here.get() == want.get()).then_some(call),
        (true, None, Some(_)) => None,
    }
}

/// Join a voice channel.
///
/// Defers before the handshake, and only there. Every join that reaches
/// `Songbird::join` can outlive Discord's three-second interaction deadline --
/// songbird's `gateway_timeout` is 10s -- and this is the one point every
/// command's join funnels through, so the defer belongs here rather than in
/// each of them. poise's defer is idempotent and a no-op on prefix commands,
/// so a caller that already deferred pays nothing.
///
/// It sits deliberately *after* the permission gate: a refusal is cache-only
/// and answers in microseconds, so it should not spend a round-trip putting
/// the user on "thinking...".
///
/// 🪤 `perms::join_site_guard_tests` asserts the gate appears within 1200
/// bytes before `manager.join`, so keep prose out of the span between them.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument]
pub async fn do_join(
    ctx: Context<'_>,
    manager: &songbird::Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<Arc<Mutex<Call>>, Error> {
    // One cache read, no clone. This used to deep-copy the whole Guild --
    // every member, role, channel and presence -- for a name and one lookup.
    // The ref is confined to this block because it cannot be held across the
    // await below.
    let (guild_name, channel_name) = {
        let guild = guild_id
            .to_guild_cached(ctx.cache())
            .ok_or(CrackedError::NoGuildCached)?;
        let channel = guild.channels.get(&channel_id);
        // A channel missing from the cache is ambiguous: either Discord
        // withheld it (the bot lacks VIEW_CHANNEL, and it omits those from
        // GUILD_CREATE) or it is not a channel in this guild at all --
        // `/summonchannel` takes a raw id, so a typo lands here. Voice states
        // are NOT filtered by VIEW_CHANNEL, so a member sitting in it proves
        // it is real and merely hidden. With nobody in it we cannot tell, and
        // `NoChannelId` is the answer that does not invent a permission
        // problem on a channel that may not exist.
        if channel.is_none()
            && !guild
                .voice_states
                .iter()
                .any(|vs| vs.channel_id == Some(channel_id))
        {
            return Err(Box::new(CrackedError::NoChannelId));
        }
        (
            guild.name.to_string(),
            channel.map(|c| c.base.name.to_string()),
        )
    };
    // Logged before the gate so a refusal is visible server-side too. The
    // name is an Option because the channel we most need to report on is
    // exactly the one Discord did not send us.
    tracing::warn!(
        "Joining {} ({channel_id:?}) in {guild_name} ({guild_id:?})",
        channel_name.as_deref().unwrap_or("<not visible to us>")
    );
    // Refuse a join Discord would silently drop. Without this the voice state
    // update is accepted, nothing happens, and songbird reports TimedOut ~10s
    // later naming no permission at all.
    crate::music::perms::ensure_can_join(ctx.cache(), guild_id, channel_id)
        .map_err(|e| -> Error { Box::new(e) })?;
    // See this function's doc comment for why the defer sits exactly here.
    ctx.defer().await?;
    let call = match manager.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(err) => match connected_call(manager, guild_id, Some(channel_id)).await {
            Some(call) => {
                // The handshake reported a problem but the connection is up.
                // Worth a line: a recovered join should be distinguishable in
                // the log from a clean one.
                tracing::warn!("Join into {channel_id:?} reported {err:?} but connected");
                call
            },
            None => {
                tracing::warn!("Error joining channel: {:?}", err);
                // Drop the connectionless Call, or the next join finds it and
                // reports success without ever trying. songbird's `remove` is
                // `leave(..)?` *then* `calls.remove(..)`, so a failing leave
                // skips the removal -- log it rather than discarding the only
                // signal that the Call is still registered.
                //
                // 🪤 Guarded on there being NO connection at all. The arm
                // above is also taken when we are connected to a *different*
                // channel -- the concurrent-`/gp`-resume race `expect` exists
                // for -- and `remove` is `leave` then drop, so removing here
                // would disconnect that live session and bin its queue.
                if connected_call(manager, guild_id, None).await.is_none() {
                    if let Err(e) = manager.remove(guild_id).await {
                        tracing::warn!(
                            "Could not remove the connectionless Call for {guild_id:?}: {e:?}. \
                             A later join may find it and skip the permission gate."
                        );
                    }
                }
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
    set_global_handlers(ctx, call.clone(), guild_id, channel_id.widen()).await;
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
