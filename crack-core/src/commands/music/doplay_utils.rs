use super::QueryType;
use crate::handlers::track_end::update_queue_messages;
use crate::{
    commands::{get_track_source_and_metadata, MyAuxMetadata, RequestingUser},
    db::{aux_metadata_to_db_structures, PlayLog, User},
};
use crate::{errors::CrackedError, Context, Error};

use serenity::all::{ChannelId, GuildId, Http, UserId};
use songbird::tracks::TrackHandle;
use songbird::Call;
use songbird::{input::Input as SongbirdInput, tracks::Track};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Mutex;

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

#[cfg(not(tarpaulin_include))]
/// Queue a list of keywords to be played
pub async fn queue_keyword_list(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    keyword_list: Vec<String>,
) -> Result<(), Error> {
    queue_keyword_list_w_offset(ctx, call, keyword_list, 0).await
    // let pool = get_db_or_err!(ctx);
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let user_id = ctx.author().id;
    // for keywords in keyword_list.iter() {
    //     let queue = enqueue_track_pgwrite(
    //         // &pool,
    //         // guild_id,
    //         // ctx.channel_id(),
    //         // user_id,
    //         // ctx.http(),
    //         ctx,
    //         &call,
    //         &QueryType::Keywords(keywords.to_string()),
    //     )
    //     .await?;
    //     update_queue_messages(&Arc::new(ctx.http()), ctx.data(), &queue, guild_id).await;
    // }

    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let channel_id = ctx.channel_id();

    // let query_type = QueryType::KeywordList(keyword_list);

    // let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
    //     get_track_source_and_metadata(ctx.http(), query_type.clone()).await;

    // let db_pool = get_db_or_err!(ctx);

    // let metadata_vec: Vec<AuxMetadata> = Vec::new();
    // let playls = Playlist::create(db_pool, &name.clone(), ctx.author().id.get() as i64).await?;
    // let guild_id_i64 = guild_id.get() as i64;
    // let channel_id_i64 = channel_id.get() as i64;
    // for MyAuxMetadata::Data(m) in metadata {
    //     let res = aux_metadata_to_db_structures(&m, guild_id_i64, channel_id_i64);
    //     match res {
    //         Ok((in_metadata, _track)) => {
    //             let metadata = Metadata::get_or_create(db_pool, &in_metadata).await?;

    //             let _res = Playlist::add_track(
    //                 db_pool,
    //                 playls.id,
    //                 metadata.id,
    //                 guild_id_i64,
    //                 channel_id_i64,
    //             )
    //             .await?;
    //         }
    //         Err(e) => {
    //             tracing::error!("Error converting metadata to aux metadata: {}", e);
    //         }
    //     }
    // }
    // Ok((metadata_vec, source))
}

/// Inserts a track into the queue at the specified index.
#[cfg(not(tarpaulin_include))]
pub async fn insert_track(
    // pool: &PgPool,
    // guild_id: GuildId,
    // channel_id: ChannelId,
    // user_id: UserId,
    // http: &Http,
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

/// Enqueues a track and adds metadata to the database.
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

    let MyAuxMetadata::Data(res2) = res.clone();
    let returned_metadata = {
        let (metadata, _playlist_track) = match aux_metadata_to_db_structures(
            &res2,
            guild_id.get() as i64,
            channel_id.get() as i64,
        ) {
            Ok(x) => x,
            Err(e) => {
                tracing::error!("aux_metadata_to_db_structures error: {}", e);
                return Err(CrackedError::Other("aux_metadata_to_db_structures error"));
            }
        };
        let updated_metadata =
            match crate::db::metadata::Metadata::get_or_create(database_pool, &metadata).await {
                Ok(x) => x,
                Err(e) => {
                    tracing::error!("crate::db::metadata::Metadata::create error: {}", e);
                    metadata.clone()
                }
            };

        // Get the username (string) of the user.
        let username = match http.get_user(user_id).await {
            Ok(x) => x.name,
            Err(e) => {
                tracing::error!("http.get_user error: {}", e);
                "Unknown".to_string()
            }
        };

        match User::insert_or_update_user(database_pool, user_id.get() as i64, username).await {
            Ok(_) => {
                tracing::info!("Users::insert_or_update");
            }
            Err(e) => {
                tracing::error!("Users::insert_or_update error: {}", e);
            }
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
            }
            Err(e) => {
                tracing::error!("PlayLog::create error: {}", e);
            }
        };
        metadata
    };

    tracing::info!("returned_metadata: {:?}", returned_metadata);

    let mut handler = call.lock().await;
    let track_handle = handler.enqueue(track).await;
    let mut map = track_handle.typemap().write().await;
    map.insert::<MyAuxMetadata>(res.clone());
    map.insert::<RequestingUser>(RequestingUser::UserId(user_id));

    Ok(handler.queue().current_queue())
}
