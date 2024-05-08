use super::{Mode, QueryType};
use crate::db::Metadata;
use crate::errors::verify;
use crate::handlers::track_end::update_queue_messages;
use crate::http_utils;
use crate::{
    commands::{MyAuxMetadata, RequestingUser},
    db::{aux_metadata_to_db_structures, PlayLog, User},
};
use crate::{errors::CrackedError, Context, Error};

use rusty_ytdl::search::Playlist as YTPlaylist;
use serenity::all::{CacheHttp, ChannelId, CreateEmbed, EditMessage, GuildId, Message, UserId};
use songbird::input::AuxMetadata;
use songbird::tracks::TrackHandle;
use songbird::Call;
use songbird::{input::Input as SongbirdInput, tracks::Track};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

struct QueueTrackData {
    title: String,
    url: String,
}

/// Queue a list of tracks to be played.
#[cfg(not(tarpaulin_include))]
async fn queue_tracks(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    tracks: Vec<QueueTrackData>,
    search_msg: &mut Message,
    mode: Mode,
) -> Result<(), Error> {
    use crate::commands::youtube::queue_track_front;

    use super::youtube::queue_track_back;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let n = tracks.len() as f32;
    for (i, track) in (0_u32..).zip(tracks.into_iter()) {
        // Update the search message with what's queuing right now.
        let _ = search_msg
            .edit(
                ctx,
                EditMessage::new().embed(CreateEmbed::default().description(format!(
                    "Queuing: [{}]({})\n{}% Done...",
                    track.title,
                    track.url,
                    (i as f32 / n) as u32 * 100
                ))),
            )
            .await;
        let user_id = ctx.author().id;
        let new_query_type = QueryType::VideoLink(track.url.to_string());
        let _queue_res = if mode == Mode::Next {
            queue_track_front(ctx, &call, &new_query_type, user_id).await
        } else {
            queue_track_back(ctx, &call, &new_query_type, user_id).await
        };
        // let _queue_res =
        //    enqueue_track_pgwrite(ctx, &call, &QueryType::VideoLink(track.url.to_string())).await;
        // let queue = match queue_res {
        //     Ok(q) => q,
        //     Err(e) => {
        //         tracing::error!("Error: {}", e);
        //         continue;
        //     },
        // };
        // update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    }
    let queue = call.lock().await.queue().current_queue();
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    Ok(())
}

/// Queue a YouTube playlist to be played.
#[cfg(not(tarpaulin_include))]
pub async fn queue_yt_playlist<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    _guild_id: GuildId,
    playlist: YTPlaylist,
    search_msg: &'a mut Message,
) -> Result<(), Error> {
    let tracks = playlist.videos.iter().map(|x| QueueTrackData {
        title: x.title.clone(),
        url: x.url.clone(),
    });
    queue_tracks(ctx, call, tracks.collect(), search_msg, Mode::End).await
}

#[cfg(not(tarpaulin_include))]
pub async fn queue_yt_playlist_front<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    _guild_id: GuildId,
    playlist: YTPlaylist,
    search_msg: &'a mut Message,
) -> Result<(), Error> {
    // let tracks = playlist.videos.iter().map(|x| QueueTrackData {
    //     title: x.title.clone(),
    //     url: x.url.clone(),
    // });
    // queue_tracks(ctx, call, tracks.collect(), search_msg, Mode::Next).await
    queue_yt_playlist_internal(ctx, call, _guild_id, playlist, search_msg, Mode::Next)
        .await
        .map(|_| ())
}

/// Queue a YouTube playlist to be played.
#[cfg(not(tarpaulin_include))]
pub async fn queue_yt_playlist_internal<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    _guild_id: GuildId,
    playlist: YTPlaylist,
    search_msg: &'a mut Message,
    mode: Mode,
) -> Result<Vec<TrackHandle>, Error> {
    use super::youtube::ready_query;
    let user_id = ctx.author().id;
    search_msg
        .edit(
            ctx,
            EditMessage::new().embed(CreateEmbed::default().description(format!("Searching...",))),
        )
        .await?;

    let track_data = playlist.videos.iter().map(|x| QueueTrackData {
        title: x.title.clone(),
        url: x.url.clone(),
    });
    let mut tracks = Vec::new();
    if mode == Mode::Next {
        for track in track_data.rev().collect::<Vec<QueueTrackData>>() {
            let new_query = QueryType::VideoLink(track.url.to_string());
            let asdf = ready_query(ctx, new_query, user_id).await?;
            tracks.push(asdf);
        }
    } else {
        for track in track_data.collect::<Vec<QueueTrackData>>() {
            let new_query = QueryType::VideoLink(track.url.to_string());
            let asdf = ready_query(ctx, new_query, user_id).await?;
            tracks.push(asdf);
        }
    };

    search_msg
        .edit(
            ctx,
            EditMessage::new()
                .embed(CreateEmbed::default().description(format!("Search done, queuing...",))),
        )
        .await?;

    let mut handler = call.lock().await;
    for ready_track in tracks {
        let track_handle = handler.enqueue(ready_track.track).await;
        let mut map = track_handle.typemap().write().await;
        map.insert::<MyAuxMetadata>(ready_track.metadata.clone());
        map.insert::<RequestingUser>(RequestingUser::UserId(user_id));
        if mode == Mode::Next {
            handler.queue().modify_queue(|queue| {
                let back = queue.pop_back().unwrap();
                queue.push_front(back);
            });
        }
    }
    // let handler = call.lock().await;
    Ok(handler.queue().current_queue())
    //.rev()
    //.collect();
    // if mode == Mode::Shuffle {
    //     let mut tracks: Vec<QueueTrackData> = tracks.collect();
    //     tracks.shuffle(&mut rand::thread_rng());
    //     queue_tracks(ctx, call, tracks, search_msg).await
    // } else if mode == Mode::Reverse {
    //     let mut tracks: Vec<QueueTrackData> = tracks.collect();
    //     tracks.reverse();
    //     queue_tracks(ctx, call, tracks, search_msg).await
    // } else if mode == Mode::Next {
    // if mode == Mode::Next {
    //     let tracks2 = tracks.rev().collect();
    // }
    //queue_tracks_ready(ctx, call, tracks, search_msg, mode).await
}

/// Queue a list of keywords to be played at the front of the queue.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_front<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
    msg: &'a mut Message,
) -> Result<(), Error> {
    queue_keyword_list_w_offset(ctx, call, keyword_list, 1, msg).await
}

/// Queue a list of keywords to be played from the end of the queue.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_back<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
    msg: &'a mut Message,
) -> Result<(), Error> {
    queue_keyword_list_w_offset(ctx, call, keyword_list, 0, msg).await
}

/// Queue a list of keywords to be played with an offset.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_w_offset<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
    offset: usize,
    search_msg: &'a mut Message,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mut failed: usize = 0;
    let n = keyword_list.len() as f32;
    for (idx, keywords) in keyword_list.into_iter().enumerate() {
        search_msg
            .edit(
                ctx.http(),
                EditMessage::new().embed(CreateEmbed::default().description(format!(
                    "Queuing: {}\n{}% Done...",
                    keywords,
                    (idx as f32 / n) * 100.0
                ))),
            )
            .await?;
        let queue_res = if offset > 0 {
            let idx = idx + offset - failed;
            insert_track(ctx, &call, &QueryType::Keywords(keywords), idx).await
        } else {
            enqueue_track_pgwrite(ctx, &call, &QueryType::Keywords(keywords)).await
        };
        let queue = match queue_res {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("enqueue_track_pgwrite error: {}", e);
                failed += 1;
                Vec::new()
            },
        };
        if queue.is_empty() {
            continue;
        }
        // TODO: Perhaps pass this off to a background task?
        update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    }

    Ok(())
}

/// Inserts a track into the queue at the specified index.
#[cfg(not(tarpaulin_include))]
pub async fn insert_track(
    ctx: Context<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
    idx: usize,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let handler = call.lock().await;
    let queue_size = handler.queue().len();
    drop(handler);
    tracing::trace!("queue_size: {}, idx: {}", queue_size, idx);

    if queue_size <= 1 {
        let queue = enqueue_track_pgwrite(ctx, call, query_type).await?;
        return Ok(queue);
    }

    verify(
        idx > 0 && idx <= queue_size + 1,
        CrackedError::NotInRange("index", idx as isize, 1, queue_size as isize),
    )?;

    enqueue_track_pgwrite(ctx, call, query_type).await?;

    let handler = call.lock().await;
    handler.queue().modify_queue(|queue| {
        let back = queue.pop_back().unwrap();
        queue.insert(idx, back);
    });

    Ok(handler.queue().current_queue())
}

/// Enqueues a track and adds metadata to the database. (concise parameters)
#[cfg(not(tarpaulin_include))]
pub async fn enqueue_track_pgwrite(
    ctx: Context<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    // let database_pool = get_db_or_err!(ctx);
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let channel_id = ctx.channel_id();
    // let user_id = ctx.author().id;
    // let http = ctx.http();

    use super::youtube;
    youtube::queue_track_back(ctx, call, query_type, ctx.author().id).await
    // enqueue_track_pgwrite_asdf(
    //     ctx.data().database_pool.as_ref().unwrap(),
    //     ctx.guild_id().unwrap(),
    //     ctx.channel_id(),
    //     ctx.author().id,
    //     ctx,
    //     call,
    //     query_type,
    // )
    // .await
}

/// Writes metadata to the database for a playing track.
#[cfg(not(tarpaulin_include))]
pub async fn write_metadata_pg(
    database_pool: &PgPool,
    aux_metadata: AuxMetadata,
    user_id: UserId,
    username: String,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<Metadata, CrackedError> {
    let returned_metadata = {
        let (metadata, _playlist_track) = match aux_metadata_to_db_structures(
            &aux_metadata,
            guild_id.get() as i64,
            channel_id.get() as i64,
        ) {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("aux_metadata_to_db_structures error: {}", e);
                return Err(CrackedError::Other("aux_metadata_to_db_structures error"));
            },
        };
        let updated_metadata =
            match crate::db::metadata::Metadata::get_or_create(database_pool, &metadata).await {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!("crate::db::metadata::Metadata::create error: {}", e);
                    metadata.clone()
                },
            };

        match User::insert_or_update_user(database_pool, user_id.get() as i64, username).await {
            Ok(_) => {
                tracing::info!("Users::insert_or_update");
            },
            Err(e) => {
                tracing::error!("Users::insert_or_update error: {}", e);
            },
        };
        match PlayLog::create(
            database_pool,
            user_id.get() as i64,
            guild_id.get() as i64,
            updated_metadata.id as i64,
        )
        .await
        {
            Ok(x) => {
                tracing::info!("PlayLog::create: {:?}", x);
            },
            Err(e) => {
                tracing::error!("PlayLog::create error: {}", e);
            },
        };
        metadata
    };
    Ok(returned_metadata)
}

/// Enqueues a track and adds metadata to the database. (parameters broken out)
#[cfg(not(tarpaulin_include))]
pub async fn enqueue_track_pgwrite_asdf(
    database_pool: &PgPool,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    cache_http: impl CacheHttp,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    use crate::commands::youtube::get_track_source_and_metadata;

    tracing::info!("query_type: {:?}", query_type);
    // is this comment still relevant to this section of code?
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(query_type.clone()).await?;
    let res = match metadata.first() {
        Some(x) => x.clone(),
        None => {
            return Err(CrackedError::Other("metadata.first() failed"));
        },
    };
    let track: Track = source.into();

    // let username = http_utils::http_to_username_or_default(http, user_id).await;
    let username = http_utils::cache_to_username_or_default(cache_http, user_id);

    let MyAuxMetadata::Data(aux_metadata) = res.clone();

    let returned_metadata = write_metadata_pg(
        database_pool,
        aux_metadata,
        user_id,
        username,
        guild_id,
        channel_id,
    )
    .await?;

    tracing::info!("returned_metadata: {:?}", returned_metadata);

    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(res.clone());
    map.insert::<RequestingUser>(RequestingUser::UserId(user_id));

    Ok(handler.queue().current_queue())
}

/// Get the play mode and the message from the parameters to the play command.
pub fn get_mode(is_prefix: bool, msg: Option<String>, mode: Option<String>) -> (Mode, String) {
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
            (mode, s2.to_string())
        } else {
            (Mode::End, msg.unwrap_or_default())
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
        (mode, msg.unwrap_or_default())
    }
}

/// Parses the msg variable from the parameters to the play command.
/// Due to the way that the way the poise library works with auto filling them
/// based on types, it could be kind of mangled if the prefix version of the
/// command is used.
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

/// Rotates the queue by `n` tracks to the right.
#[cfg(not(tarpaulin_include))]
pub async fn rotate_tracks(
    call: &Arc<Mutex<Call>>,
    n: usize,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let handler = call.lock().await;

    verify(
        handler.queue().len() > 2,
        CrackedError::Other("cannot rotate queues smaller than 3 tracks"),
    )?;

    handler.queue().modify_queue(|queue| {
        let mut not_playing = queue.split_off(1);
        not_playing.rotate_right(n);
        queue.append(&mut not_playing);
    });

    Ok(handler.queue().current_queue())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_mode() {
        let is_prefix = true;
        let x = "asdf".to_string();
        let msg = Some(x.clone());
        let mode = Some("".to_string());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::End, x.clone()));

        let x = "".to_string();
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
        let mode = Some("next".to_string());

        assert_eq!(get_mode(is_prefix, msg, mode), (Mode::Next, x.clone()));

        let is_prefix = false;
        let msg = None;
        let mode = Some("downloadmkv".to_string());

        assert_eq!(
            get_mode(is_prefix, msg, mode),
            (Mode::DownloadMKV, x.clone())
        );

        let is_prefix = false;
        let msg = None;
        let mode = Some("downloadmp3".to_string());

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
