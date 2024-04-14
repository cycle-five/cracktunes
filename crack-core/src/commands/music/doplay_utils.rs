use super::{Mode, QueryType};
use crate::db::Metadata;
use crate::handlers::track_end::update_queue_messages;
use crate::{
    commands::{get_track_source_and_metadata, MyAuxMetadata, RequestingUser},
    db::{aux_metadata_to_db_structures, PlayLog, User},
};
use crate::{errors::CrackedError, Context, Error};

use serenity::all::{ChannelId, GuildId, Http, UserId};
use songbird::input::AuxMetadata;
use songbird::tracks::TrackHandle;
use songbird::Call;
use songbird::{input::Input as SongbirdInput, tracks::Track};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Queue a list of keywords to be played
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
) -> Result<(), Error> {
    queue_keyword_list_w_offset(ctx, call, keyword_list, 0).await
}

/// Queue a list of keywords to be played with an offset.
#[cfg(not(tarpaulin_include))]
pub async fn queue_keyword_list_w_offset(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
    offset: usize,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    for (idx, keywords) in keyword_list.into_iter().enumerate() {
        let queue = insert_track(ctx, &call, &QueryType::Keywords(keywords), idx + offset).await?;
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
    use crate::errors::verify;

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
    enqueue_track_pgwrite_asdf(
        ctx.data().database_pool.as_ref().unwrap(),
        ctx.guild_id().unwrap(),
        ctx.channel_id(),
        ctx.author().id,
        ctx.http(),
        call,
        query_type,
    )
    .await
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
    http: &Http,
    call: &Arc<Mutex<Call>>,
    query_type: &QueryType,
) -> Result<Vec<TrackHandle>, CrackedError> {
    tracing::info!("query_type: {:?}", query_type);
    // is this comment still relevant to this section of code?
    // safeguard against ytdl dying on a private/deleted video and killing the playlist
    let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(http, query_type.clone()).await;
    let res = metadata.first().unwrap().clone();
    let track: Track = source.into();

    // Get the username (string) of the user.
    let username = match http.get_user(user_id).await {
        Ok(x) => x.name,
        Err(e) => {
            tracing::error!("http.get_user error: {}", e);
            "Unknown".to_string()
        },
    };

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
            .unwrap_or("".to_string())
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
    //    use rspotify::model::{FullTrack, SimplifiedAlbum};

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
