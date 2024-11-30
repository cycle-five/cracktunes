use crate::commands::{cmd_check_music, help};
use crate::music::query::query_type_from_url;
use crate::music::queue::{get_mode, get_msg, queue_track_back};
use crate::music::NewQueryType;
use crate::utils::edit_embed_response2;
use crate::{commands::get_call_or_join_author, http_utils::SendMessageParams};
use crate::{
    errors::{verify, CrackedError},
    handlers::track_end::update_queue_messages,
    messaging::interface::create_now_playing_embed,
    messaging::{
        message::CrackedMessage,
        messages::{
            PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, TRACK_DURATION, TRACK_TIME_TO_PLAY,
        },
    },
    poise_ext::ContextExt,
    sources::youtube::build_query_aux_metadata,
    utils::get_track_handle_metadata,
    Context, Data, Error,
};
use crate::{http_utils, CrackedResult};
use ::serenity::all::CreateAutocompleteResponse;
use ::serenity::{
    all::{CommandInteraction, Message},
    builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, EditMessage},
};
use crack_types::QueryType;
use crack_types::{
    get_human_readable_timestamp, search_result_to_aux_metadata, Mode, NewAuxMetadata,
};
use poise::{serenity_prelude as serenity, ReplyHandle};
use songbird::{tracks::TrackHandle, Call};
use std::borrow::Cow;
use std::{cmp::Ordering, sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// Get the guild name.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    let shard_id = ctx.serenity_context().shard_id;
    ctx.say(format!(
        "The name of this guild is: {}, shard_id: {}",
        ctx.partial_guild().await.unwrap().name,
        shard_id
    ))
    .await?;

    Ok(())
}

/// Play a song next
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("next", "pn", "Pn", "insert", "ins", "push"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn playnext(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, Some("next".to_string()), None, query_or_url).await
}

/// Search interactively for a song
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("s", "S"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn search(
    ctx: Context<'_>,
    #[rest]
    #[description = "search query."]
    query: String,
) -> Result<(), Error> {
    play_internal(ctx, Some("search".to_string()), None, Some(query)).await
}

use crack_testing::{suggestion2, ResolvedTrack};

/// Autocomplete to suggest a search query.
pub async fn autocomplete<'a>(
    _ctx: poise::ApplicationContext<'_, Data, Error>,
    searching: &'a str,
) -> CreateAutocompleteResponse<'a> {
    // let choices = match suggestion2(searching).await {
    //     Ok(x) => {
    //         let choices = x.iter().map(|choice| choice).collect();
    //         choices
    //     },
    //     Err(e) => {
    //         tracing::error!("Error getting suggestions: {:?}", e);
    //         vec![]
    //     },
    // };
    let choices = suggestion2(searching).await.unwrap_or_default();
    let res = CreateAutocompleteResponse::new();
    res.set_choices(Cow::Owned(choices.clone()))
}

/// Play a song.
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("p", "P"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn play(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    #[autocomplete = "autocomplete"]
    query: String,
) -> Result<(), Error> {
    // Split off the first part of the query for
    let query = query.split("~").next().unwrap_or_default().to_string();
    play_internal(ctx, None, None, Some(query)).await
}

/// Play a song with more options
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    aliases("opt"),
    check = "cmd_check_music"
)]
pub async fn optplay(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
    #[description = "Play mode"] mode: Option<String>,
    #[description = "File to play."] file: Option<serenity::Attachment>,
    #[description = "song link or search query."] query_or_url: Option<String>,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    play_internal(ctx, mode, file, query_or_url).await
}

/// Play a local file.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    category = "Music",
    check = "cmd_check_music"
)]
pub async fn playfile(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
    #[description = "File to play."] file: serenity::Attachment,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    play_internal(ctx, None, Some(file), None).await
}

use songbird::input::{Input as SongbirdInput, YoutubeDl};

/// Enqueue an extrernal queue of resolved tracks to the internal queue
/// for the bot in songbird.
pub async fn enqueue_resolved_tracks(
    call: Arc<Mutex<Call>>,
    tracks: Vec<ResolvedTrack<'_>>,
    mode: crack_types::Mode,
) -> Vec<TrackHandle> {
    let mut handler = call.lock().await;
    let http_client = http_utils::get_client_old();
    let mut out_tracks: Vec<TrackHandle> = Vec::new();
    match mode {
        crack_types::Mode::End => {
            for track in tracks.iter() {
                let ytdl = YoutubeDl::new(http_client.clone(), track.get_url());
                let res = handler
                    .enqueue_input(Into::<SongbirdInput>::into(ytdl))
                    .await;
                //handler.queue()
                out_tracks.push(res);
            }
        },
        //crack_types::Mode::Next => {},
        _ => unimplemented!(),
    }
    out_tracks
}

// /// Pushes a track to the front of the queue, after readying it.
// pub async fn queue_track_ready_front(
//     call: &Arc<Mutex<Call>>,
//     ready_track: TrackReadyData,
// ) -> Result<Vec<TrackHandle>, CrackedError> {
//     let mut handler = call.lock().await;
//     let track_handle = handler.enqueue_input(ready_track.source).await;
//     let new_q = handler.queue().current_queue();
//     // Zeroth index: Currently playing track
//     // First index: Current next track
//     // Second index onward: Tracks to be played, we get in here most likely,
//     // but if we're in one of the first two we don't want to do anything.
//     if new_q.len() >= 3 {
//         //return Ok(new_q);
//         handler.queue().modify_queue(|queue| {
//             let back = queue.pop_back().unwrap();
//             queue.insert(1, back);
//         });
//     }

//     drop(handler);
//     let mut map = track_handle.typemap().write().await;
//     map.insert::<NewAuxMetadata>(ready_track.metadata.clone());
//     map.insert::<RequestingUser>(RequestingUser::UserId(
//         ready_track.user_id.unwrap_or(UserId::new(1)),
//     ));
//     drop(map);
//     Ok(new_q)
// }

/// Play a youtube playlist.
#[cfg(not(tarpaulin_include))]
#[tracing::instrument(skip(ctx))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    category = "Music",
    check = "cmd_check_music"
)]
pub async fn playytplaylist(
    ctx: Context<'_>,
    #[rest]
    #[description = "Playlist URL."]
    query: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mut crack_client = ctx.data().ct_client.clone();
    // This retrieves the call that the bot is connected to or joins the author's channel.
    // We error hear if the bot can't join the channel, or if the author isn't in a channel,
    // or the bot is in another channel, etc. So this should happen first.
    let _call = get_call_or_join_author(ctx).await?;
    // This gets the metadata for all the tracks in the playlist.
    // At this point we should have enough information to determine if any of the tracks
    // aren't allowed or able to be played (possibly?) and display the who list of them.
    let _tracks = crack_client.resolve_playlist(&query).await?;
    let _ = crack_client.build_display(guild_id).await;
    let yt_playlist_str = crack_client.get_display(guild_id);
    tracing::warn!("yt_playlist_str: {}", yt_playlist_str);
    let _ = ctx
        .send_reply_embed(CrackedMessage::Other(yt_playlist_str))
        .await?;
    // This enqueues the tracks into the internal queue for the bot.
    //let _ = enqueue_resolved_tracks(call, tracks).await;
    let _ = ctx.send_reply_embed(CrackedMessage::PlaylistQueued).await?;
    Ok(())
}

use crate::commands::resume_internal;
use crate::messaging::interface as msg_int;
use crate::poise_ext::PoiseContextExt;
use crack_types::to_fixed;

/// Does the actual playing of the song, all the other commands use this.
//#[tracing::instrument(skip(ctx))]
#[cfg(not(tarpaulin_include))]
pub async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // FIXME: This should be generalized.
    // Get current time for timing purposes.

    let _start = std::time::Instant::now();

    let is_prefix = ctx.is_prefix();

    let msg = get_msg(mode.clone(), query_or_url, is_prefix).map(to_fixed);

    if msg.is_none() && file.is_none() {
        if ctx.is_paused().await.unwrap_or_default() {
            return resume_internal(ctx).await;
        }
        let msg_params = SendMessageParams::default()
            .with_channel(ctx.channel_id())
            .with_msg(CrackedMessage::CrackedError(CrackedError::NoQuery))
            .with_color(crate::serenity::Color::RED);

        ctx.send_message(msg_params).await?;
        return Ok(());
    }

    let _after_msg_parse = std::time::Instant::now();

    let mode = mode.map(to_fixed);
    let (mode, msg) = get_mode(is_prefix, msg.clone(), mode);

    let _after_get_mode = std::time::Instant::now();

    // TODO: Maybe put into it's own function?
    let url = match file.clone() {
        Some(file) => file.url.clone(),
        None => msg.clone(),
    };
    let url = url.as_str();

    tracing::warn!(target: "PLAY", "url: {}", url);

    let call = get_call_or_join_author(ctx).await?;

    let _after_call = std::time::Instant::now();

    let search_msg = msg_int::send_search_message(&ctx).await?;
    //tracing::debug!("search response msg: {:?}", search_msg.message());

    // determine whether this is a link or a query string
    let query_type = query_type_from_url(ctx, url, file).await?;

    // FIXME: Decide whether we're using this everywhere, or not.
    // Don't like the inconsistency.
    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    let _after_query_type = std::time::Instant::now();

    // FIXME: Super hacky, fix this shit.
    // This is actually where the track gets queued into the internal queue, it's the main work function.
    let _move_on = match_mode(
        ctx,
        call.clone(),
        mode,
        query_type.clone(),
        search_msg.clone(),
    )
    .await;

    let _after_move_on = std::time::Instant::now();

    // // FIXME: Yeah, this is terrible, fix this.
    // if !move_on {
    //     return Ok(());
    // }

    // refetch the queue after modification
    // FIXME: I'm beginning to think that this walking of the queue is what's causing the performance issues.
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();
    drop(handler);

    let _after_refetch_queue = std::time::Instant::now();

    // This makes sense, we're getting the final response to the user based on whether
    // the song / playlist was queued first, last, or is now playing.
    // Ah! Also, sometimes after a long queue process the now playing message says that it's already
    // X seconds into the song, so this is definitely after the section of the code that
    // takes a long time.
    let embed = match queue.len().cmp(&1) {
        Ordering::Greater => {
            let estimated_time = calculate_time_until_play(&queue, mode)
                .await
                .unwrap_or_default();
            let NewQueryType(query_type) = query_type;
            match (query_type, mode) {
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::Next,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::Next");
                    let track = queue.get(1).unwrap();
                    build_queued_embed(PLAY_TOP, track, estimated_time).await
                },
                (
                    QueryType::VideoLink(_) | QueryType::Keywords(_) | QueryType::NewYoutubeDl(_),
                    Mode::End,
                ) => {
                    tracing::error!("QueryType::VideoLink|Keywords|NewYoutubeDl, mode: Mode::End");
                    let track = queue.last().unwrap();
                    build_queued_embed(PLAY_QUEUE, track, estimated_time).await
                },
                (QueryType::PlaylistLink(_) | QueryType::KeywordList(_), y) => {
                    tracing::error!(
                        "QueryType::PlaylistLink|QueryType::KeywordList, mode: {:?}",
                        y
                    );
                    CreateEmbed::default()
                        .description(format!("{:?}", CrackedMessage::PlaylistQueued))
                },
                (QueryType::File(_x_), y) => {
                    tracing::error!("QueryType::File, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track.clone()).await
                },
                (QueryType::YoutubeSearch(_x), y) => {
                    tracing::error!("QueryType::YoutubeSearch, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track.clone()).await
                },
                (x, y) => {
                    tracing::error!("{:?} {:?} {:?}", x, y, mode);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track.clone()).await
                },
            }
        },
        Ordering::Equal => {
            tracing::warn!("Only one track in queue, just playing it.");
            let track = queue.first().unwrap();
            create_now_playing_embed(track.clone()).await
        },
        Ordering::Less => {
            tracing::warn!("No tracks in queue, this only happens when an interactive search is done with an empty queue.");
            CreateEmbed::default()
                .description("No tracks in queue!")
                .footer(CreateEmbedFooter::new("No tracks in queue!"))
        },
    };

    let _after_embed = std::time::Instant::now();

    let _msg = edit_embed_response2(ctx, embed, search_msg.clone()).await?;

    // [Manage Messages]: Permissions::MANAGE_MESSAGES
    // I think this does different things based on prefix or not?
    // if !is_prefix {
    //     match search_msg.delete(&ctx).await {
    //         Ok(_) => {},
    //         Err(e) => {
    //             tracing::error!("Error deleting search message: {:?}", e);
    //         },
    //     }
    // }

    let _after_edit_embed = std::time::Instant::now();

    tracing::warn!(
        r#"
        after_msg_parse: {:?}
        after_get_mode: {:?} (+{:?})
        after_call: {:?} (+{:?})
        after_query_type: {:?} (+{:?})
        after_move_on: {:?} (+{:?})
        after_refetch_queue: {:?} (+{:?})
        after_embed: {:?} (+{:?})
        after_edit_embed: {:?} (+{:?})"#,
        _after_msg_parse.duration_since(_start),
        _after_get_mode.duration_since(_start),
        _after_get_mode.duration_since(_after_msg_parse),
        _after_call.duration_since(_start),
        _after_call.duration_since(_after_get_mode),
        _after_query_type.duration_since(_start),
        _after_query_type.duration_since(_after_call),
        _after_move_on.duration_since(_start),
        _after_move_on.duration_since(_after_query_type),
        _after_refetch_queue.duration_since(_start),
        _after_refetch_queue.duration_since(_after_move_on),
        _after_embed.duration_since(_start),
        _after_embed.duration_since(_after_refetch_queue),
        _after_edit_embed.duration_since(_start),
        _after_edit_embed.duration_since(_after_embed),
    );
    Ok(())
}
pub enum MessageOrInteraction {
    Message(Message),
    Interaction(CommandInteraction),
}

pub async fn get_user_message_if_prefix(ctx: Context<'_>) -> MessageOrInteraction {
    match ctx {
        Context::Prefix(ctx) => MessageOrInteraction::Message(ctx.msg.clone()),
        Context::Application(ctx) => MessageOrInteraction::Interaction(ctx.interaction.clone()),
    }
}

/// This is what actually does the majority of the work of the function.
/// It finds the track that the user wants to play and then actually
/// does the process of queuing it. This needs to be optimized.
async fn match_mode(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: NewQueryType,
    //search_msg: &'a mut ReplyHandle<'a>,
    search_msg: ReplyHandle<'_>,
) -> CrackedResult<bool> {
    tracing::info!("mode: {:?}", mode);

    // let ctx = Arc::new(ctx.clone());
    match mode {
        Mode::Search => query_type
            .mode_search(ctx, call)
            .await
            .map(|x| !x.is_empty()),
        Mode::DownloadMKV => query_type.mode_download(ctx, false).await,
        Mode::DownloadMP3 => query_type.mode_download(ctx, true).await,
        Mode::End => query_type.mode_end(ctx, call, search_msg.clone()).await,
        Mode::Next => query_type.mode_next(ctx, call, search_msg.clone()).await,
        Mode::Jump => query_type.mode_jump(ctx, call).await,
        Mode::All | Mode::Reverse | Mode::Shuffle => {
            query_type.mode_rest(ctx, call, search_msg).await
        },
    }
}

// /// new match_mode function.
// async fn match_mode_new<'ctx>(
//     ctx: Context<'_>,
//     call: Arc<Mutex<Call>>,
//     mode: Mode,
//     query_type: QueryType,
//     search_msg: Message,
// ) -> JoinHandle<dyn std::future::Future<Output = CrackedResult<bool>> + Send> {
//     tracing::info!("mode: {:?}", mode);

//     tokio::task::spawn(async move {
//         //let ctx = *ctx.as_ref().to_owned();
//         match mode {
//             _ => query_type.mode_download(ctx, false),
//         }
//     })
//     // let handle = tokio::spawn(async move {
//     //     let ctx2 = ctx.as_ref();
//     //     match mode {
//     //         _ => {
//     //             // query_type.mode_end(ctx, call, search_msg)
//     //             let beg_time = std::time::Instant::now();
//     //             let _ready_q = match query_type.get_track_source_and_metadata(None).await {
//     //                 Ok(x) => x,
//     //                 Err(e) => {
//     //                     return Err(e);
//     //                 },
//     //             };
//     //             let end_time = std::time::Instant::now();
//     //             tracing::info!(
//     //                 "get_track_source_and_metadata: {:?}",
//     //                 end_time.duration_since(beg_time)
//     //             );
//     //             let res = match query_type.mode_end(ctx, call, search_msg.clone()).await {
//     //                 Ok(x) if x => (),
//     //                 Ok(_) => {
//     //                     return Err(CrackedError::Other("No tracks in queue!"));
//     //                 },
//     //                 Err(e) => {
//     //                     return Err(e);
//     //                 },
//     //             };
//     //             Ok(())
//     //         },
//     //         // Mode::Search => query_type.mode_search(ctx1, call).await,
//     //         //    .map(|x| !x.is_empty()),
//     //         // Mode::DownloadMKV => query_type.mode_download(ctx, false).await,
//     //         // Mode::DownloadMP3 => query_type.mode_download(ctx, true).await,
//     //         // Mode::Next => query_type.mode_next(ctx, call, search_msg).await,
//     //         // Mode::Jump => query_type.mode_jump(ctx, call).await,
//     //         // Mode::All | Mode::Reverse | Mode::Shuffle => {
//     //         //     query_type.mode_rest(ctx, call, search_msg).await
//     //         // },
//     //         // _ => unimplemented!(),
//     //     }
//     //});

//     //handle
// }

// async fn query_type_to_metadata<'a>(
//     ctx: Context<'_>,
//     call: Arc<Mutex<Call>>,
//     mode: Mode,
//     query_type: QueryType,
//     search_msg: &'a mut Message,
// ) -> CrackedResult<bool> {
//     tracing::info!("mode: {:?}", mode);
// }

/// Calculate the time until the next track plays.
async fn calculate_time_until_play(queue: &[TrackHandle], mode: Mode) -> Option<Duration> {
    if queue.is_empty() {
        return None;
    }

    let zero_duration = Duration::ZERO;
    let top_track = queue.first()?;
    let top_track_elapsed = top_track
        .get_info()
        .await
        .map(|i| i.position)
        .unwrap_or(zero_duration);
    let metadata = get_track_handle_metadata(top_track).await.ok()?;

    let top_track_duration = match metadata.duration {
        Some(duration) => duration,
        None => return Some(Duration::MAX),
    };

    match mode {
        Mode::Next => Some(top_track_duration - top_track_elapsed),
        _ => {
            let center = &queue[1..queue.len() - 1];
            let livestreams =
                center.len() - center.iter().filter_map(|_t| metadata.duration).count();

            // if any of the tracks before are livestreams, the new track will never play
            if livestreams > 0 {
                return Some(Duration::MAX);
            }

            let durations = center
                .iter()
                .fold(Duration::ZERO, |acc, _x| acc + metadata.duration.unwrap());

            Some(durations + top_track_duration - top_track_elapsed)
        },
    }
}

/// Build an embed for the cure
async fn build_queued_embed<'att>(
    author_title: &'att str,
    track: &'att TrackHandle,
    estimated_time: Duration,
) -> CreateEmbed<'att> {
    let metadata = {
        // let map = track.typemap().read().await;
        // let my_metadata = map.get::<NewAuxMetadata>().unwrap();

        // match my_metadata {
        //     NewAuxMetadata(metadata) => metadata.clone(),
        // }
        get_track_handle_metadata(track).await.unwrap_or_default()
    };
    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();
    let meta_title = metadata.title.clone().unwrap_or(QUEUE_NO_TITLE.to_string());
    let source_url = metadata
        .source_url
        .clone()
        .unwrap_or(QUEUE_NO_SRC.to_string());

    // let title_text = &format!("[**{}**]({})", meta_title, source_url);

    let footer_text = format!(
        "{} {}\n{} {}",
        TRACK_DURATION,
        get_human_readable_timestamp(metadata.duration),
        TRACK_TIME_TO_PLAY,
        get_human_readable_timestamp(Some(estimated_time))
    );

    let author = CreateEmbedAuthor::new(author_title);

    CreateEmbed::new()
        .author(author)
        .title(meta_title)
        .url(source_url)
        .thumbnail(thumbnail)
        .footer(CreateEmbedFooter::new(Cow::Owned(footer_text)))
}

use crate::sources::rusty_ytdl::RequestOptionsBuilder;
use rusty_ytdl::search::YouTube;
/// Add tracks to the queue from aux_metadata.
#[cfg(not(tarpaulin_include))]
pub async fn queue_aux_metadata(
    ctx: Context<'_>,
    aux_metadata: &[NewAuxMetadata],
    mut msg: Message,
) -> CrackedResult<()> {
    // use crate::http_utils;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let search_results = aux_metadata;

    let client = &ctx.data().http_client;
    let manager = ctx.data().songbird.clone();

    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    let req = RequestOptionsBuilder::new()
        .set_client(client.clone())
        .build();
    let rusty_ytdl = YouTube::new_with_options(&req)?;
    for metadata in search_results {
        let source_url = metadata.metadata().source_url.as_ref();
        let metadata_final = if source_url.is_none() || source_url.unwrap().is_empty() {
            let search_query = build_query_aux_metadata(metadata.metadata());
            let _ = msg
                .edit(
                    &ctx,
                    EditMessage::default().content(format!("Queuing... {}", search_query)),
                )
                .await;

            let res = rusty_ytdl.search_one(search_query, None).await?;
            let res = res.ok_or(CrackedError::Other("No results found"))?;
            let new_aux_metadata = search_result_to_aux_metadata(&res);

            NewAuxMetadata(new_aux_metadata)
        } else {
            metadata.clone()
        };

        let query_type = QueryType::VideoLink(
            metadata_final
                .metadata()
                .source_url
                .as_ref()
                .cloned()
                .expect("source_url does not exist"),
        );
        let _ = queue_track_back(ctx, &call, &query_type).await?;
    }

    let queue = call.lock().await.queue().current_queue();
    update_queue_messages(&ctx, ctx.data(), &queue, guild_id).await;
    Ok(())
}
