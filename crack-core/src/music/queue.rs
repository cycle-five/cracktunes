use super::QueryType;
use crate::{
    commands::{Mode, MyAuxMetadata, RequestingUser},
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    http_utils::CacheHttpExt,
    poise_ext::ContextExt,
    Context as CrackContext, Error,
};
use serenity::all::{CreateEmbed, EditMessage, Message, UserId};
use songbird::{input::Input as SongbirdInput, tracks::TrackHandle, Call};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Data needed to queue a track.
pub struct TrackReadyData {
    pub source: SongbirdInput,
    pub metadata: MyAuxMetadata,
    pub user_id: Option<UserId>,
    pub username: Option<String>,
}

/// Takes a query and returns a track that is ready to be played, along with relevant metadata.
pub async fn ready_query(
    ctx: CrackContext<'_>,
    query_type: QueryType,
) -> Result<TrackReadyData, CrackedError> {
    let user_id = Some(ctx.author().id);
    let (source, metadata_vec): (SongbirdInput, Vec<MyAuxMetadata>) =
        query_type.get_track_source_and_metadata(None).await?;
    let metadata = match metadata_vec.first() {
        Some(x) => x.clone(),
        None => {
            return Err(CrackedError::Other("metadata.first() failed"));
        },
    };

    let username = user_id.map(|x| ctx.user_id_to_username_or_default(x));

    Ok(TrackReadyData {
        source,
        metadata,
        user_id,
        username,
    })
}

/// Pushes a track to the front of the queue, after readying it.
pub async fn queue_track_ready_front(
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    let track_handle = handler.enqueue_input(ready_track.source).await;
    let new_q = handler.queue().current_queue();
    // Zeroth index: Currently playing track
    // First index: Current next track
    // Second index onward: Tracks to be played, we get in here most likely,
    // but if we're in one of the first two we don't want to do anything.
    if new_q.len() >= 3 {
        //return Ok(new_q);
        handler.queue().modify_queue(|queue| {
            let back = queue.pop_back().unwrap();
            queue.insert(1, back);
        });
    }

    drop(handler);
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(ready_track.metadata.clone());
    map.insert::<RequestingUser>(RequestingUser::UserId(
        ready_track.user_id.unwrap_or(UserId::new(1)),
    ));
    drop(map);
    Ok(new_q)
}

/// Pushes a track to the back of the queue, after readying it.
pub async fn queue_track_ready_back(
    call: &Arc<Mutex<Call>>,
    ready_track: TrackReadyData,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let mut handler = call.lock().await;
    let track_handle = handler.enqueue_input(ready_track.source).await;
    let new_q = handler.queue().current_queue();
    drop(handler);
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(ready_track.metadata.clone());
    map.insert::<RequestingUser>(RequestingUser::from(ready_track.user_id));
    Ok(new_q)
}

/// Pushes a track to the front of the queue.
pub async fn queue_track_front(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    ctx.send_track_metadata_write_msg(&ready_track);
    let q = queue_track_ready_front(call, ready_track).await?;
    Ok(q)
}

/// Pushes a track to the front of the queue.
pub async fn queue_track_back(
    ctx: CrackContext<'_>,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    let begin = std::time::Instant::now();
    let ready_track = ready_query(ctx, query_type.clone()).await?;
    let after_ready = std::time::Instant::now();
    ctx.send_track_metadata_write_msg(&ready_track);
    let after_send = std::time::Instant::now();
    let queue = queue_track_ready_back(call, ready_track).await;
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

/// Queue a list of tracks to be played.
pub async fn queue_ready_track_list(
    call: Arc<Mutex<Call>>,
    _user_id: UserId,
    tracks: Vec<TrackReadyData>,
    mode: Mode,
) -> Result<Vec<TrackHandle>, Error> {
    let mut handler = call.lock().await;
    for (idx, ready_track) in tracks.into_iter().enumerate() {
        let TrackReadyData {
            source,
            metadata,
            user_id,
            ..
        } = ready_track;
        let track_handle = handler.enqueue_input(source).await;
        let mut map = track_handle.typemap().write().await;
        map.insert::<MyAuxMetadata>(metadata);
        map.insert::<RequestingUser>(RequestingUser::from(user_id));
        if mode == Mode::Next {
            handler.queue().modify_queue(|queue| {
                let back = queue.pop_back().unwrap();
                queue.insert(idx + 1, back);
            });
        }
    }
    Ok(handler.queue().current_queue())
}

/// Queue a list of keywords to be played from the end of the queue.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_back<'a>(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    msg: &'a mut Message,
) -> Result<(), Error> {
    let first = queries
        .first()
        .ok_or(CrackedError::Other("queries.first()"))?;
    queue_vec_query_type(ctx, call.clone(), vec![first.clone()], Mode::End).await?;
    let queries = queries[1..].to_vec();
    for chunk in queries.chunks(10) {
        let to_queue_str = chunk
            .iter()
            .map(|q| q.build_query_base().unwrap_or_default())
            .collect::<Vec<String>>()
            .join("\n");
        msg.edit(
            &ctx,
            EditMessage::new().embed(CreateEmbed::default().description(format!(
                "Queuing {} songs... \n{}",
                chunk.len(),
                to_queue_str
            ))),
        )
        .await?;
        queue_vec_query_type(ctx, call.clone(), chunk.to_vec(), Mode::End).await?
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

    let mut tracks = Vec::new();

    for query in queries {
        let ready_track = ready_query(ctx, query).await?;
        ctx.send_track_metadata_write_msg(&ready_track);
        tracks.push(ready_track);
    }
    let queue = queue_ready_track_list(call, ctx.author().id, tracks, Mode::End).await?;
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    Ok(())
}

/// Queue a list of queries to be played with a given offset.
/// N.B. The offset must be 0 < offset < queue.len() + 1
#[cfg(not(tarpaulin_include))]
pub async fn queue_query_list_offset<'a>(
    ctx: CrackContext<'_>,
    call: Arc<Mutex<Call>>,
    queries: Vec<QueryType>,
    offset: usize,
    _search_msg: &'a mut Message,
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

    let mut tracks = Vec::new();
    for query in queries {
        let ready_track = ready_query(ctx, query).await?;
        ctx.send_track_metadata_write_msg(&ready_track);
        tracks.push(ready_track);
    }

    let mut handler = call.lock().await;
    for (idx, ready_track) in tracks.into_iter().enumerate() {
        let input = ready_track.source;
        let metadata = ready_track.metadata;
        let user_id = ready_track.user_id;

        // let mut handler = call.lock().await;
        let track_handle = handler.enqueue_input(input).await;
        let mut map = track_handle.typemap().write().await;
        map.insert::<MyAuxMetadata>(metadata);
        map.insert::<RequestingUser>(RequestingUser::from(user_id));
        handler.queue().modify_queue(|q| {
            let back = q.pop_back().unwrap();
            q.insert(idx + offset, back);
        })
    }

    let cur_q = handler.queue().current_queue();
    drop(handler);
    update_queue_messages(&ctx, ctx.data(), &cur_q, guild_id).await;

    Ok(())
}

/// Get the play mode and the message from the parameters to the play command.
// TODO: There is a lot of cruft in this from the older version of this. Clean it up.
#[tracing::instrument]
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
