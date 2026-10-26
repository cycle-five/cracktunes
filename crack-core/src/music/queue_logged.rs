//! The queue's entry points that take a poise `Context`: they log what was
//! queued, clear autoplay's buffer and redraw the queue messages. The
//! primitives they call stay in `queue.rs`, which re-exports this file.

use super::queue::*;
use crate::music::from_ctx::ActorFromCtx;
use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    http_utils::{self, CacheHttpExt},
    messaging::{courier, message::CrackedMessage, transport::DiscordTransport},
    music::{
        audit::{Actor, AddAt},
        NewQueryType, PlaybackOwner, QueueGuard,
    },
    poise_ext::ContextExt,
    Context as CrackContext, Error,
};
use crack_testing::ResolvedTrack;
use crack_types::{Mode, NewAuxMetadata, QueryType};
use serenity::all::Message;
use songbird::{input::Input as SongbirdInput, tracks::TrackHandle, Call};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Takes a query and returns a track that is ready to be played, along with relevant metadata.
pub async fn ready_query(
    ctx: CrackContext<'_>,
    query_type: QueryType,
) -> Result<TrackReadyData, CrackedError> {
    let user_id = Some(ctx.author().id);
    let qt = NewQueryType(query_type);
    let (source, metadata_vec): (SongbirdInput, Vec<NewAuxMetadata>) =
        qt.get_track_source_and_metadata(None).await?;
    let metadata = match metadata_vec.first() {
        Some(x) => x.clone(),
        None => {
            return Err(CrackedError::Other("metadata.first() failed"));
        },
    };

    let username = match user_id {
        Some(x) => ctx.user_id_to_username_or_default(x).await,
        None => "(none)".to_string(),
    };

    Ok(TrackReadyData {
        source,
        metadata,
        user_id,
        username: Some(username),
    })
}

/// Pushes a track to the front of the queue.
///
/// Not itself in the guard-parameter group ([`queue_track_ready_front`] is):
/// `ready_query` is the slow leg (it can hit the network), so the guard is
/// acquired here internally, after readying completes, and held only for the
/// enqueue that follows. A guard taken by the caller before this call would
/// wrap the whole readying step, which is exactly the "second /play waits out
/// the first one's resolution" shape the funnel exists to avoid.
pub async fn queue_track_front(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // A user queued something, so autoplay's buffered picks -- chosen from
    // what played before -- no longer apply (Ruling 49).
    ctx.data().autoplay_buffer.clear(guild_id);
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    // Logged BEFORE the guard is taken and before `ready_track` is moved into
    // the enqueue below. The send is a non-blocking channel push, so it costs
    // the play path nothing.
    ctx.send_track_metadata_write_msg(&ready_track);
    let guard = ctx
        .data()
        .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
        .await?;
    let q = queue_track_ready_front(&guard, call, ready_track).await?;
    Ok(q)
}

/// Pushes a track to the front of the queue.
///
/// Not itself in the guard-parameter group ([`queue_resolved_track_back`] and
/// [`_queue_track_ready_back`] are): this function resolves the query
/// internally, and resolution is the slow leg (8-15s cold via `ct_client`, or
/// the `ready_query` fallback). The guard is acquired here, after resolution
/// completes on whichever branch was taken, and held only for the enqueue.
/// Requiring an externally-supplied guard instead would make the caller hold
/// playback exclusion for the whole resolution, which is the "second /play
/// waits out the first one's resolve" shape the funnel exists to avoid.
#[tracing::instrument(skip(ctx, call))]
pub async fn queue_track_back(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let user_id = ctx.author().id;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // A user queued something, so autoplay's buffered picks -- chosen from
    // what played before -- no longer apply (Ruling 49).
    ctx.data().autoplay_buffer.clear(guild_id);

    let begin = std::time::Instant::now();
    let resolved = match ctx.data().ct_client.resolve_track(query_type.clone()).await {
        Ok(resolved) => resolved.with_user_id(user_id),
        Err(e1) => {
            // 🪤 Say why before falling back. This once matched `e1.into()` as
            // an `Option` -- always `Some`, since `e1` is a boxed error -- and
            // dropped it, so a resolver failing on every play left no trace.
            tracing::warn!(
                "ct_client could not resolve {query_type:?}, falling back to ready_query: {e1}"
            );
            let ready_track = ready_query(ctx, query_type.clone()).await?;
            // 🪤 This branch RETURNS, so the send below never runs for it. A
            // play that fell back to `ready_query` is still a play and must be
            // logged here, or the fallback path stays silently unlogged exactly
            // as it was before this fix.
            ctx.send_track_metadata_write_msg(&ready_track);
            let guard = ctx
                .data()
                .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
                .await?;
            return _queue_track_ready_back(&guard, call, ready_track).await;
        },
    };
    let after_ready = std::time::Instant::now();
    // 🪤 `ready_track` is NOT in scope here -- it exists only in the fallback
    // branch above, which returns. This leg goes `ct_client` -> ResolvedTrack
    // and never builds one, which is why simply uncommenting the old
    // `//ctx.async_send_track_metadata_write_msg(&ready_track);` line did not
    // even compile, and why this path needs its own sender.
    ctx.send_resolved_metadata_write_msg(&resolved);
    let after_send = std::time::Instant::now();
    //let queue = queue_track_ready_back(call, ready_track).await;
    let guard = ctx
        .data()
        .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
        .await?;
    let queue =
        queue_resolved_track_back(&guard, call, resolved, http_utils::get_client_old().clone())
            .await;
    let after_queue = std::time::Instant::now();
    tracing::warn!(
        r#"
            after_ready: {:?}
            after_send: {:?}
            after_queue: {:?}
            total: {:?}
        "#,
        after_ready.duration_since(begin),
        after_send.duration_since(after_ready),
        after_queue.duration_since(after_send),
        after_queue.duration_since(begin)
    );
    queue
}

/// How many queries to resolve and enqueue per progress step.
///
/// Each batch is resolved with [`crack_testing::RESOLVE_CONCURRENCY`] lookups
/// in flight, so a batch costs roughly `BATCH / RESOLVE_CONCURRENCY` round
/// trips. Big enough to amortise, small enough that the queue visibly grows.
const QUEUE_BATCH_SIZE: usize = 24;

/// Minimum gap between progress message edits.
///
/// Discord rate-limits message edits per channel; editing once per batch on a
/// long playlist used to stall the load waiting on 429 backoff.
const PROGRESS_EDIT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

/// Where a playlist's progress line is edited: the placeholder reply, by
/// its channel and id.
fn progress_transport(ctx: &CrackContext<'_>) -> DiscordTransport {
    DiscordTransport {
        http: ctx.serenity_context().http.clone(),
        cache: ctx.serenity_context().cache.clone(),
    }
}

/// Queue a list of keywords to be played from the end of the queue.
///
/// The first track is resolved and queued on its own so playback starts
/// immediately; the remainder is resolved concurrently in batches behind it.
pub async fn queue_keyword_list_back(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    msg: &Message,
) -> Result<(), Error> {
    let (first, rest) = queries
        .split_first()
        .ok_or(CrackedError::Other("queries.first()"))?;

    // Get audio going before doing anything else -- the user should hear the
    // first track while the rest of the playlist is still being resolved.
    queue_vec_query_type(ctx, call.clone(), vec![first.clone()], Mode::End).await?;

    if rest.is_empty() {
        return Ok(());
    }

    let total = rest.len();
    let mut queued = 0usize;
    let mut last_edit = std::time::Instant::now();
    let transport = progress_transport(&ctx);

    for chunk in rest.chunks(QUEUE_BATCH_SIZE) {
        queue_vec_query_type(ctx, call.clone(), chunk.to_vec(), Mode::End).await?;
        queued += chunk.len();

        let is_last = queued >= total;
        if is_last || last_edit.elapsed() >= PROGRESS_EDIT_INTERVAL {
            last_edit = std::time::Instant::now();
            let description = if is_last {
                format!("Queued {total} additional tracks.")
            } else {
                format!("Queuing playlist... {queued}/{total}")
            };
            // A failed progress edit must not abort the load.
            if let Err(e) = courier::edit_message(
                &transport,
                msg.channel_id,
                msg.id,
                &CrackedMessage::Other(description),
            )
            .await
            {
                tracing::warn!("Failed to update queue progress message: {e:?}");
            }
        }
    }
    Ok(())
}

/// Queue an already-resolved list of tracks to the back of the queue.
///
/// Used for playlists, where every entry's metadata came back with the
/// playlist fetch itself and no per-track lookup is needed. The first track is
/// enqueued on its own so playback starts immediately.
pub async fn queue_resolved_list_back(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    tracks: Vec<ResolvedTrack<'static>>,
    msg: &Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // A user queued something, so autoplay's buffered picks -- chosen from
    // what played before -- no longer apply (Ruling 49).
    ctx.data().autoplay_buffer.clear(guild_id);
    let user_id = ctx.author().id;
    let client = http_utils::get_client_old().clone();

    let mut tracks = tracks
        .into_iter()
        .map(|t| t.with_user_id(user_id))
        .collect::<Vec<_>>();
    if tracks.is_empty() {
        return Err(CrackedError::Other("Playlist resolved to no playable tracks").into());
    }

    // Before `split_off`, so the whole list is logged rather than just the
    // head that gets enqueued first.
    ctx.send_resolved_metadata_write_msgs(&tracks);

    let rest = tracks.split_off(1);
    // Hold the guard across the enqueue AND the snapshot read right after it,
    // closing the window a concurrent command's insert could otherwise land
    // in -- the enqueue and the snapshot used to be two separate acquisitions
    // of just the call lock. Neither `build_track` nor the enqueue itself does
    // I/O (tracks here are already resolved), so this is never held across a
    // slow operation. See #333.
    let guard = ctx
        .data()
        .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
        .await?;
    enqueue_resolved_tracks_back(&guard, &call, tracks, client.clone()).await?;
    let snapshot = call.lock().await.queue().current_queue();
    drop(guard);
    update_queue_messages(
        &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
        ctx.data(),
        &snapshot,
        guild_id,
    )
    .await;

    if rest.is_empty() {
        return Ok(());
    }

    let total = rest.len();
    let mut queued = 0usize;
    let mut last_edit = std::time::Instant::now();
    let transport = progress_transport(&ctx);

    for chunk in rest.chunks(QUEUE_BATCH_SIZE) {
        // Same reasoning as the first batch above: guard held across the
        // enqueue and its snapshot, dropped before the (network) progress
        // message edit below. See #333.
        let guard = ctx
            .data()
            .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
            .await?;
        let inserted =
            enqueue_resolved_tracks_back(&guard, &call, chunk.to_vec(), client.clone()).await?;
        queued += inserted.count();
        let snapshot = call.lock().await.queue().current_queue();
        drop(guard);
        update_queue_messages(
            &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
            ctx.data(),
            &snapshot,
            guild_id,
        )
        .await;

        let is_last = queued >= total;
        if is_last || last_edit.elapsed() >= PROGRESS_EDIT_INTERVAL {
            last_edit = std::time::Instant::now();
            let description = if is_last {
                format!("Queued {total} additional tracks.")
            } else {
                format!("Queuing playlist... {queued}/{total}")
            };
            if let Err(e) = courier::edit_message(
                &transport,
                msg.channel_id,
                msg.id,
                &CrackedMessage::Other(description),
            )
            .await
            {
                tracing::warn!("Failed to update queue progress message: {e:?}");
            }
        }
    }
    Ok(())
}

/// Enqueue already-resolved tracks and return a snapshot of the queue taken
/// right after, while `guard` is still held -- closing the window a
/// concurrent command's insert could otherwise land in between the enqueue
/// and the snapshot. See #333.
///
/// Deliberately takes already-resolved tracks rather than doing the
/// `resolve_track_many` itself: resolution is the slow leg, and a caller must
/// do it before acquiring `guard`, not while holding it. [`queue_vec_query_type`]
/// and [`queue_query_list_offset`]'s low-queue branch are the two callers, and
/// each resolves on its own schedule before reaching here.
///
/// Deliberately does NOT call `update_queue_messages` itself: that is a
/// Discord HTTP round trip, and `guard` -- borrowed here, owned by the caller
/// -- must be dropped before it, not held across it (`lease.rs`: "held for
/// milliseconds ... do not hold one across a slow operation"). Callers drop
/// their guard, then redraw with the returned snapshot.
async fn enqueue_resolved_and_snapshot(
    guard: &QueueGuard,
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    resolved: Vec<ResolvedTrack<'static>>,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    debug_assert_eq!(
        guard.guild_id(),
        guild_id,
        "QueueGuard is for another guild"
    );
    enqueue_resolved_tracks_back(guard, call, resolved, http_utils::get_client_old().clone())
        .await?;
    // The whole queue, not what this call added -- `update_queue_messages`
    // redraws the whole queue message. See #333.
    Ok(call.lock().await.queue().current_queue())
}

/// Queue a list of keywords to be played with an offset.
///
/// Resolves first, then acquires a [`QueueGuard`] and holds it only across
/// the enqueue and its snapshot -- never across the resolve above (the slow
/// leg) or the Discord round trip below. See [`enqueue_resolved_and_snapshot`].
pub async fn queue_vec_query_type(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    _mode: Mode,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // A user queued something, so autoplay's buffered picks -- chosen from
    // what played before -- no longer apply (Ruling 49).
    ctx.data().autoplay_buffer.clear(guild_id);
    let user_id = ctx.author().id;

    // This used to be a serial `for` loop calling `ready_query`, which spawned
    // a `yt-dlp` subprocess per track and waited for it. `resolve_track_many`
    // overlaps the lookups and skips individual failures.
    let resolved = ctx
        .data()
        .ct_client
        .resolve_track_many(queries)
        .await?
        .into_iter()
        .map(|t| t.with_user_id(user_id))
        .collect::<Vec<_>>();

    // Logged before the guard is taken: the send is a non-blocking channel
    // push, and keeping it off the guarded section costs playback nothing.
    ctx.send_resolved_metadata_write_msgs(&resolved);

    let guard = ctx
        .data()
        .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
        .await?;
    let snapshot = enqueue_resolved_and_snapshot(&guard, ctx, &call, resolved).await?;
    drop(guard);
    update_queue_messages(
        &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
        ctx.data(),
        &snapshot,
        guild_id,
    )
    .await;
    Ok(())
}

/// Queue a list of queries to be played with a given offset.
/// N.B. The offset must be 0 < offset < queue.len() + 1
///
/// Resolves first (the slow leg -- worse than the usual 8-15s here, since a
/// big playlist batches through `RESOLVE_CONCURRENCY` lookups at a time), then
/// acquires a [`QueueGuard`] and reads *and* acts on the queue length under
/// that one guard, so the length cannot go stale between the check and the
/// insert -- it used to be read before resolving and acted on afterward,
/// against whatever the queue had become in between.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub async fn queue_query_list_offset(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    offset: usize,
    _search_msg: &mut Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // A user queued something, so autoplay's buffered picks -- chosen from
    // what played before -- no longer apply (Ruling 49).
    ctx.data().autoplay_buffer.clear(guild_id);
    let user_id = ctx.author().id;

    // Resolved concurrently; this was a serial round trip per track. Runs
    // before the guard is taken -- see the doc comment above.
    let tracks = ctx.data().ct_client.resolve_track_many(queries).await?;

    // 🪤 Sent HERE, once, rather than inside the two branches below. This
    // function splits into a low-queue path (`enqueue_resolved_and_snapshot`)
    // and a bulk loop; a send placed in either one silently misses the other,
    // which is the shape of the bug this whole fix exists to close.
    ctx.send_resolved_metadata_write_msgs(&tracks);

    let guard = ctx
        .data()
        .lock_queue(guild_id, PlaybackOwner::Free, Actor::from_ctx(&ctx))
        .await?;
    let queue_size = {
        let handler = call.lock().await;
        handler.queue().len()
    };

    if queue_size <= 1 {
        let resolved = tracks
            .into_iter()
            .map(|t| t.with_user_id(user_id))
            .collect::<Vec<_>>();
        let snapshot = enqueue_resolved_and_snapshot(&guard, ctx, &call, resolved).await?;
        drop(guard);
        update_queue_messages(
            &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
            ctx.data(),
            &snapshot,
            guild_id,
        )
        .await;
        return Ok(());
    }

    verify(
        offset > 0 && offset <= queue_size + 1,
        CrackedError::NotInRange("index", offset as isize, 1, queue_size as isize),
    )?;

    // One lock for the whole insert. The `Compose` per track is lazy, but
    // `enqueue` is not: it reads the duration back off the input to schedule
    // preload, which runs yt-dlp. `enqueue_with_preload` is what actually keeps
    // this loop lazy -- see [`preload_time`].
    let client = http_utils::get_client_old().clone();
    let cur_q = {
        let mut handler = call.lock().await;
        let mut queued = Vec::new();
        for (idx, resolved) in tracks.into_iter().enumerate() {
            let track = match build_track(&resolved, &client) {
                Ok(track) => track,
                Err(e) => {
                    tracing::warn!("Failed to build track {}: {e}", resolved.get_url());
                    continue;
                },
            };
            queued.push(enqueue(&mut handler, track, preload_time(&resolved)));
            handler.queue().modify_queue(|q| {
                if let Some(back) = q.pop_back() {
                    q.insert((idx + offset).min(q.len()), back);
                }
            });
        }
        record_add(&guard, &handler, &queued, AddAt::Index(offset));
        handler.queue().current_queue()
    };
    drop(guard);

    update_queue_messages(
        &crate::messaging::transport::DiscordTransport::of(ctx.serenity_context()),
        ctx.data(),
        &cur_q,
        guild_id,
    )
    .await;

    Ok(())
}

#[cfg(test)]
mod play_history_wiring_tests {
    //! Regression guard for #486, REWRITTEN after the first version of it
    //! passed while the bug was still live.
    //!
    //! v0.9.3 wired the three single-track call sites and this test asserted
    //! exactly those three. It was green, and a Spotify playlist still wrote
    //! nothing -- because the LIST paths (`queue_query_list_offset`,
    //! `queue_vec_query_type`, `queue_resolved_list_back`) had no metadata
    //! write at all and the test never knew to look for one.
    //!
    //! 🔑 **A source scan is only ever as good as the set it enumerates.** The
    //! first version hard-coded a count of call sites, so it encoded the
    //! author's survey rather than the property. This version pins the whole
    //! enqueue SURFACE: every public enqueue entry point is listed with what it
    //! is expected to do, and a function added, renamed or removed fails the
    //! test until someone states which case it is. That is the only shape that
    //! could have caught the playlist gap.

    /// The non-test part of `queue.rs` and `queue_logged.rs`, one after the other.
    ///
    /// 🪤 Stops at the first `#[cfg(test)]` of each file. This module names the very
    /// functions and senders it counts, so scanning the whole file would match
    /// its own text and pass no matter what the real code did.
    fn production_source() -> String {
        ["src/music/queue.rs", "src/music/queue_logged.rs"]
            .iter()
            .map(|path| {
                let src = std::fs::read_to_string(path).expect("readable");
                let end = src
                    .find("#[cfg(test)]")
                    .expect("this file has test modules");
                src[..end].to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// What each public enqueue entry point is expected to do about history.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expect {
        /// Takes `ctx` and must contain a live metadata-sender call.
        Logs,
        /// Takes `ctx` but enqueues only via the named entry point, which logs.
        Delegates(&'static str),
        /// Takes a `QueueGuard` and NO `ctx`, so it structurally cannot log --
        /// it has no channel to send on. Its ctx-bearing callers do.
        ///
        /// 🪤 There is deliberately NO `Exempt` variant for "takes ctx but
        /// should not log". Nothing needs one today, and its absence is what
        /// forces the conversation: such a path cannot be marked `Primitive`
        /// (that assertion checks for the absence of a ctx) and cannot be
        /// marked `Logs` without actually logging, so it fails the suite until
        /// someone adds the variant along with a written reason.
        Primitive(&'static str),
    }
    use Expect::{Delegates, Logs, Primitive};

    /// THE PINNED SURFACE. Adding a `pub async fn queue_*`/`enqueue_*` without
    /// adding it here fails `the_enqueue_surface_is_exactly_what_we_reviewed`.
    const SURFACE: &[(&str, Expect)] = &[
        ("queue_track_front", Logs),
        ("queue_track_back", Logs),
        ("queue_resolved_list_back", Logs),
        ("queue_vec_query_type", Logs),
        ("queue_query_list_offset", Logs),
        ("queue_keyword_list_back", Delegates("queue_vec_query_type")),
        (
            "queue_track_ready_front",
            Primitive("caller: queue_track_front"),
        ),
        (
            "queue_resolved_track_back",
            Primitive("caller: queue_track_back"),
        ),
        (
            "enqueue_resolved_tracks_back",
            Primitive(
                "callers: queue_vec_query_type, queue_resolved_list_back, ops::add \
                 (which sends its own play-log message)",
            ),
        ),
        (
            "enqueue_resolved_track_next",
            Primitive("caller: ops::add, which sends its own play-log message"),
        ),
        (
            "enqueue_track_back",
            Primitive("caller: gp.rs, which keeps submissions out of history before the reveal"),
        ),
        (
            "enqueue_input_back",
            Primitive(
                "caller: track_end.rs autoplay. ⚠️ KNOWN GAP: an autoplayed track \
                 is not logged. Autoplay is live again through crack-musicreco \
                 (off by default, session-only); whether autoplayed plays belong \
                 in play history is still undecided.",
            ),
        ),
    ];

    /// Any call that puts metadata on the db worker channel.
    const SENDERS: &[&str] = &[
        "send_track_metadata_write_msg(",
        "send_resolved_metadata_write_msg(",
        "send_resolved_metadata_write_msgs(",
    ];

    /// Every `pub async fn` in the production source whose name enqueues.
    fn declared_entry_points(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in src.lines() {
            let line = line.trim_start();
            let Some(rest) = line.strip_prefix("pub async fn ") else {
                continue;
            };
            let Some(name) = rest.split('(').next() else {
                continue;
            };
            if name.starts_with("queue_") || name.starts_with("enqueue_") {
                out.push(name.to_owned());
            }
        }
        out
    }

    /// A function's body: its signature line through to the next `pub async fn`.
    fn body_of<'a>(src: &'a str, name: &str) -> &'a str {
        let sig = format!("pub async fn {name}(");
        let start = src.find(&sig).unwrap_or_else(|| panic!("{name} not found"));
        let after = &src[start + sig.len()..];
        match after.find("\npub async fn ") {
            Some(end) => &after[..end],
            None => after,
        }
    }

    fn has_live_sender(body: &str) -> bool {
        body.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .any(|l| SENDERS.iter().any(|s| l.contains(s)))
    }

    #[test]
    fn every_queue_entry_point_is_in_one_of_the_two_files() {
        let src = production_source();
        for (name, _) in SURFACE {
            assert!(
                src.contains(&format!("fn {name}")),
                "{name} is in SURFACE but defined in neither queue.rs nor queue_logged.rs"
            );
        }
    }

    #[test]
    fn the_enqueue_surface_is_exactly_what_we_reviewed() {
        let src = production_source();
        let mut found = declared_entry_points(&src);
        found.sort();
        let mut pinned: Vec<String> = SURFACE.iter().map(|(n, _)| (*n).to_owned()).collect();
        pinned.sort();

        assert_eq!(
            found, pinned,
            "the set of public enqueue entry points changed. Add the new one to \
             SURFACE with Logs / DelegatesTo / Exempt -- an unreviewed enqueue \
             path is exactly how playlists went unlogged through v0.9.3 while \
             this test was green."
        );
    }

    #[test]
    fn every_logging_entry_point_actually_sends() {
        let src = production_source();
        let mut missing = Vec::new();
        for (name, expect) in SURFACE {
            if *expect != Logs {
                continue;
            }
            if !has_live_sender(body_of(&src, name)) {
                missing.push(*name);
            }
        }
        assert!(
            missing.is_empty(),
            "these enqueue paths are marked Logs but contain no live metadata \
             sender, so plays through them vanish silently -- no error, no log \
             line, just an empty play_log: {missing:#?}"
        );
    }

    #[test]
    fn delegation_targets_exist_and_log() {
        let src = production_source();
        for (name, expect) in SURFACE {
            let Delegates(target) = expect else { continue };
            let entry = SURFACE
                .iter()
                .find(|(n, _)| n == target)
                .unwrap_or_else(|| panic!("{name} delegates to unknown {target}"));
            assert_eq!(
                entry.1, Logs,
                "{name} delegates to {target}, which is not marked Logs -- a \
                 delegation chain has to end somewhere that writes"
            );
            assert!(
                body_of(&src, name).contains(target),
                "{name} is marked as delegating to {target} but does not call it"
            );
        }
    }

    /// Ruling 49: every entry point a user queues through clears autoplay's
    /// buffer, so the next autoplay pick is seeded from what they chose rather
    /// than from a list made for an earlier track. `Logs` is exactly the set
    /// with a user behind it; autoplay's own path is a `Primitive` and must not
    /// clear what it is draining.
    #[test]
    fn every_user_entry_point_clears_the_autoplay_buffer() {
        let src = production_source();
        let missing: Vec<&str> = SURFACE
            .iter()
            .filter(|(_, expect)| *expect == Logs)
            .map(|(name, _)| *name)
            .filter(|name| {
                !body_of(&src, name)
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .any(|l| l.contains("autoplay_buffer.clear("))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "these enqueue paths have a user behind them but leave autoplay's \
             buffer alone, so autoplay would carry on from a stale list: {missing:#?}"
        );
    }

    /// A `Primitive` is only safe to leave unlogged because it has no `ctx` --
    /// it physically cannot reach the db channel, so responsibility sits with
    /// its caller. If one ever gains a `ctx`, that reasoning evaporates and
    /// this test says so.
    #[test]
    fn primitives_really_have_no_context_to_log_with() {
        let src = production_source();
        for (name, expect) in SURFACE {
            let Primitive(note) = expect else { continue };
            let sig_end = body_of(&src, name)
                .find(')')
                .expect("a signature has a closing paren");
            let params = &body_of(&src, name)[..sig_end];
            assert!(
                !params.contains("ctx: CrackContext"),
                "{name} is marked Primitive ({note}) but now takes a CrackContext -- \
                 it can reach the db channel, so it must either log or be \
                 re-marked Exempt with a reason"
            );
        }
    }
}
