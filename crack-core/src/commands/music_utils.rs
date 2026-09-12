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
/// 🪤 Keep the body between the gate and `manager.join` short.
/// `perms::join_site_guard_tests` asserts the gate appears within 1200 bytes
/// before the join, so prose added between them eats that budget and makes an
/// unrelated guard fail with a misleading "ungated join" message. That is why
/// this rationale lives up here and not inline.
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
    //
    // 🪤 But a channel absent from the cache is ambiguous: either Discord
    // withheld it (no VIEW_CHANNEL) or it is not a channel in this guild at
    // all -- and `/summonchannel` takes a raw id, so a typo lands here.
    // Voice states are NOT filtered by VIEW_CHANNEL, so a member sitting in
    // it proves it is real and merely hidden. With nobody in it we cannot
    // tell, and `NoChannelId` is the answer that does not invent a
    // permission problem on a channel that may not exist.
    if !guild.channels.contains_key(&channel_id)
        && !guild
            .voice_states
            .iter()
            .any(|vs| vs.channel_id == Some(channel_id))
    {
        return Err(Box::new(CrackedError::NoChannelId));
    }
    crate::music::perms::ensure_can_join(ctx.cache(), guild_id, channel_id).map_err(
        |e| -> Error {
            // The named "Joining channel" line below never runs for a refusal,
            // so without this a gate refusal is invisible server-side -- the
            // same blind spot as #467/#468, on the feature built to end it.
            tracing::warn!("Refusing join into {channel_id:?} in {guild_id:?}: {e}");
            Box::new(e)
        },
    )?;
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

    /// Drop a trailing `//` comment.
    ///
    /// 🪤 Deliberately conservative: it refuses to cut when a quote appears
    /// before the `//`, so a line carrying a string literal -- a URL, or any
    /// text with a slash pair in it -- is scanned whole rather than truncated
    /// mid-literal. Over-inclusion makes a guard fail loudly; truncation makes
    /// it pass while checking less than it claims, which is the failure this
    /// module exists to prevent. It does not parse Rust and does not need to:
    /// every assertion here is "this exact code line is, or is not, present".
    fn strip_comment(line: &str) -> &str {
        match line.find("//") {
            Some(i) if !line[..i].contains('"') => &line[..i],
            _ => line,
        }
    }

    #[test]
    fn strip_comment_cuts_comments_but_never_truncates_a_literal() {
        assert_eq!(strip_comment("let x = 1; // note"), "let x = 1; ");
        assert_eq!(strip_comment("// whole line"), "");
        assert_eq!(strip_comment("no comment here"), "no comment here");
        // A URL inside a literal must not cost us the rest of the line.
        assert_eq!(
            strip_comment(r#"warn!("see https://x/y");"#),
            r#"warn!("see https://x/y");"#
        );
        // The case that matters most: code AFTER a literal containing `//`
        // stays visible. Truncating here would hide a banned call from the
        // scan, which is a guard passing while checking less than it claims.
        assert_eq!(
            strip_comment(r#"warn!("a // b"); manager.get(guild_id)"#),
            r#"warn!("a // b"); manager.get(guild_id)"#
        );
        // Over-inclusive by design: a genuine trailing comment after a literal
        // survives. That can only make a guard louder, never blinder.
        assert_eq!(
            strip_comment(r#"let u = "x"; // trailing"#),
            r#"let u = "x"; // trailing"#
        );
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
        // Anchored on the lookup itself, not on any mention of the error --
        // there is a second, deliberate `NoChannelId` above the gate now, and
        // matching that one made this guard fail correct code.
        let lookup = body
            .find(".ok_or(CrackedError::NoChannelId)")
            .expect("do_join lost its channel-name lookup -- rewrite this guard");
        assert!(
            gate < lookup,
            "`ensure_can_join` must precede the `NoChannelId` lookup. Discord omits a \
             voice channel the bot cannot see from GUILD_CREATE, so the lookup misses \
             exactly when the gate has the most to say, and `JoinLookup::Withheld` \
             becomes unreachable from this join site."
        );
    }

    /// The gate may only claim VIEW_CHANNEL for a channel we have evidence is
    /// real. `/summonchannel` takes a raw id, and a cache miss cannot tell a
    /// hidden channel from a typo.
    #[test]
    fn an_unknown_unoccupied_channel_is_not_called_a_permission_problem() {
        let body = body_of(&read("commands/music_utils.rs"), "pub async fn do_join(");
        let bail = body
            .find("return Err(Box::new(CrackedError::NoChannelId))")
            .expect(
                "do_join must bail on a channel that is neither cached nor occupied, or a \
                 typo'd id is reported as a missing View Channel on a channel that does \
                 not exist -- the exact false diagnosis perms.rs exists to prevent",
            );
        assert!(
            body.contains("voice_states"),
            "the existence check must consult voice states: they are NOT filtered by \
             VIEW_CHANNEL, so a member sitting in the channel is the only offline proof \
             that a channel missing from the cache is real rather than imaginary"
        );
        let gate = body
            .find("ensure_can_join(")
            .expect("do_join lost its gate");
        assert!(
            bail < gate,
            "the existence check must precede the gate, or the gate answers first"
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
            body.contains("connected_call(manager, guild_id, None).await.is_none()"),
            "the teardown must be guarded on there being NO connection at all. The arm \
             above is also taken when we are connected to a DIFFERENT channel -- the \
             concurrent-/gp-resume race `expect` exists for -- and `remove` is `leave` \
             then drop, so removing there would disconnect a live session and bin its \
             queue."
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
