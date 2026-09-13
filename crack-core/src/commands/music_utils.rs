use crate::connection::get_voice_channel_for_user;
use crate::guild::operations::GuildSettingsOperations;
use crate::handlers::{IdleHandler, TrackEndHandler};
use crate::messaging::message::CrackedMessage;
use crate::music::perms::JoinPermit;
use crate::poise_ext::PoiseContextExt;
use crate::CrackedError;
use crate::{Context, Data, Error};
// use crack_testing::ReplyHandleWrapper;
use poise::serenity_prelude::{Context as SerenityContext, Mentionable};
use serenity::all::{ChannelId, GenericChannelId, GuildId};
use songbird::{error::JoinError, Call, Event, TrackEvent};
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
/// `Songbird::get` is banned in `clippy.toml` (#507). It used to be called
/// from ~17 sites, eight of which spelled their result
/// `CrackedError::NotConnected` while asking a question that cannot establish
/// it. Those now come here. The few that genuinely want "is a Call
/// registered" (debug dumps, `/gp end`'s cleanup) carry an `#[allow]` saying
/// why.
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
    // The one place the raw question is asked: this function exists to turn
    // it into the right one.
    #[allow(clippy::disallowed_methods)]
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

/// Join the channel a [`JoinPermit`] was issued for.
///
/// **The crate's only caller of `Songbird::join`.** Taking the permit by value
/// is what makes that enforceable rather than aspirational: `ensure_can_join`
/// is its only constructor, so a join that skipped the permission gate does
/// not typecheck. This is the compiler doing the job `join_site_guard_tests`
/// was scanning source text to approximate.
///
/// Because there is one implementation, every join site gets the same failure
/// handling. They used to get three different ones (#502): `do_join` dropped
/// the connectionless `Call`, `join_vc` called `leave` -- which clears the
/// connection but deliberately keeps the handler registered, so the stale
/// entry survived exactly as if nothing had been done -- and the `/gp`
/// restart-resume did neither.
pub(crate) async fn join_permitted(
    manager: &songbird::Songbird,
    permit: JoinPermit,
) -> Result<Arc<Mutex<Call>>, JoinError> {
    let (guild_id, channel_id) = permit.into_parts();
    // The one permitted `Songbird::join` in the crate -- `clippy.toml` bans it
    // everywhere else, which is what makes this function the only door rather
    // than merely the recommended one.
    #[allow(clippy::disallowed_methods)]
    let joined = manager.join(guild_id, channel_id).await;
    match joined {
        Ok(call) => Ok(call),
        Err(err) => match connected_call(manager, guild_id, Some(channel_id)).await {
            Some(call) => {
                // The handshake reported a problem but the connection is up.
                // Worth a line: a recovered join should be distinguishable in
                // the log from a clean one.
                tracing::warn!("Join into {channel_id:?} reported {err:?} but connected");
                Ok(call)
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
                Err(err)
            },
        },
    }
}

/// Tell the user we joined; if the embed cannot be delivered, retry as text.
///
/// The join has already succeeded by the time this runs, so a failure here must
/// not be reported as a failed join -- but it must not be silent either, which
/// is what the `tracing::warn!`-and-return-`Ok` it replaces did (#501). The bot
/// ended up sitting in a voice channel with nothing in the text channel saying
/// so.
///
/// 🪤 **This does NOT rescue a stranded "Bot is thinking..." placeholder, and
/// an earlier version of this comment claimed it did.** Two reasons, and they
/// do not overlap:
///
/// - For a **slash** command the reply goes out over the interaction webhook,
///   which is not gated by the text channel's `EMBED_LINKS` / `SEND_MESSAGES`.
///   So the permission case the retry exists for does not arise there.
/// - For a **prefix** command those permissions do apply -- but poise's
///   `defer()` is `if let Self::Application(ctx)`, a no-op for prefix, so there
///   is no deferred interaction to strand in the first place.
///
/// The placeholder is never cleared either way, because poise's
/// `send_application_reply` has only `create_response` and `create_followup`
/// branches and never PATCHes `@original`. That is #504, and it is untouched
/// here.
///
/// What the retry genuinely buys: a **prefix** `/summon` in a channel where
/// `EMBED_LINKS` is denied but `SEND_MESSAGES` is not now gets an answer
/// instead of silence. Cheap, and worth keeping -- just not for the reason
/// first written down.
async fn announce_join(ctx: Context<'_>, channel_id: ChannelId) {
    let Err(embed_err) = ctx
        .send_reply_embed(CrackedMessage::Summon {
            mention: channel_id.mention(),
        })
        .await
    else {
        return;
    };
    tracing::warn!("Could not answer the join with an embed: {embed_err:?}, retrying as text");
    // 🪤 Built here rather than through `send_reply_owned(.., false)`. That
    // helper's non-embed branch pipes the text through `colored`, whose
    // tty/`CLICOLOR_FORCE`/`NO_COLOR` decision nothing here pins -- if it ever
    // resolves to "yes", the user's join confirmation arrives as a literal
    // `\e[38;2;...m` escape sequence. Discord is not a terminal.
    let plain = poise::CreateReply::default().content(
        CrackedMessage::Summon {
            mention: channel_id.mention(),
        }
        .to_string(),
    );
    if let Err(text_err) = ctx.send(plain).await {
        tracing::warn!(
            "Could not answer the join at all: {text_err:?}. \
             The bot IS in {channel_id:?} but nothing in the channel says so."
        );
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
/// The gate can no longer be skipped or reordered by accident -- [`JoinPermit`]
/// is the only way to reach [`join_permitted`] -- so the defer is free to sit
/// between them.
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
    let permit = crate::music::perms::ensure_can_join(ctx.cache(), guild_id, channel_id)
        .map_err(|e| -> Error { Box::new(e) })?;
    // See this function's doc comment for why the defer sits exactly here.
    ctx.defer().await?;
    let call = join_permitted(manager, permit)
        .await
        .map_err(|err| -> Error { Box::new(CrackedError::JoinChannelError(err)) })?;
    set_global_handlers(ctx, call.clone(), guild_id, channel_id.widen()).await;
    announce_join(ctx, channel_id).await;
    Ok(call)
}
