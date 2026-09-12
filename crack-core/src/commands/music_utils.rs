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
/// 🪤 `Songbird::get` is not a connection test, and this is the only place in
/// the crate allowed to call it (`join_order_guard_tests` enforces that).
/// songbird registers the Call when a join is *attempted*, before the gateway
/// handshake, so a join that timed out still hands one back. Three call sites
/// used to treat that as success: `do_join` registered handlers and sent a
/// "Summoned" embed for a bot in no voice channel, and `summon_internal` and
/// `get_call_or_join_author` skipped the join -- and therefore the permission
/// gate -- entirely.
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
    // Every join that gets this far can outlive Discord's three-second
    // interaction deadline -- songbird's gateway_timeout is 10s -- so defer
    // here, the one point every command's join funnels through, rather than
    // in each of them. poise's defer is idempotent and a no-op on prefix
    // commands, so callers that already deferred pay nothing.
    //
    // Deliberately AFTER the gate: a refusal is cache-only and answers in
    // microseconds, so it should not spend a round-trip on "thinking...".
    ctx.defer().await?;
    let call = match manager.join(guild_id, channel_id).await {
        Ok(call) => call,
        Err(err) => match connected_call(manager, guild_id, Some(channel_id)).await {
            Some(call) => call,
            None => {
                tracing::warn!("Error joining channel: {:?}", err);
                // Drop the connectionless Call, or the next join finds it and
                // reports success without ever trying. songbird's `remove` is
                // `leave(..)?` *then* `calls.remove(..)`, so a failing leave
                // skips the removal -- log it rather than discarding the only
                // signal that the Call is still registered.
                if let Err(e) = manager.remove(guild_id).await {
                    tracing::warn!(
                        "Could not remove the connectionless Call for {guild_id:?}: {e:?}. \
                         A later join may find it and skip the permission gate."
                    );
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
    /// Drop a trailing `//` comment, but not the `//` in a URL.
    ///
    /// 🪤 A naive cut at the first `//` truncates any line holding a
    /// `https://` literal -- routine in a Discord bot -- and a guard that
    /// silently scans less than it claims is the failure this whole module
    /// exists to prevent.
    fn strip_comment(line: &str) -> &str {
        let b = line.as_bytes();
        let mut i = 0;
        while i + 1 < b.len() {
            if b[i] == b'/' && b[i + 1] == b'/' {
                if i > 0 && b[i - 1] == b':' {
                    i += 2;
                    continue;
                }
                return &line[..i];
            }
            i += 1;
        }
        line
    }

    #[test]
    fn strip_comment_cuts_comments_and_keeps_urls() {
        assert_eq!(strip_comment("let x = 1; // note"), "let x = 1; ");
        assert_eq!(strip_comment("// whole line"), "");
        assert_eq!(strip_comment("no comment here"), "no comment here");
        assert_eq!(
            strip_comment(r#"warn!("see https://x/y");"#),
            r#"warn!("see https://x/y");"#
        );
        assert_eq!(
            strip_comment(r#"let u = "https://a"; // trailing"#),
            r#"let u = "https://a"; "#
        );
    }

    fn body_of(src: &str, signature: &str) -> String {
        let start = src.find(signature).unwrap_or_else(|| {
            panic!("{signature} not found -- guard is looking at the wrong file")
        });
        // 🪤 `find` takes the FIRST match, and the test module below holds
        // these same signatures as string literals. That resolves to the real
        // definition only because the module happens to sit at the bottom of
        // the file -- position, not design. Pin it, so moving this module up
        // fails loudly instead of silently parsing its own literals.
        if let Some(tests) = src.find("#[cfg(test)]") {
            assert!(
                start < tests,
                "`{signature}` was first found inside a test module. `body_of` is \
                 parsing the guard's own string literals, not the program."
            );
        }
        let rest = &src[start..];
        let end = rest.find("\n}\n").expect("unterminated function body");
        rest[..end]
            .lines()
            .map(strip_comment)
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
            body.contains("connected_call(manager, guild_id, Some(channel_id))"),
            "the join-failure fallback must go through `connected_call`, and must name \
             the channel it asked for -- a Call connected to a *different* channel is \
             not a successful join of this one"
        );
        assert!(
            body.contains("manager.remove(guild_id)"),
            "a connectionless Call must be dropped, or a later join finds it and skips \
             the gate"
        );
        assert!(
            !body.contains("let _ = manager.remove"),
            "songbird's `remove` is `leave(..)?` then `calls.remove(..)`, so a failing \
             leave skips the removal entirely. Discarding that error hides the fact \
             that the Call is still registered."
        );
    }

    /// The twin-hunting guard. The narrow version of this pinned `do_join`
    /// alone and read green while two live copies of the same defect shipped
    /// on the `/summon` and `/play` entry points.
    #[test]
    fn songbird_get_is_reachable_only_through_connected_call() {
        let files = ["commands/music_utils.rs", "commands/music/summon.rs"];
        let mut inspected = 0usize;
        for rel in files {
            // 🪤 Stop at the first test module. This very guard contains the
            // literal it searches for, so scanning itself fails itself -- the
            // fourth time in this change that a scanner tripped over text
            // written about it. A guard reads the program, not the tests.
            let whole = read(rel);
            let src = match whole.find("#[cfg(test)]") {
                Some(i) => whole[..i].to_string(),
                None => whole,
            };
            let helper_body = if rel.ends_with("music_utils.rs") {
                body_of(&src, "pub(crate) async fn connected_call(")
            } else {
                String::new()
            };
            for (n, line) in src.lines().enumerate() {
                let code = strip_comment(line);
                if !code.contains("manager.get(") {
                    continue;
                }
                inspected += 1;
                assert!(
                    helper_body.lines().any(|h| h.trim() == code.trim()),
                    "{rel}:{}: `Songbird::get` outside `connected_call`.\n\n\
                     It returns a Call for a join that only *started*, so this reads a \
                     connectionless handle as a live one -- skipping the join, and with \
                     it `ensure_can_join`. Route it through `connected_call`.\n\n  {}",
                    n + 1,
                    code.trim()
                );
            }
        }
        assert!(
            inspected >= 1,
            "scanned {inspected} `manager.get(` sites across {} files -- the scan has \
             stopped checking rather than the calls being gone. Fix the scan.",
            files.len()
        );
    }

    #[test]
    fn do_join_defers_before_the_handshake() {
        let body = body_of(&read("commands/music_utils.rs"), "pub async fn do_join(");
        let defer = body.find("ctx.defer()").expect(
            "do_join must defer: songbird waits 10s for the handshake, the interaction \
             token dies at 3s. It is the one point every command's join funnels through.",
        );
        let join = body
            .find("manager.join(")
            .expect("do_join no longer joins -- rewrite this guard");
        let gate = body
            .find("ensure_can_join(")
            .expect("do_join lost its gate -- see the other guard");
        assert!(
            defer < join,
            "the defer must precede the handshake it exists to outlast"
        );
        assert!(
            gate < defer,
            "the gate must precede the defer: a refusal is cache-only and answers in \
             microseconds, so it should not spend a round-trip on \"thinking...\""
        );
    }
}
