use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    http_utils::CacheHttpExt,
    music::NewQueryType,
    sources::rusty_ytdl::RustyYoutubeSearch,
    utils::{set_track_handle_metadata, set_track_handle_requesting_user, TrackData},
    Context as CrackContext, Error,
};
use crack_testing::ResolvedTrack;
use crack_types::{Mode, NewAuxMetadata, QueryType};
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
/// # Errors
/// Returns a [`CrackedError`] if the track cannot be queued.
/// Can fail during the search itself, or when adding the metadata to the track,
/// or when adding the track to the internal queue.
pub async fn queue_resolved_track_back(
    call: &Arc<Mutex<Call>>,
    track_resolved: ResolvedTrack<'static>,
    http_client: reqwest::Client,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    //let ytdl = YoutubeDl::new(http_client.clone(), track.get_url());
    let query = QueryType::VideoLink(track_resolved.get_url());
    let track2 = track_resolved.clone();
    let ytdl = RustyYoutubeSearch::new_with_stuff(
        http_client.clone(),
        query,
        track2.metadata,
        track2.video,
    )?;
    let resolved_clone = &track_resolved.clone();
    let track_data = Arc::new(TrackData {
        user_id: Arc::new(RwLock::new(Some(resolved_clone.clone().user_id))),
        aux_metadata: Arc::new(RwLock::new(resolved_clone.metadata.clone())),
    });
    let track = Track::new_with_data(ytdl.clone().into(), track_data);
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
/// This performs no I/O: [`RustyYoutubeSearch`] is a lazy [`Compose`], so the
/// actual stream (and any metadata we don't already hold) is fetched when the
/// track reaches the front of the queue rather than when it is enqueued.
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

/// Queue a batch of resolved tracks to the back of the queue.
///
/// Takes the call lock once for the whole batch instead of once per track,
/// which matters when a playlist adds tens of tracks at a time.
pub async fn enqueue_resolved_tracks_back(
    call: &Arc<Mutex<Call>>,
    tracks: Vec<ResolvedTrack<'static>>,
    http_client: reqwest::Client,
) -> Result<Vec<TrackHandle>, CrackedError> {
    if tracks.is_empty() {
        let handler = call.lock().await;
        return Ok(handler.queue().current_queue());
    }

    let mut handler = call.lock().await;
    for resolved in &tracks {
        match build_track(resolved, &http_client) {
            Ok(track) => {
                let _ = handler.enqueue(track).await;
            },
            Err(e) => {
                tracing::warn!("Failed to enqueue {}: {e}", resolved.get_url());
            },
        }
    }
    Ok(handler.queue().current_queue())
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
pub async fn queue_track_ready_front(
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
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
pub async fn _queue_track_ready_back(
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
pub async fn queue_track_front(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    // FIXME:
    //ctx.async_send_track_metadata_write_msg(&ready_track);
    let q = queue_track_ready_front(call, ready_track).await?;
    Ok(q)
}

use crack_types::TrackResolveError;
/// Pushes a track to the front of the queue.
#[tracing::instrument(skip(ctx, call))]
pub async fn queue_track_back(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let user_id = ctx.author().id;

    let begin = std::time::Instant::now();
    let resolved = match ctx.data().ct_client.resolve_track(query_type.clone()).await {
        Ok(resolved) => resolved.with_user_id(user_id),
        Err(e1) => {
            match e1.into() {
                Some(_e) => {
                    let ready_track = ready_query(ctx, query_type.clone()).await?;
                    return _queue_track_ready_back(call, ready_track).await;
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
    let queue =
        queue_resolved_track_back(call, resolved, http_utils::get_client_old().clone()).await;
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
pub async fn _append_queue(
    call: Arc<Mutex<Call>>,
    mut tracks: VecDeque<Queued>,
) -> Result<Vec<TrackHandle>, Error> {
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
    let queue = enqueue_resolved_tracks_back(&call, tracks, client.clone()).await?;
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;

    if rest.is_empty() {
        return Ok(());
    }

    let total = rest.len();
    let mut queued = 0usize;
    let mut last_edit = std::time::Instant::now();

    for chunk in rest.chunks(QUEUE_BATCH_SIZE) {
        let queue = enqueue_resolved_tracks_back(&call, chunk.to_vec(), client.clone()).await?;
        queued += chunk.len();
        update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;

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

/// Queue a list of keywords to be played with an offset.
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

    let queue =
        enqueue_resolved_tracks_back(&call, resolved, http_utils::get_client_old().clone()).await?;
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    Ok(())
}

use crate::http_utils;
/// Queue a list of queries to be played with a given offset.
/// N.B. The offset must be 0 < offset < queue.len() + 1
#[cfg(not(tarpaulin_include))]
pub async fn queue_query_list_offset(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    offset: usize,
    _search_msg: &mut Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    // Can this starting section be simplified?
    let queue_size = {
        let handler = call.lock().await;
        handler.queue().len()
    };

    if queue_size <= 1 {
        return queue_vec_query_type(ctx, call, queries, Mode::End).await;
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
