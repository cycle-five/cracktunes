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
    // Return the call if it already exists.
    // Otherwise, try to join the channel of the user who sent the message.
    if let Some(call) = manager.get(guild_id) {
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

/// The guild's Call, but only if it actually carries a voice connection.
///
/// 🪤 `Songbird::get` is not a connection test. songbird registers the Call
/// when a join is *attempted*, before the gateway handshake, so a join that
/// timed out still hands one back. `do_join` used to treat that as success:
/// it registered handlers, sent a "Summoned" embed and logged "joined
/// channel" while the bot sat in no voice channel at all -- which is why a
/// total failure left no error anywhere in the log.
async fn connected_call(
    manager: &songbird::Songbird,
    guild_id: GuildId,
) -> Option<Arc<Mutex<Call>>> {
    let call = manager.get(guild_id)?;
    let connected = call.lock().await.current_connection().is_some();
    connected.then_some(call)
}

/// Join a voice channel.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument]
pub async fn do_join(
    ctx: Context<'_>,
    manager: &songbird::Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<Arc<Mutex<Call>>, Error> {
    // let ctx_owned = ctx.clone();
    let guild = guild_id
        .to_guild_cached(ctx.cache())
        .ok_or(CrackedError::NoGuildCached)?
        .clone();
    let guild_name = guild.name;
    // Refuse a join Discord would silently drop. Without this the voice state
    // update is accepted, nothing happens, and songbird reports TimedOut ~10s
    // later with no mention of a permission.
    //
    // 🪤 This MUST come before the channel-name lookup below. Discord omits a
    // voice channel the bot lacks VIEW_CHANNEL on from GUILD_CREATE entirely,
    // so `guild.channels.get` misses exactly when the gate has the most to
    // say — and `NoChannelId` would win the race and report nothing useful.
    // That ordering made `JoinLookup::Withheld` unreachable from this, the
    // busiest of the three join sites.
    crate::music::perms::ensure_can_join(ctx.cache(), guild_id, channel_id)
        .map_err(|e| -> Error { Box::new(e) })?;
    let channel_name = guild
        .channels
        .get(&channel_id)
        .ok_or(CrackedError::NoChannelId)?
        .base
        .name
        .clone();
    tracing::warn!(
        "Joining channel: {channel_name} ({channel_id:?}) in {guild_name} ({guild_id:?})"
    );
    let call = match manager.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(err) => match connected_call(manager, guild_id).await {
            Some(call) => call,
            None => {
                tracing::warn!("Error joining channel: {:?}", err);
                // Drop the connectionless Call, or the next summon finds it
                // via `manager.get` and reports success without ever trying.
                let _ = manager.remove(guild_id).await;
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

/// Source-order guards for `do_join`'s three ordering bugs, all of which cost
/// a production `/summon` its answer (see the 2026-09-12 RuneCast trace).
///
/// 🪤 These assert on source text, not behaviour. `do_join` takes a poise
/// `Context` and a live `songbird::Songbird`, neither of which is
/// constructible offline, so the decisions they pin have no reachable seam --
/// unlike `perms::join_refusal`, which was extracted precisely so it would.
/// A guard that reads the file is weaker than a test that runs the code; it
/// is far stronger than the nothing that was here while all three shipped.
#[cfg(test)]
mod join_order_guard_tests {
    /// 🪤 Builds the path by formatting rather than `Path::join`, on purpose,
    /// and this comment avoids spelling that method call out.
    /// `perms::join_site_guard_tests` scans source text for a join whose first
    /// argument is not a string literal and calls it a songbird join, so
    /// joining a path with a variable -- or merely naming the method in prose,
    /// which cost a second red run to notice -- reads as an ungated join. The
    /// heuristic is deliberately conservative: a false positive is loud and
    /// cheap, a false negative is silent and cost ct#496. The fix belongs on
    /// this side, not in that guard.
    fn read(rel: &str) -> String {
        let path = format!("{}/src/{rel}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    }

    /// The body of one function, bounded at the next `}` in column 0, with
    /// `//` comments stripped.
    ///
    /// Bounding matters: this very module lives in `music_utils.rs` and
    /// contains every literal searched for below. An unbounded slice would
    /// let the test satisfy itself -- the trap `perms.rs` names `SELF_PATH`.
    ///
    /// 🪤 Stripping comments matters just as much, and cost a red test to
    /// learn: the comment *explaining* why the gate precedes the channel
    /// lookup names `NoChannelId`, and sits above the gate. Searching raw
    /// text found the prose first and failed correct code. A guard that reads
    /// commentary is not reading the program.
    fn body_of(src: &str, signature: &str) -> String {
        let start = src.find(signature).unwrap_or_else(|| {
            panic!("{signature} not found -- guard is looking at the wrong file")
        });
        let rest = &src[start..];
        let end = rest.find("\n}\n").expect("unterminated function body");
        rest[..end]
            .lines()
            .map(|line| match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn gate_runs_before_the_channel_name_lookup() {
        let body = body_of(&read("commands/music_utils.rs"), "pub async fn do_join(");
        let gate = body
            .find("ensure_can_join(")
            .expect("do_join lost its gate");
        let lookup = body
            .find("NoChannelId")
            .expect("do_join lost its channel-name lookup -- rewrite this guard");
        assert!(
            gate < lookup,
            "`ensure_can_join` must precede the `NoChannelId` lookup. Discord omits a \
             voice channel the bot cannot see from GUILD_CREATE, so the lookup misses \
             exactly when the gate has the most to say, and `JoinLookup::Withheld` \
             becomes unreachable from this join site."
        );
    }

    #[test]
    fn a_failed_join_is_not_reported_as_success() {
        let body = body_of(&read("commands/music_utils.rs"), "pub async fn do_join(");
        assert!(
            body.contains("connected_call(manager, guild_id)"),
            "the join-failure fallback must go through `connected_call`"
        );
        assert!(
            !body.contains("manager.get(guild_id)"),
            "`do_join` must not consult `Songbird::get` directly: it returns a Call for a \
             join that only *started*, so a timed-out join is reported as success. Route \
             it through `connected_call`, which checks for an actual connection."
        );
        let helper = body_of(&read("commands/music_utils.rs"), "async fn connected_call(");
        assert!(
            helper.contains("current_connection().is_some()"),
            "`connected_call` must test the connection, not merely fetch the Call"
        );
        assert!(
            body.contains("manager.remove(guild_id)"),
            "a connectionless Call must be dropped, or the next summon finds it via \
             `manager.get` and reports success without ever attempting a join"
        );
    }

    #[test]
    fn summon_defers_before_joining() {
        let body = body_of(
            &read("commands/music/summon.rs"),
            "pub async fn summon_internal(",
        );
        let defer = body
            .find("ctx.defer()")
            .expect("summon_internal must defer: a join can take 10s, the token dies at 3s");
        let join = body
            .find("do_join(")
            .expect("summon_internal no longer calls do_join -- rewrite this guard");
        assert!(
            defer < join,
            "the defer must precede the join, not follow it"
        );
    }
}
