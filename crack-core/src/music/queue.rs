use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    http_utils::CacheHttpExt,
    music::{NewQueryType, PlaybackOwner, QueueGuard},
    utils::{set_track_handle_metadata, set_track_handle_requesting_user, TrackData},
    Context as CrackContext, Error,
};
use crack_testing::ResolvedTrack;
use crack_types::{Mode, NewAuxMetadata, QueryType};
use rand::RngExt;
use serenity::{
    all::{CreateEmbed, EditMessage, Message, UserId},
    small_fixed_array::FixedString,
};
use songbird::{
    input::{Input as SongbirdInput, YoutubeDl},
    tracks::{Queued, Track, TrackHandle},
    Call,
};
use std::str::FromStr;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::{Mutex, RwLock};

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
    let _ = guard;
    // Through `build_track`, not a second copy of it. This function used to
    // construct its own `RustyYoutubeSearch` inline, which is why fixing the
    // source in `build_track` fixed `/gp` and left `/play` still silent.
    let track = build_track(&track_resolved, &http_client)?;
    let mut handler = call.lock().await;
    let _track_handle = handler.enqueue(track).await;
    // .enqueue_input(Into::<SongbirdInput>::into(track))
    let new_q = handler.queue().current_queue();
    drop(handler);
    // if let Some(metadata) = track_resolved.metadata {
    //     set_track_handle_metadata(&mut track_handle, metadata.clone()).await?;
    // }
    // set_track_handle_requesting_user(&mut track_handle, track_resolved.user_id).await?;

    Ok(new_q)
}

/// Takes a resolved track and queues it to the back of the queue.
/// Old version.
/// # Errors
/// Returns a [`CrackedError`] if the track cannot be queued.
#[allow(dead_code)]
pub async fn queue_resolved_track_back_old(
    call: &Arc<Mutex<Call>>,
    track: ResolvedTrack<'static>,
    http_client: reqwest::Client,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    let ytdl = YoutubeDl::new(http_client.clone(), track.get_url());

    let mut track_handle = handler
        .enqueue_input(Into::<SongbirdInput>::into(ytdl))
        .await;
    let new_q = handler.queue().current_queue();
    drop(handler);
    set_track_handle_metadata(&mut track_handle, track.metadata.unwrap()).await?;
    set_track_handle_requesting_user(&mut track_handle, track.user_id).await?;

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
) -> Result<Track, CrackedError> {
    // yt-dlp, not rusty_ytdl. rusty_ytdl resolves fine but the googlevideo URL
    // it hands back is rejected: `c=ANDROID` fetches 403, songbird gets an empty
    // stream, and symphonia reports it as "no suitable format reader found",
    // which reads like a codec problem and is not one. yt-dlp negotiates a
    // client whose URL actually serves (`c=VISIONOS` at the time of writing) and
    // keeps up with YouTube's changes, which is the whole reason it exists.
    //
    // Needs `yt-dlp` on PATH -- see the Dockerfile, and note it must be the musl
    // build on this Alpine base.
    let ytdl = YoutubeDl::new(http_client.clone(), resolved.get_url());
    let track_data = Arc::new(TrackData {
        user_id: Arc::new(RwLock::new(Some(resolved.user_id))),
        aux_metadata: Arc::new(RwLock::new(resolved.metadata.clone())),
    });
    Ok(Track::new_with_data(ytdl.into(), track_data))
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
    let _ = guard;
    let mut handler = call.lock().await;
    let mut handles = Vec::with_capacity(tracks.len());
    for resolved in &tracks {
        match build_track(resolved, &http_client) {
            Ok(track) => handles.push(handler.enqueue(track).await),
            Err(e) => tracing::warn!("Failed to enqueue {}: {e}", resolved.get_url()),
        }
    }
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

/// Pushes a track to the front of the queue, after readying it.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn queue_track_ready_front(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let _ = guard;
    let mut handler = call.lock().await;
    let mut track_handle = handler.enqueue_input(ready_track.source).await;
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

    drop(handler);
    set_track_handle_metadata(&mut track_handle, ready_track.metadata.into()).await?;
    set_track_handle_requesting_user(&mut track_handle, UserId::new(1)).await?;
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
    let _ = guard;
    let mut handler = call.lock().await;

    let TrackReadyData {
        source,
        metadata,
        user_id,
        ..
    } = ready_track;

    let track_data = TrackData::new()
        .with_user_id(user_id.unwrap())
        .with_metadata(metadata.into());
    let track = Track::new_with_data(source, track_data);

    let _track_handle = handler.enqueue(track).await;
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
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    // FIXME:
    //ctx.async_send_track_metadata_write_msg(&ready_track);
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let q = queue_track_ready_front(&guard, call, ready_track).await?;
    Ok(q)
}

use crack_types::TrackResolveError;
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

    let begin = std::time::Instant::now();
    let resolved = match ctx.data().ct_client.resolve_track(query_type.clone()).await {
        Ok(resolved) => resolved.with_user_id(user_id),
        Err(e1) => {
            match e1.into() {
                Some(_e) => {
                    let ready_track = ready_query(ctx, query_type.clone()).await?;
                    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
                    return _queue_track_ready_back(&guard, call, ready_track).await;
                },
                None => {
                    return Err(CrackedError::TrackResolveError(
                        TrackResolveError::UnknownQueryType,
                    ));
                },
            };
        },
    };
    let after_ready = std::time::Instant::now();
    // FIXME:
    //ctx.async_send_track_metadata_write_msg(&ready_track);
    let after_send = std::time::Instant::now();
    //let queue = queue_track_ready_back(call, ready_track).await;
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
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

/// Append a list of tracks to the end of the queue.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub async fn _append_queue(
    guard: &QueueGuard,
    call: Arc<Mutex<Call>>,
    mut tracks: VecDeque<Queued>,
) -> Result<Vec<TrackHandle>, Error> {
    let _ = guard;
    let handler = call.lock().await;
    handler.queue().modify_queue(|queue| {
        queue.append(&mut tracks);
    });
    Ok(handler.queue().current_queue())
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

/// Queue a list of keywords to be played from the end of the queue.
///
/// The first track is resolved and queued on its own so playback starts
/// immediately; the remainder is resolved concurrently in batches behind it.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_back(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    msg: &mut Message,
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
            if let Err(e) = msg
                .edit(
                    &ctx,
                    EditMessage::new().embed(CreateEmbed::default().description(description)),
                )
                .await
            {
                tracing::warn!("Failed to update queue progress message: {e}");
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
    msg: &mut Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let user_id = ctx.author().id;
    let client = http_utils::get_client_old().clone();

    let mut tracks = tracks
        .into_iter()
        .map(|t| t.with_user_id(user_id))
        .collect::<Vec<_>>();
    if tracks.is_empty() {
        return Err(CrackedError::Other("Playlist resolved to no playable tracks").into());
    }

    let rest = tracks.split_off(1);
    // Hold the guard across the enqueue AND the snapshot read right after it,
    // closing the window a concurrent command's insert could otherwise land
    // in -- the enqueue and the snapshot used to be two separate acquisitions
    // of just the call lock. Neither `build_track` nor the enqueue itself does
    // I/O (tracks here are already resolved), so this is never held across a
    // slow operation. See #333.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
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

    for chunk in rest.chunks(QUEUE_BATCH_SIZE) {
        // Same reasoning as the first batch above: guard held across the
        // enqueue and its snapshot, dropped before the (network) progress
        // message edit below. See #333.
        let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
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
            if let Err(e) = msg
                .edit(
                    &ctx,
                    EditMessage::new().embed(CreateEmbed::default().description(description)),
                )
                .await
            {
                tracing::warn!("Failed to update queue progress message: {e}");
            }
        }
    }
    Ok(())
}

/// Enqueue already-resolved tracks and hold `guard` across the snapshot read
/// right after, closing the window a concurrent command's insert could
/// otherwise land in -- those used to be two separate acquisitions of just the
/// call lock. See #333.
///
/// Deliberately takes already-resolved tracks rather than doing the
/// `resolve_track_many` itself: resolution is the slow leg, and a caller must
/// do it before acquiring `guard`, not while holding it. [`queue_vec_query_type`]
/// and [`queue_query_list_offset`]'s low-queue branch are the two callers, and
/// each resolves on its own schedule before reaching here.
#[cfg(not(tarpaulin_include))]
async fn enqueue_resolved_and_snapshot(
    guard: &QueueGuard,
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    resolved: Vec<ResolvedTrack<'static>>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    debug_assert_eq!(
        guard.guild_id(),
        guild_id,
        "QueueGuard is for another guild"
    );
    enqueue_resolved_tracks_back(guard, call, resolved, http_utils::get_client_old().clone())
        .await?;
    // update_queue_messages redraws the whole queue message, so it wants the
    // whole queue -- read deliberately here rather than received from the
    // enqueue, which now reports only what this call added. See #333.
    let snapshot = call.lock().await.queue().current_queue();
    update_queue_messages(&ctx, ctx.data(), &snapshot, guild_id).await;
    Ok(())
}

/// Queue a list of keywords to be played with an offset.
///
/// Resolves first, then acquires a [`QueueGuard`] and holds it across the
/// enqueue and its snapshot -- never across the resolve above, which is the
/// slow leg. See [`enqueue_resolved_and_snapshot`].
#[cfg(not(tarpaulin_include))]
pub async fn queue_vec_query_type(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    _mode: Mode,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
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

    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    enqueue_resolved_and_snapshot(&guard, ctx, &call, resolved).await
}

use crate::http_utils;
/// Queue a list of queries to be played with a given offset.
/// N.B. The offset must be 0 < offset < queue.len() + 1
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
///
/// 🪤 The `resolve_track_many` call below runs while `guard` is held, which
/// wraps a second `/play` in this guild around this call's whole 8-15s
/// resolution rather than just its enqueue. That is a known, deliberate gap:
/// fixing it means restructuring this function's check-then-act shape, which
/// is a separate, already-tracked piece of work. Do not fix it here.
#[cfg(not(tarpaulin_include))]
pub async fn queue_query_list_offset(
    guard: &QueueGuard,
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    offset: usize,
    _search_msg: &mut Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    debug_assert_eq!(
        guard.guild_id(),
        guild_id,
        "QueueGuard is for another guild"
    );

    // Can this starting section be simplified?
    let queue_size = {
        let handler = call.lock().await;
        handler.queue().len()
    };

    if queue_size <= 1 {
        // Reuses the guard already held here rather than going through
        // `queue_vec_query_type`, which acquires its own -- a second
        // `lock_queue` for this same guild from inside this call would
        // deadlock on the per-guild mutex `QueueGuard` wraps, which is not
        // re-entrant. This is the smallest change that avoids that; the
        // check-then-act restructuring is the separate work referenced above.
        let user_id = ctx.author().id;
        let resolved = ctx
            .data()
            .ct_client
            .resolve_track_many(queries)
            .await?
            .into_iter()
            .map(|t| t.with_user_id(user_id))
            .collect::<Vec<_>>();
        return enqueue_resolved_and_snapshot(guard, ctx, &call, resolved).await;
    }

    verify(
        offset > 0 && offset <= queue_size + 1,
        CrackedError::NotInRange("index", offset as isize, 1, queue_size as isize),
    )?;

    // Resolved concurrently; this was a serial round trip per track.
    let tracks = ctx.data().ct_client.resolve_track_many(queries).await?;
    // enqueue_resolved_tracks(ctx.get_call(), tracks).await?;
    // for query in queries {
    //     let ready_track = ready_query(ctx, query).await?;
    //     // FIXME:
    //     //ctx.async_send_track_metadata_write_msg(&ready_track);
    //     tracks.push(ready_track);
    // }

    // One lock for the whole insert, and a lazy `Compose` per track rather than
    // an eager `YoutubeDl` metadata fetch.
    let client = http_utils::get_client_old().clone();
    let cur_q = {
        let mut handler = call.lock().await;
        for (idx, resolved) in tracks.into_iter().enumerate() {
            let track = match build_track(&resolved, &client) {
                Ok(track) => track,
                Err(e) => {
                    tracing::warn!("Failed to build track {}: {e}", resolved.get_url());
                    continue;
                },
            };
            let _ = handler.enqueue(track).await;
            handler.queue().modify_queue(|q| {
                if let Some(back) = q.pop_back() {
                    q.insert((idx + offset).min(q.len()), back);
                }
            });
        }
        handler.queue().current_queue()
    };

    update_queue_messages(&ctx, ctx.data(), &cur_q, guild_id).await;

    Ok(())
}

/// Drop everything from `from` onward, stopping each track. Used by `/clear`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn clear_from(guard: &QueueGuard, handler: &Call, from: usize) {
    let _ = guard;
    handler.queue().modify_queue(|v| {
        v.drain(from..).for_each(|x| {
            let _ = x.stop();
            drop(x);
        });
    });
}

/// Drop `count` tracks after the currently-playing one. Used by `/skip`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn drain_after_current(guard: &QueueGuard, handler: &Call, count: usize) {
    let _ = guard;
    handler.queue().modify_queue(|v| {
        let end = (1 + count).min(v.len());
        v.drain(1..end);
    });
}

/// Reorder the queue behind the currently-playing track. Used by `/shuffle`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn shuffle_behind_current(guard: &QueueGuard, handler: &Call) {
    let _ = guard;
    handler.queue().modify_queue(|queue| {
        // skip the first track on queue because it's being played
        fisher_yates(queue.make_contiguous()[1..].as_mut(), &mut rand::rng())
    });
}

/// Move a track from one queue index to another. Used by `/movesong`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn move_track(guard: &QueueGuard, handler: &Call, at: usize, to: usize) {
    let _ = guard;
    handler.queue().modify_queue(|queue| {
        // The caller verifies both indices are in range before calling.
        let song = queue.remove(at).expect("index out of bounds");
        queue.insert(to, song);
    });
}

/// Remove one track by queue index. Used by `/remove`.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn remove_at(guard: &QueueGuard, handler: &Call, index: usize) {
    let _ = guard;
    handler.queue().modify_queue(|v| {
        if let Some(track) = v.remove(index) {
            let _ = track.stop();
        }
    });
}

// `stop_queue` and `pause_queue` (guard-taking wrappers around
// `call.lock().await.queue().stop()`/`.pause()`) are deliberately NOT defined
// here. Their only intended callers are gp.rs and track_end.rs -- Task 6's
// files, not this dispatch's -- so with no caller anywhere in this dispatch
// they trip `dead_code` (this module is `pub(crate)`, so `pub` alone does not
// exempt them), and "no new #[allow(dead_code)]" rules out silencing that.
// Add them in queue.rs, with the same doc comments Task 4 specified, at the
// point Task 6 gains a real call site for each -- that gives them a caller in
// the same commit that defines them, same as every other helper here.

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
mod insertion_tests {
    use super::*;

    #[test]
    fn inserted_describes_only_this_call() {
        // #333: the bug was returning the whole queue after enqueueing, so two
        // concurrent /play calls each reported both sets of songs. `Inserted`
        // cannot express that -- it carries only what this call added.
        let inserted = Inserted {
            handles: Vec::new(),
        };
        assert_eq!(inserted.count(), 0);
    }

    #[test]
    fn count_is_the_handles_this_call_added() {
        let inserted = Inserted {
            handles: Vec::new(),
        };
        assert_eq!(inserted.count(), inserted.handles.len());
    }
}
