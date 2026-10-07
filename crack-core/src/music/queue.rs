use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    http_utils::CacheHttpExt,
    messaging::{courier, message::CrackedMessage, transport::DiscordTransport},
    music::{
        audit::{track_ref, Action, Actor, AddAt},
        NewQueryType, PlaybackOwner, QueueGuard,
    },
    poise_ext::ContextExt,
    utils::TrackData,
    Context as CrackContext, Error,
};
use crack_testing::ResolvedTrack;
use crack_types::{Mode, NewAuxMetadata, QueryType};
use rand::RngExt;
use serenity::{
    all::{Message, UserId},
    small_fixed_array::FixedString,
};
use songbird::{
    input::{AuxMetadata, Input as SongbirdInput},
    tracks::{Track, TrackHandle, TrackResult},
    Call,
};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, MutexGuard};

/// Takes a resolved track and queues it to the back of the queue.
/// Returns a snapshot of th new queue as a [`Vec<TrackHandle>`].
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
/// # Errors
/// Returns a [`CrackedError`] if the track cannot be queued.
/// Can fail during the search itself, or when adding the metadata to the track,
/// or when adding the track to the internal queue.
pub async fn queue_resolved_track_back(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track_resolved: ResolvedTrack<'static>,
    http_client: reqwest::Client,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // Through `build_track`, not a second copy of it. This function used to
    // construct its own `RustyYoutubeSearch` inline, which is why fixing the
    // source in `build_track` fixed `/gp` and left `/play` still silent.
    let track = build_track(&track_resolved, &http_client)?;
    let mut handler = call.lock().await;
    let track_handle = enqueue(&mut handler, track, preload_time(&track_resolved));
    record_add(guard, &handler, &[track_handle], AddAt::Back);
    // .enqueue_input(Into::<SongbirdInput>::into(track))
    let new_q = handler.queue().current_queue();
    drop(handler);
    // if let Some(metadata) = track_resolved.metadata {
    //     set_track_handle_metadata(&mut track_handle, metadata.clone()).await?;
    // }
    // set_track_handle_requesting_user(&mut track_handle, track_resolved.user_id).await?;

    Ok(new_q)
}

/// Build the songbird [`Track`] for an already-resolved track.
///
/// This performs no I/O: [`YoutubeDl`] is a lazy [`Compose`], so yt-dlp is not
/// run and the stream is not opened until the track reaches the front of the
/// queue.
///
/// Every playback path goes through here. Two of them used to build their own
/// source inline instead, which is how `/play` stayed broken after `/gp` was
/// fixed -- keep it that way.
///
/// [`Compose`]: songbird::input::Compose
pub(crate) fn build_track(
    resolved: &ResolvedTrack<'static>,
    http_client: &reqwest::Client,
) -> Result<QueuedTrack, CrackedError> {
    // yt-dlp, not rusty_ytdl. rusty_ytdl resolves fine but the googlevideo URL
    // it hands back is rejected: `c=ANDROID` fetches 403, songbird gets an empty
    // stream, and symphonia reports it as "no suitable format reader found",
    // which reads like a codec problem and is not one. yt-dlp negotiates a
    // client whose URL actually serves (`c=VISIONOS` at the time of writing) and
    // keeps up with YouTube's changes, which is the whole reason it exists.
    //
    // Needs `yt-dlp` on PATH -- see the Dockerfile, and note it must be the musl
    // build on this Alpine base.
    let ytdl = crack_types::ytdl_for_url(http_client.clone(), &resolved.get_url())?;
    Ok(new_track(
        ytdl.into(),
        resolved.metadata.clone(),
        Some(resolved.user_id),
    ))
}

/// When to start loading the *next* track, given what we already know about this
/// one.
///
/// # 🪤 This exists to keep yt-dlp out of the guard
///
/// `Call::enqueue` (songbird `driver/mod.rs:301`) calls
/// `TrackQueue::get_preload_time`, which does
/// `Input::Lazy(rec).aux_metadata().await` purely to read the duration.
/// [`build_track`] hands songbird a `YoutubeDl` with `metadata: None`, so that
/// `aux_metadata` falls through to `query(1)` and **spawns yt-dlp** -- 1-3s, per
/// track, inside the enqueue, inside the [`QueueGuard`]. A 20-track playlist
/// paid it twenty times and made a second `/play` in the same guild wait out
/// every one of them.
///
/// songbird's `YoutubeDl::metadata` is private with no setter on the pinned rev
/// (`3fe7289`), so it cannot be primed; `enqueue_with_preload` is the way out.
/// This computes exactly what `get_preload_time` would have -- duration minus
/// five seconds -- from the [`ResolvedTrack`] metadata resolution already
/// produced, so preload is preserved rather than disabled.
///
/// `None` (no metadata, or metadata with no duration) disables preload for that
/// track: it is readied when the previous one ends instead of five seconds
/// early. That is a small gap, not a failure, and it is what songbird itself
/// falls back to when the query yields no duration.
pub(crate) fn preload_time(resolved: &ResolvedTrack<'_>) -> Option<Duration> {
    preload_from_metadata(resolved.metadata.as_ref())
}

/// [`preload_time`] for the paths that hold [`AuxMetadata`] rather than a
/// [`ResolvedTrack`]. The five seconds is songbird's own figure, from
/// `TrackQueue::get_preload_time`.
pub(crate) fn preload_from_metadata(meta: Option<&AuxMetadata>) -> Option<Duration> {
    meta.and_then(|meta| meta.duration)
        .map(|d| d.saturating_sub(Duration::from_secs(5)))
}

/// What one enqueue call put into the queue.
///
/// 🔑 Deliberately NOT the whole queue. `enqueue_resolved_tracks_back` used to
/// return `handler.queue().current_queue()` *after* enqueueing, so two
/// concurrent `/play` calls both read the post-both state and both replies
/// listed both sets of songs -- #333, reported as "one of the songs got queued
/// twice". A reply built from what *this* call inserted cannot be corrupted by
/// a concurrent one.
#[derive(Debug, Clone)]
pub struct Inserted {
    /// The handles this call added, in the order they were added.
    pub handles: Vec<TrackHandle>,
}

impl Inserted {
    /// How many tracks this call added.
    #[must_use]
    pub fn count(&self) -> usize {
        self.handles.len()
    }
}

/// Queue a batch of resolved tracks to the back of the queue.
///
/// Takes the call lock once for the whole batch instead of once per track,
/// which matters when a playlist adds tens of tracks at a time.
///
/// Returns only what THIS call inserted -- see [`Inserted`]. Callers that also
/// want a whole-queue snapshot (to redraw a queue message, say) take it
/// explicitly and separately; see #333.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn enqueue_resolved_tracks_back(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    tracks: Vec<ResolvedTrack<'static>>,
    http_client: reqwest::Client,
) -> Result<Inserted, CrackedError> {
    let mut handler = call.lock().await;
    let mut handles = Vec::with_capacity(tracks.len());
    for resolved in &tracks {
        match build_track(resolved, &http_client) {
            // An explicit preload, not a derived one: see [`enqueue`]. This is
            // the loop that made it matter -- a 20-track playlist ran yt-dlp
            // twenty times here, under the guard, before this.
            Ok(track) => handles.push(enqueue(&mut handler, track, preload_time(resolved))),
            Err(e) => tracing::warn!("Failed to enqueue {}: {e}", resolved.get_url()),
        }
    }
    record_add(guard, &handler, &handles, AddAt::Back);
    Ok(Inserted { handles })
}

/// Data needed to queue a track.
/// TODO: This is mostly become redundant with ResolvedTrack, need to clean this up.
pub struct TrackReadyData {
    pub source: SongbirdInput,
    pub metadata: NewAuxMetadata,
    pub user_id: Option<UserId>,
    pub username: Option<String>,
}

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

/// A songbird [`Track`] that carries the [`TrackData`] every reader expects.
///
/// Only [`new_track`] makes one, and only [`enqueue`] queues one. `clippy.toml`
/// bans songbird's `Track` constructors and every songbird method that queues
/// or plays a bare `Track` or `Input`, so a track built any other way -- the
/// blanket `Track::from(input)` cannot be banned -- has nowhere to go.
pub struct QueuedTrack(Track);

/// Build a [`QueuedTrack`]: the crate's one songbird `Track` constructor.
///
/// 🪤 `/queue`, `/nowplaying`, `/lyrics` and autoplay's refill read every
/// queued track through `TrackHandle::data::<TrackData>()`, which panics on a
/// track built any other way: `Track::from(input)` attaches `()`. Writing the
/// data in after enqueueing reads it first and panics the same way. Until
/// v0.12.1 `/play mode:next` and autoplay each built their track like that, and
/// autoplay's panic took songbird's event task for the call down with it.
///
/// A track with no requester (autoplay's) is recorded as user 1, as it always
/// has been.
#[allow(clippy::disallowed_methods)]
pub(crate) fn new_track(
    source: SongbirdInput,
    metadata: Option<AuxMetadata>,
    requester: Option<UserId>,
) -> QueuedTrack {
    let data = TrackData::new().with_user_id(requester.unwrap_or(UserId::new(1)));
    let data = match metadata {
        Some(metadata) => data.with_metadata(metadata),
        None => data,
    };
    // Every queued track passes here, so this is where a refused stream URL
    // gets its second try: see `sources::retry`.
    QueuedTrack(Track::new_with_data(
        crate::sources::retry::retry_refused(source),
        data,
    ))
}

/// Put a [`QueuedTrack`] at the back of `call`'s queue.
///
/// **The crate's only way into a songbird queue**; `clippy.toml` bans the rest
/// (see [`QueuedTrack`]).
///
/// `preload` is explicit rather than left to songbird, which derives it by
/// reading the duration back off the input -- and for a lazy source that runs
/// yt-dlp, under the queue guard. See [`preload_time`].
#[allow(clippy::disallowed_methods)]
fn enqueue(call: &mut Call, track: QueuedTrack, preload: Option<Duration>) -> TrackHandle {
    call.enqueue_with_preload(track.0, preload)
}

/// Record one `Add` for everything a single entry-point call queued. Nothing
/// queued, nothing recorded.
fn record_add(guard: &QueueGuard, handler: &Call, handles: &[TrackHandle], at: AddAt) {
    if handles.is_empty() {
        return;
    }
    guard.record(
        handler.current_channel(),
        Action::Add {
            tracks: handles.iter().map(track_ref).collect(),
            at,
        },
    );
}

/// Pushes a track to the front of the queue, after readying it.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub async fn queue_track_ready_front(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let TrackReadyData {
        source,
        metadata,
        user_id,
        ..
    } = ready_track;
    // An explicit preload: see [`enqueue`]. The readied metadata is right
    // here, so songbird never has to go and ask.
    let preload = preload_from_metadata(Some(&metadata.0));
    let track = new_track(source, Some(metadata.into()), user_id);
    let mut handler = call.lock().await;
    let track_handle = enqueue(&mut handler, track, preload);
    let new_q = handler.queue().current_queue();
    // Zeroth index: Currently playing track
    // First index: Current next track
    // Second index onward: Tracks to be played, we get in here most likely,
    // but if we're in one of the first two we don't want to do anything.
    if new_q.len() >= 3 {
        handler.queue().modify_queue(|queue| {
            let back = queue.pop_back().unwrap();
            queue.insert(1, back);
        });
    }

    record_add(guard, &handler, &[track_handle], AddAt::Front);
    drop(handler);
    Ok(new_q)
}

/// Pushes a track to the back of the queue, after readying it.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn _queue_track_ready_back(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;

    let TrackReadyData {
        source,
        metadata,
        user_id,
        ..
    } = ready_track;

    // Computed before `metadata` is moved into the track below.
    let preload = preload_from_metadata(Some(&metadata.0));
    let track = new_track(source, Some(metadata.into()), user_id);
    let track_handle = enqueue(&mut handler, track, preload);
    record_add(guard, &handler, &[track_handle], AddAt::Back);
    let new_q = handler.queue().current_queue();
    drop(handler);

    Ok(new_q)
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
#[cfg(not(tarpaulin_include))]
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
#[cfg(not(tarpaulin_include))]
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
    update_queue_messages(&ctx, ctx.data(), &snapshot, guild_id).await;

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
        update_queue_messages(&ctx, ctx.data(), &snapshot, guild_id).await;

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
#[cfg(not(tarpaulin_include))]
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
#[cfg(not(tarpaulin_include))]
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
    update_queue_messages(&ctx, ctx.data(), &snapshot, guild_id).await;
    Ok(())
}

use crate::http_utils;
/// Queue a list of queries to be played with a given offset.
/// N.B. The offset must be 0 < offset < queue.len() + 1
///
/// Resolves first (the slow leg -- worse than the usual 8-15s here, since a
/// big playlist batches through `RESOLVE_CONCURRENCY` lookups at a time), then
/// acquires a [`QueueGuard`] and reads *and* acts on the queue length under
/// that one guard, so the length cannot go stale between the check and the
/// insert -- it used to be read before resolving and acted on afterward,
/// against whatever the queue had become in between.
#[cfg(not(tarpaulin_include))]
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
        update_queue_messages(&ctx, ctx.data(), &snapshot, guild_id).await;
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

    update_queue_messages(&ctx, ctx.data(), &cur_q, guild_id).await;

    Ok(())
}

/// Drop everything from `from` onward, stopping each track. Used by `/clear`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn clear_from(guard: &QueueGuard, handler: &Call, from: usize) {
    let removed = handler.queue().modify_queue(|v| {
        let mut removed = 0;
        v.drain(from..).for_each(|x| {
            let _ = x.stop();
            drop(x);
            removed += 1;
        });
        removed
    });
    if removed > 0 {
        guard.record(handler.current_channel(), Action::Clear { removed });
    }
}

/// Drop `count` tracks after the currently-playing one. Used by `/skip`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn drain_after_current(guard: &QueueGuard, handler: &Call, count: usize) {
    let removed = handler.queue().modify_queue(|v| {
        let end = (1 + count).min(v.len());
        if end <= 1 {
            return 0;
        }
        v.drain(1..end).count()
    });
    if removed > 0 {
        guard.record(handler.current_channel(), Action::Clear { removed });
    }
}

/// Reorder the queue behind the currently-playing track. Used by `/shuffle`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn shuffle_behind_current(guard: &QueueGuard, handler: &Call) {
    let count = handler.queue().modify_queue(|queue| {
        // Fewer than two upcoming tracks cannot be reordered.
        let upcoming = queue.len().saturating_sub(1);
        if upcoming < 2 {
            return 0;
        }
        // skip the first track on queue because it's being played
        fisher_yates(queue.make_contiguous()[1..].as_mut(), &mut rand::rng());
        upcoming
    });
    if count >= 2 {
        guard.record(handler.current_channel(), Action::Shuffle { count });
    }
}

/// Move a track from one queue index to another. Used by `/movesong`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn move_track(guard: &QueueGuard, handler: &Call, at: usize, to: usize) {
    if at == to {
        return;
    }
    let track = handler.queue().modify_queue(|queue| {
        // The caller verifies both indices are in range before calling.
        let song = queue.remove(at).expect("index out of bounds");
        let moved = track_ref(&song.handle());
        queue.insert(to, song);
        moved
    });
    guard.record(
        handler.current_channel(),
        Action::Move {
            track,
            from: at,
            to,
        },
    );
}

/// Move a track, named by its id, to a position among the upcoming tracks.
/// Used by the web dashboard, whose view of the queue can be seconds old:
/// an index would move whichever track now sits there, an id cannot.
///
/// `to_upcoming` is 0-based among the tracks after the one playing and is
/// clamped into range; the position actually used is returned. The playing
/// track never moves, and an id no longer queued moves nothing.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn move_track_by_id(
    guard: &QueueGuard,
    handler: &Call,
    id: uuid::Uuid,
    to_upcoming: usize,
) -> Result<usize, crate::music::remote::MoveRefused> {
    use crate::music::remote::MoveRefused;
    let (at, to, track) = handler.queue().modify_queue(|queue| {
        let at = queue
            .iter()
            .position(|q| q.uuid() == id)
            .ok_or(MoveRefused::Absent)?;
        if at == 0 {
            return Err(MoveRefused::NowPlaying);
        }
        // `at >= 1` means at least two tracks, so the range is never empty.
        let to = to_upcoming.saturating_add(1).clamp(1, queue.len() - 1);
        let track = queue.remove(at).expect("the position came from this queue");
        let moved = track_ref(&track.handle());
        queue.insert(to, track);
        Ok((at, to, moved))
    })?;
    if at != to {
        guard.record(
            handler.current_channel(),
            Action::Move {
                track,
                from: at - 1,
                to: to - 1,
            },
        );
    }
    Ok(to - 1)
}

/// Remove one track by queue index. Used by `/remove`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn remove_at(guard: &QueueGuard, handler: &Call, index: usize) {
    let removed = handler.queue().modify_queue(|v| {
        v.remove(index).map(|track| {
            let gone = track_ref(&track.handle());
            let _ = track.stop();
            gone
        })
    });
    if let Some(track) = removed {
        guard.record(handler.current_channel(), Action::Remove { track, index });
    }
}

/// Stop everything in the queue. Used by `/gp start`, `/gp end` and the abort
/// path.
///
/// Takes the already-locked `handler` rather than the `Arc<Mutex<Call>>`, like
/// the other synchronous helpers above: two of its three callers read
/// `queue().is_empty()` under the same lock to report what they cleared, and a
/// helper that re-locked internally would deadlock them.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
///
/// # 🪤 This fires `TrackEvent::End`
///
/// `stop()` queues the `End` rather than firing it inline, and songbird's event
/// task dispatches handlers inline. So a handler that awaits `lock_queue` parks
/// that task until this caller's guard drops -- release it before anything slow
/// (Discord HTTP, resolution, the database), never after.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn stop_queue(guard: &QueueGuard, handler: &Call) {
    let removed = handler.queue().len();
    handler.queue().stop();
    if removed > 0 {
        guard.record(handler.current_channel(), Action::Stop { removed });
    }
}

/// Pause the queue. Used by `/pause` and by autopause in the global track-end
/// handler.
///
/// Same `&Call` shape as [`stop_queue`]. The failure is handed back rather than
/// swallowed, because the two callers want opposite things with it: `/pause`
/// has a user to tell, autopause does not.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn pause_queue(guard: &QueueGuard, handler: &Call) -> TrackResult<()> {
    // songbird answers Ok on an empty queue, and autopause lands here after
    // every session: nothing paused, so nothing to record.
    if handler.queue().current().is_none() {
        return Ok(());
    }
    handler.queue().pause()?;
    guard.record(handler.current_channel(), Action::Pause);
    Ok(())
}

/// Resume the queue. Used by `/resume`.
///
/// Same `&Call` shape as [`stop_queue`].
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. 🔴 That is not belt-and-braces here: `/resume` was in neither
/// `GP_BLOCKED_COMMANDS` nor the funnel, so it was the one queue mutation a
/// user could still land on a running `/gp` round. The guard is what closes it;
/// the blocklist entry added alongside only makes the refusal arrive earlier.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub fn resume_queue(guard: &QueueGuard, handler: &Call) -> TrackResult<()> {
    // As in `pause_queue`: nothing to resume is not a change.
    if handler.queue().current().is_none() {
        return Ok(());
    }
    handler.queue().resume()?;
    guard.record(handler.current_channel(), Action::Resume);
    Ok(())
}

/// Enqueue an already-built [`QueuedTrack`] at the back of the queue, returning
/// its handle. Used by `/gp` to play a round's song.
///
/// Returns the [`TrackHandle`] rather than a queue snapshot because the caller
/// arms per-track event handlers on it; [`queue_resolved_track_back`] is the
/// snapshot-returning shape.
///
/// `preload` is passed explicitly rather than letting songbird derive it,
/// because deriving it spawns yt-dlp inside the guard -- see [`preload_time`],
/// which is how the caller should compute this.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn enqueue_track_back(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track: QueuedTrack,
    preload: Option<Duration>,
) -> TrackHandle {
    let mut handler = call.lock().await;
    let handle = enqueue(&mut handler, track, preload);
    record_add(guard, &handler, std::slice::from_ref(&handle), AddAt::Back);
    handle
}

/// Enqueue an already-resolved songbird [`Input`](SongbirdInput) at the back of
/// the queue, returning its handle. Used by autoplay in the global track-end
/// handler.
///
/// `preload` is passed explicitly for the same reason as
/// [`enqueue_track_back`]: `enqueue_input` would derive it by spawning yt-dlp
/// under the guard. The caller has the resolved metadata and can supply it.
///
/// `metadata` goes into the track as it is built, not afterwards: see
/// [`new_track`]. An autoplayed track has no requester.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn enqueue_input_back(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    source: SongbirdInput,
    metadata: Option<AuxMetadata>,
    preload: Option<Duration>,
) -> TrackHandle {
    let mut handler = call.lock().await;
    let handle = enqueue(&mut handler, new_track(source, metadata, None), preload);
    record_add(guard, &handler, std::slice::from_ref(&handle), AddAt::Back);
    handle
}

/// Do the actual skipping of the top track.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
#[cfg(not(tarpaulin_include))]
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub async fn force_skip_top_track(
    guard: &QueueGuard,
    handler: &MutexGuard<'_, Call>,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // this is an odd sequence of commands to ensure the queue is properly updated
    // apparently, skipping/stopping a track takes a while to remove it from the queue
    // also, manually removing tracks doesn't trigger the next track to play
    // so first, stop the top song, manually remove it and then resume playback
    // #480: the caller saw something playing, but a track that ends on its own
    // in between empties the queue first. Nothing left to stop is already
    // skipped; everything below is safe on an empty queue.
    let skipped = handler.queue().current().map(|track| {
        // Named before the stop and the dequeue: it is the track that was playing.
        let named = track_ref(&track);
        track.stop().ok();
        named
    });
    let _ = handler.queue().dequeue(0);
    handler.queue().resume().ok();
    if let Some(track) = skipped {
        guard.record(
            handler.current_channel(),
            Action::Skip { track: Some(track) },
        );
    }

    Ok(handler.queue().current_queue())
}

/// Shuffle `values` in place using the Fisher-Yates algorithm.
fn fisher_yates<T, R>(values: &mut [T], mut rng: R)
where
    R: rand::Rng + Sized,
{
    let mut index = values.len();
    while index >= 2 {
        index -= 1;
        values.swap(index, rng.random_range(0..(index + 1)));
    }
}

/// Get the play mode and the message from the parameters to the play command.
// TODO: There is a lot of cruft in this from the older version of this. Clean it up.
#[tracing::instrument]
pub fn get_mode(
    is_prefix: bool,
    msg: Option<FixedString>,
    mode: Option<FixedString>,
) -> (Mode, FixedString) {
    let opt_mode = mode.clone();
    if is_prefix {
        let asdf2 = msg
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default();
        let asdf = asdf2.split_whitespace().next().unwrap_or_default();
        let mode = if asdf.starts_with("next") {
            Mode::Next
        } else if asdf.starts_with("all") {
            Mode::All
        } else if asdf.starts_with("shuffle") {
            Mode::Shuffle
        } else if asdf.starts_with("reverse") {
            Mode::Reverse
        } else if asdf.starts_with("jump") {
            Mode::Jump
        } else if asdf.starts_with("downloadmkv") {
            Mode::DownloadMKV
        } else if asdf.starts_with("downloadmp3") {
            Mode::DownloadMP3
        } else if asdf.starts_with("search") {
            Mode::Search
        } else {
            Mode::End
        };
        if mode != Mode::End {
            let s = msg.clone().unwrap_or_default();
            let s2 = s.splitn(2, char::is_whitespace).last().unwrap();
            (mode, FixedString::from_str(s2).expect("wtf?"))
        } else {
            (
                Mode::End,
                FixedString::from_str(&msg.unwrap_or_default()).expect("wtf?"),
            )
        }
    } else {
        let mode = match opt_mode
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default()
            .as_str()
        {
            "next" => Mode::Next,
            "all" => Mode::All,
            "reverse" => Mode::Reverse,
            "shuffle" => Mode::Shuffle,
            "jump" => Mode::Jump,
            "downloadmkv" => Mode::DownloadMKV,
            "downloadmp3" => Mode::DownloadMP3,
            "search" => Mode::Search,
            _ => Mode::End,
        };
        (
            mode,
            FixedString::from_str(&msg.unwrap_or_default()).expect("wtf?"),
        )
    }
}

/// Parses the msg variable from the parameters to the play command.
/// Due to the way that the way the poise library works with auto filling them
/// based on types, it could be kind of mangled if the prefix version of the
/// command is used.
// TODO: Old and crufty. Clean up.
#[tracing::instrument]
pub fn get_msg(
    mode: Option<String>,
    query_or_url: Option<String>,
    is_prefix: bool,
) -> Option<String> {
    let step1 = query_or_url.clone().map(|s| s.replace("query_or_url:", ""));
    if is_prefix {
        match (mode
            .clone()
            .map(|s| s.replace("query_or_url:", ""))
            .unwrap_or_default()
            + " "
            + &step1.unwrap_or("".to_string()))
            .trim()
        {
            "" => None,
            x => Some(x.to_string()),
        }
    } else {
        step1
    }
}

#[cfg(test)]
mod test {
    use crack_types::to_fixed;

    use super::*;
    use crate::music::audit::{AuditEvent, BotReason, TrackRef};
    use crate::music::ops::test_support::*;
    use crate::utils::{get_requesting_user, get_track_handle_metadata};
    use crate::{Data, DataInner};

    /// Every queued track gets a second try at a refused stream URL.
    #[tokio::test]
    async fn a_queued_track_retries_a_refused_stream() {
        let (inner, calls) = crate::sources::retry::refuses_once();
        let track = new_track(SongbirdInput::Lazy(inner), None, None);
        let SongbirdInput::Lazy(mut source) = track.0.input else {
            panic!("still lazy");
        };
        assert!(source.create_async().await.is_ok());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    fn actor() -> Actor {
        Actor::bot(BotReason::Autopause)
    }

    fn next(rx: &mut tokio::sync::mpsc::Receiver<AuditEvent>) -> Action {
        rx.try_recv().expect("one record").action
    }

    fn t(i: usize) -> TrackRef {
        TrackRef {
            title: Some(format!("t{i}")),
            url: None,
        }
    }

    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn each_primitive_records_exactly_what_it_did() {
        let (data, call, ids, mut rx) = queue_of(5).await;
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        {
            let h = call.lock().await;
            move_track(&guard, &h, 3, 1);
            assert_eq!(
                next(&mut rx),
                Action::Move {
                    track: t(3),
                    from: 3,
                    to: 1
                }
            );
            remove_at(&guard, &h, 1);
            assert_eq!(
                next(&mut rx),
                Action::Remove {
                    track: t(3),
                    index: 1
                }
            );
            shuffle_behind_current(&guard, &h);
            assert_eq!(next(&mut rx), Action::Shuffle { count: 3 });
            pause_queue(&guard, &h).ok();
            assert_eq!(next(&mut rx), Action::Pause);
            resume_queue(&guard, &h).ok();
            assert_eq!(next(&mut rx), Action::Resume);
            clear_from(&guard, &h, 2);
            assert_eq!(next(&mut rx), Action::Clear { removed: 2 });
            stop_queue(&guard, &h);
            assert_eq!(next(&mut rx), Action::Stop { removed: 2 });
        }
        let _ = ids;
        assert!(rx.try_recv().is_err(), "nothing extra");
    }

    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn draining_after_the_current_track_records_a_clear() {
        let (data, call, _ids, mut rx) = queue_of(4).await;
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let h = call.lock().await;
        drain_after_current(&guard, &h, 2);
        assert_eq!(next(&mut rx), Action::Clear { removed: 2 });
    }

    #[tokio::test]
    async fn a_dashboard_move_records_upcoming_positions() {
        let (data, call, ids, mut rx) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[3], 0).await, Ok(0));
        assert_eq!(
            next(&mut rx),
            Action::Move {
                track: t(3),
                from: 2,
                to: 0
            }
        );
    }

    #[tokio::test]
    async fn a_batch_add_is_one_record_listing_every_track() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let data = Data(Arc::new(DataInner {
            audit_tx: Some(tx),
            ..Default::default()
        }));
        let call = offline_call();
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let tracks = (0..3)
            .map(|i| {
                ResolvedTrack::new(QueryType::VideoLink(
                    "http://127.0.0.1:9/unreachable".to_owned(),
                ))
                .with_metadata(titled(&format!("t{i}")))
            })
            .collect::<Vec<_>>();
        let inserted = enqueue_resolved_tracks_back(&guard, &call, tracks, reqwest::Client::new())
            .await
            .expect("queued");
        assert_eq!(inserted.handles.len(), 3);
        assert_eq!(
            next(&mut rx),
            Action::Add {
                tracks: vec![t(0), t(1), t(2)],
                at: AddAt::Back
            }
        );
        assert!(rx.try_recv().is_err(), "one record, not three");
    }

    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn no_ops_record_nothing() {
        let (data, call, _ids, mut rx) = queue_of(1).await;
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        {
            let h = call.lock().await;
            move_track(&guard, &h, 0, 0);
            clear_from(&guard, &h, 1);
            drain_after_current(&guard, &h, 3);
            shuffle_behind_current(&guard, &h);
            remove_at(&guard, &h, 7);
        }
        assert!(rx.try_recv().is_err());
        let empty = offline_call();
        let h = empty.lock().await;
        stop_queue(&guard, &h);
        force_skip_top_track(&guard, &h).await.unwrap();
        // songbird answers Ok to pause/resume on an empty queue (autopause
        // does this after every session); that is not a change.
        pause_queue(&guard, &h).unwrap();
        resume_queue(&guard, &h).unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn skipping_records_the_track_that_was_playing() {
        let (data, call, _ids, mut rx) = queue_of(2).await;
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let h = call.lock().await;
        force_skip_top_track(&guard, &h).await.unwrap();
        assert_eq!(next(&mut rx), Action::Skip { track: Some(t(0)) });
    }

    /// #480. Every caller checks that something is playing before calling
    /// this, but a track that ends on its own in between empties the queue
    /// first. That race leaves exactly this state -- an empty queue -- so it is
    /// reproduced here without any timing. It used to panic a tokio worker on
    /// `current().unwrap()`, which the user never sees and the log records only
    /// as a backtrace.
    #[tokio::test]
    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn skipping_an_already_empty_queue_is_a_no_op_not_a_panic() {
        let data = Data(Arc::new(DataInner::default()));
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .expect("an uncontended guild grants the lease");
        let call = offline_call();
        let handler = call.lock().await;

        let remaining = force_skip_top_track(&guard, &handler)
            .await
            .expect("an empty queue is already skipped");

        assert!(remaining.is_empty());
    }

    async fn order(call: &Arc<Mutex<Call>>) -> Vec<uuid::Uuid> {
        call.lock()
            .await
            .queue()
            .current_queue()
            .iter()
            .map(|h| h.uuid())
            .collect()
    }

    #[expect(clippy::disallowed_methods, reason = "test fixture")]
    async fn move_in(
        data: &Data,
        call: &Arc<Mutex<Call>>,
        id: uuid::Uuid,
        to: usize,
    ) -> Result<usize, crate::music::remote::MoveRefused> {
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let handler = call.lock().await;
        move_track_by_id(&guard, &handler, id, to)
    }

    #[tokio::test]
    async fn a_track_moves_by_id_to_an_upcoming_position() {
        let (data, call, ids, _rx) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[3], 0).await, Ok(0));
        assert_eq!(order(&call).await, vec![ids[0], ids[3], ids[1], ids[2]]);
    }

    #[tokio::test]
    async fn a_target_past_the_end_lands_last() {
        let (data, call, ids, _rx) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[1], 99).await, Ok(2));
        assert_eq!(order(&call).await, vec![ids[0], ids[2], ids[3], ids[1]]);
    }

    #[tokio::test]
    async fn a_target_of_usize_max_lands_last() {
        let (data, call, ids, _rx) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[1], usize::MAX).await, Ok(2));
        assert_eq!(order(&call).await, vec![ids[0], ids[2], ids[3], ids[1]]);
    }

    #[tokio::test]
    async fn the_playing_track_does_not_move() {
        use crate::music::remote::MoveRefused;
        let (data, call, ids, _rx) = queue_of(3).await;
        assert_eq!(
            move_in(&data, &call, ids[0], 1).await,
            Err(MoveRefused::NowPlaying)
        );
        assert_eq!(order(&call).await, ids);
    }

    #[tokio::test]
    async fn an_id_no_longer_queued_moves_nothing() {
        use crate::music::remote::MoveRefused;
        let (data, call, ids, _rx) = queue_of(3).await;
        let gone = uuid::Uuid::from_u128(42);
        assert_eq!(
            move_in(&data, &call, gone, 0).await,
            Err(MoveRefused::Absent)
        );
        assert_eq!(order(&call).await, ids);
    }

    #[tokio::test]
    async fn a_summary_carries_the_id_title_and_who_asked() {
        use crate::music::remote::{summarize, Requester};
        let data = Data(Arc::new(DataInner::default()));
        let call = offline_call();
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let auto = enqueue_input_back(
            &guard,
            &call,
            songbird::input::File::new("/nonexistent/a.opus").into(),
            Some(titled("Auto Pick")),
            None,
        )
        .await;
        let asked = queue_track_ready_front(
            &guard,
            &call,
            TrackReadyData {
                source: songbird::input::File::new("/nonexistent/b.opus").into(),
                metadata: NewAuxMetadata(titled("Asked For")),
                user_id: Some(UserId::new(7)),
                username: None,
            },
        )
        .await
        .unwrap();
        drop(guard);

        let handles = call.lock().await.queue().current_queue();
        let s = summarize(&handles).await;

        let by_id = |id| s.iter().find(|t| t.id == id).expect("summarized");
        assert_eq!(by_id(auto.uuid()).title.as_deref(), Some("Auto Pick"));
        assert!(matches!(
            by_id(auto.uuid()).requester,
            Some(Requester::Auto)
        ));
        let asked = asked.last().unwrap();
        assert_eq!(by_id(asked.uuid()).title.as_deref(), Some("Asked For"));
        assert!(matches!(
            by_id(asked.uuid()).requester,
            Some(Requester::User(u)) if u == UserId::new(7)
        ));
    }

    #[tokio::test]
    async fn an_offline_call_with_tracks_is_idle_not_playing() {
        use crate::music::remote::{state_of_call, QueueState};
        let (_data, call, _ids, _rx) = queue_of(2).await;
        // No voice connection means no channel, and a dashboard cannot say
        // who may control a queue with no channel.
        assert!(matches!(state_of_call(&call).await, QueueState::Idle));
    }

    /// `/queue` and `/nowplaying` read a queued track through
    /// `TrackHandle::data::<TrackData>()`, which panics on a track built
    /// without it -- as `/play mode:next` built its track until v0.12.1.
    #[tokio::test]
    async fn a_track_queued_next_carries_what_queue_and_nowplaying_read() {
        let data = Data(Arc::new(DataInner::default()));
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let call = offline_call();
        let ready = TrackReadyData {
            source: songbird::input::File::new("/nonexistent/next.opus").into(),
            metadata: NewAuxMetadata(titled("Hit That")),
            user_id: Some(UserId::new(7)),
            username: None,
        };

        let queue = queue_track_ready_front(&guard, &call, ready)
            .await
            .expect("queued");

        let track = queue.last().expect("the queued track");
        let meta = get_track_handle_metadata(track).await.expect("metadata");
        assert_eq!(meta.title.as_deref(), Some("Hit That"));
        assert_eq!(get_requesting_user(track).await.unwrap(), UserId::new(7));
    }

    /// Autoplay's own enqueue. A recommendation with metadata reads back the
    /// way `/queue` shows it; one without reads back as "no metadata", an error
    /// the embeds handle, rather than a panic.
    #[tokio::test]
    async fn an_autoplayed_track_carries_what_queue_and_nowplaying_read() {
        let data = Data(Arc::new(DataInner::default()));
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let call = offline_call();

        let titled_track = enqueue_input_back(
            &guard,
            &call,
            songbird::input::File::new("/nonexistent/a.opus").into(),
            Some(titled("Want You Bad")),
            None,
        )
        .await;
        let untitled_track = enqueue_input_back(
            &guard,
            &call,
            songbird::input::File::new("/nonexistent/b.opus").into(),
            None,
            None,
        )
        .await;

        let meta = get_track_handle_metadata(&titled_track)
            .await
            .expect("metadata");
        assert_eq!(meta.title.as_deref(), Some("Want You Bad"));
        assert_eq!(
            get_requesting_user(&titled_track).await.unwrap(),
            UserId::new(1)
        );
        assert!(matches!(
            get_track_handle_metadata(&untitled_track).await,
            Err(CrackedError::NoMetadata)
        ));
    }

    /// Every way this crate queues a track, read back by everything that reads
    /// one: the queue embed, the now-playing embed and the requester. A panic
    /// here is a dead `/queue` or `/nowplaying` -- or, from the track-end
    /// handler, a dead event task.
    #[tokio::test]
    async fn every_way_a_track_is_queued_reads_back_without_a_panic() {
        use crate::messaging::interface::{create_queue_embed, now_playing_card};

        let data = Data(Arc::new(DataInner::default()));
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, actor())
            .await
            .unwrap();
        let call = offline_call();
        let client = reqwest::Client::new();
        // Nothing listens on port 9, so a source that does get opened fails at
        // once instead of reaching the network.
        let resolved = || {
            ResolvedTrack::new(QueryType::VideoLink(
                "http://127.0.0.1:9/unreachable".to_owned(),
            ))
        };
        let file = || -> SongbirdInput { songbird::input::File::new("/nonexistent/t.opus").into() };
        let ready = || TrackReadyData {
            source: file(),
            metadata: NewAuxMetadata(AuxMetadata::default()),
            user_id: None,
            username: None,
        };

        let mut handles = Vec::new();
        // The untitled track first: the queue embed takes its thumbnail from
        // the first track, and that read has to survive no metadata too.
        for resolved in [
            resolved(),
            resolved()
                .with_metadata(titled("Hit That"))
                .with_user_id(UserId::new(9)),
        ] {
            let track = build_track(&resolved, &client).expect("built");
            handles.push(enqueue_track_back(&guard, &call, track, None).await);
        }
        let titled_track = &handles[1];
        let meta = get_track_handle_metadata(titled_track)
            .await
            .expect("metadata");
        assert_eq!(meta.title.as_deref(), Some("Hit That"));
        assert_eq!(
            get_requesting_user(titled_track).await.unwrap(),
            UserId::new(9)
        );
        handles.extend(
            queue_track_ready_front(&guard, &call, ready())
                .await
                .expect("queued next"),
        );
        handles.extend(
            _queue_track_ready_back(&guard, &call, ready())
                .await
                .expect("queued back"),
        );
        handles.push(
            enqueue_input_back(&guard, &call, file(), Some(titled("Want You Bad")), None).await,
        );
        handles.push(enqueue_input_back(&guard, &call, file(), None, None).await);

        let _ = create_queue_embed(&handles, 0).await;
        for track in &handles {
            // The embed reads the metadata and the requester first, then asks
            // the driver for the play position, which an offline driver never
            // answers. A panic in the readers still fails this test; the wait
            // after them is cut short and ignored.
            let _ = tokio::time::timeout(Duration::from_millis(200), now_playing_card(track)).await;
            get_requesting_user(track).await.expect("a requester");
        }
    }

    #[test]
    fn test_fisher_yates() {
        let mut values = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        fisher_yates(&mut values, &mut rand::rng());
        assert_ne!(values, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_get_mode() {
        let is_prefix = true;
        let x = to_fixed("asdf");
        let msg = Some(x.clone());
        let mode = Some(to_fixed(""));

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let x = to_fixed("");
        let is_prefix = true;
        let msg = None;
        let mode = Some(x.clone());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let is_prefix = true;
        let msg = None;
        let mode = None;

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let is_prefix = false;
        let msg = Some(x.clone());
        let mode = Some(to_fixed("next"));

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::Next, x.clone()));

        let is_prefix = false;
        let msg = None;
        let mode = Some(to_fixed("downloadmkv"));

        assert_eq!(
            get_mode(is_prefix, msg, mode),
            (Mode::DownloadMKV, x.clone())
        );

        let is_prefix = false;
        let msg = None;
        let mode = Some(to_fixed("downloadmp3"));

        assert_eq!(
            get_mode(is_prefix, msg, mode),
            (Mode::DownloadMP3, x.clone())
        );

        let is_prefix = false;
        let msg = None;
        let mode = None;

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x));
    }

    #[test]
    fn test_get_msg() {
        let mode = Some("".to_string());
        let query_or_url = Some("".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = None;
        let query_or_url = Some("".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = None;
        let query_or_url = None;
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = Some("".to_string());
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("".to_string()));

        let mode = None;
        let query_or_url = Some("".to_string());
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("".to_string()));

        let mode = None;
        let query_or_url = None;
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = None;
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode = Some("".to_string());
        let query_or_url = None;
        let is_prefix = false;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, None);

        let mode: Option<String> = None;
        let query_or_url = Some("asdf asdf asdf asd f".to_string());
        let is_prefix = true;
        let res = get_msg(mode, query_or_url, is_prefix);
        assert_eq!(res, Some("asdf asdf asdf asd f".to_string()));
    }
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

    /// 🪤 Stops at the first `#[cfg(test)]`. This module names the very
    /// functions and senders it counts, so scanning the whole file would match
    /// its own text and pass no matter what the real code did.
    fn production_source() -> String {
        let src = std::fs::read_to_string("src/music/queue.rs").expect("readable");
        let end = src
            .find("#[cfg(test)]")
            .expect("this file has test modules");
        src[..end].to_string()
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
            Primitive("callers: queue_vec_query_type, queue_resolved_list_back"),
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
