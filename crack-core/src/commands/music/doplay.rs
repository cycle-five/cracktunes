use ::serenity::all::CommandInteraction;
use colored;
use rusty_ytdl::search::Playlist;
use rusty_ytdl::search::SearchOptions;
use rusty_ytdl::search::SearchType;

use super::doplay_utils::enqueue_track_pgwrite;
use super::doplay_utils::insert_track;
use super::doplay_utils::queue_keyword_list_back;

use crate::commands::doplay_utils::queue_yt_playlist;
use crate::commands::doplay_utils::queue_yt_playlist_front;
use crate::commands::doplay_utils::rotate_tracks;
use crate::commands::doplay_utils::{get_mode, get_msg, queue_keyword_list_w_offset};
use crate::commands::get_call_with_fail_msg;
use crate::commands::youtube::search_query_to_source_and_metadata;
use crate::commands::youtube::search_query_to_source_and_metadata_ytdl;
use crate::commands::youtube::video_info_to_source_and_metadata;
use crate::sources::rusty_ytdl::RustyYoutubeClient;
use crate::utils::send_search_response;
use crate::utils::yt_search_select;
use crate::{
    commands::skip::force_skip_top_track,
    errors::{verify, CrackedError},
    guild::settings::GuildSettings,
    handlers::track_end::update_queue_messages,
    http_utils,
    interface::create_now_playing_embed,
    messaging::{
        message::CrackedMessage,
        messages::{
            PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, SPOTIFY_AUTH_FAILED,
            TRACK_DURATION, TRACK_TIME_TO_PLAY,
        },
    },
    sources::spotify::{Spotify, SpotifyTrack, SPOTIFY},
    utils::{
        compare_domains, edit_response_poise, get_guild_name, get_human_readable_timestamp,
        get_interaction, get_track_metadata, send_embed_response_poise, send_response_poise_text,
    },
    Context, Error,
};
use ::serenity::{
    all::{Message, UserId},
    builder::{
        CreateAttachment, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage,
        EditInteractionResponse, EditMessage,
    },
};
use poise::serenity_prelude::{self as serenity, Attachment};
use reqwest::Client;
use songbird::input::HttpRequest;
use songbird::input::Input as SongbirdInput;
use songbird::{
    input::{AuxMetadata, Compose, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{
    cmp::Ordering,
    path::Path,
    process::{Output, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::process::Command;
use tokio::sync::Mutex;
use typemap_rev::TypeMapKey;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
    DownloadMKV,
    DownloadMP3,
    Search,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub query_type: QueryType,
    pub metadata: Option<AuxMetadata>,
}

#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(serenity::Attachment),
    NewYoutubeDl((YoutubeDl, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

impl QueryType {
    pub fn build_query(&self) -> Option<String> {
        match self {
            QueryType::Keywords(keywords) => Some(keywords.clone()),
            QueryType::KeywordList(keywords_list) => Some(keywords_list.join(" ")),
            QueryType::VideoLink(url) => Some(url.clone()),
            QueryType::SpotifyTracks(tracks) => Some(
                tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>()
                    .join(" "),
            ),
            QueryType::PlaylistLink(url) => Some(url.clone()),
            QueryType::File(file) => Some(file.url.clone()),
            QueryType::NewYoutubeDl((_src, metadata)) => metadata.source_url.clone(),
            QueryType::YoutubeSearch(query) => Some(query.clone()),
            QueryType::None => None,
        }
    }

    // FIXME: Do you want to have a reqwest client we keep around and pass into
    // this instead of creating a new one every time?
    pub async fn get_track_source_and_metadata(
        &self,
    ) -> Result<(SongbirdInput, Vec<MyAuxMetadata>), CrackedError> {
        use colored::Colorize;
        let client = http_utils::get_client().clone();
        tracing::warn!("{}", format!("query_type: {:?}", self).red());
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let mut ytdl = YoutubeDl::new_search(client, query.clone());
                let mut res = Vec::new();
                let asdf = ytdl.search(None).await?;
                for metadata in asdf {
                    let my_metadata = MyAuxMetadata::Data(metadata);
                    res.push(my_metadata);
                }
                Ok((ytdl.into(), res))
            },
            QueryType::VideoLink(query) => {
                tracing::warn!("In VideoLink");
                video_info_to_source_and_metadata(client.clone(), query.clone()).await
                // let mut ytdl = YoutubeDl::new(client, query);
                // tracing::warn!("ytdl: {:?}", ytdl);
                // let metadata = ytdl.aux_metadata().await?;
                // let my_metadata = MyAuxMetadata::Data(metadata);
                // Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::Keywords(query) => {
                tracing::warn!("In Keywords");
                let res = search_query_to_source_and_metadata(client.clone(), query.clone()).await;
                match res {
                    Ok((input, metadata)) => Ok((input, metadata)),
                    Err(_) => {
                        tracing::error!("falling back to ytdl!");
                        search_query_to_source_and_metadata_ytdl(client.clone(), query.clone())
                            .await
                    },
                }
            },
            QueryType::File(file) => {
                tracing::warn!("In File");
                Ok((
                    HttpRequest::new(client, file.url.to_owned()).into(),
                    vec![MyAuxMetadata::default()],
                ))
            },
            QueryType::NewYoutubeDl(ytdl_metadata) => {
                tracing::warn!("In NewYoutubeDl {:?}", ytdl_metadata.clone());
                let (ytdl, aux_metadata) = ytdl_metadata.clone();
                Ok((ytdl.into(), vec![MyAuxMetadata::Data(aux_metadata)]))
            },
            QueryType::PlaylistLink(url) => {
                tracing::warn!("In PlaylistLink");
                let rytdl = RustyYoutubeClient::new_with_client(client.clone()).unwrap();
                let search_options = SearchOptions {
                    limit: 100,
                    search_type: SearchType::Playlist,
                    ..Default::default()
                };

                let res = rytdl.rusty_ytdl.search(url, Some(&search_options)).await?;
                let mut metadata = Vec::with_capacity(res.len());
                for r in res {
                    metadata.push(MyAuxMetadata::Data(
                        RustyYoutubeClient::search_result_to_aux_metadata(&r),
                    ));
                }
                let ytdl = YoutubeDl::new(client.clone(), url.clone());
                tracing::warn!("ytdl: {:?}", ytdl);
                Ok((ytdl.into(), metadata))
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::error!("In SpotifyTracks, this is broken");
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                let mut ytdl = YoutubeDl::new(
                    client,
                    format!("ytsearch:{}", keywords_list.first().unwrap()),
                );
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = MyAuxMetadata::Data(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::warn!("In KeywordList");
                let mut ytdl =
                    YoutubeDl::new(client, format!("ytsearch:{}", keywords_list.join(" ")));
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = MyAuxMetadata::Data(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::None => unimplemented!(),
        }
    }
}

impl Query {
    pub fn build_query(&self) -> Option<String> {
        self.query_type.build_query()
    }

    pub async fn query(&self, n: usize) -> Result<(), CrackedError> {
        let _ = n;
        match self.query_type {
            QueryType::Keywords(_) => Ok(()),
            QueryType::KeywordList(_) => Ok(()),
            QueryType::VideoLink(_) => Ok(()),
            QueryType::SpotifyTracks(_) => Ok(()),
            QueryType::PlaylistLink(_) => Ok(()),
            QueryType::File(_) => Ok(()),
            QueryType::NewYoutubeDl(_) => Ok(()),
            QueryType::YoutubeSearch(_) => Ok(()),
            QueryType::None => Err(CrackedError::Other("No query provided!")),
        }
    }

    pub fn metadata(&self) -> Option<AuxMetadata> {
        match &self.query_type {
            QueryType::NewYoutubeDl((_src, metadata)) => Some(metadata.clone()),
            _ => None,
        }
    }

    pub async fn aux_metadata(&mut self) -> Result<AuxMetadata, CrackedError> {
        if let Some(meta) = self.metadata.as_ref() {
            return Ok(meta.clone());
        }

        self.query(1).await?;

        self.metadata.clone().ok_or_else(|| {
            CrackedError::Other("Failed to instansiate any metadata... Should be unreachable.")
            // let msg: Box<dyn std::error::Error + Send + Sync + 'static> =
            //     "Failed to instansiate any metadata... Should be unreachable.".into();
            // CrackedError::AudioStream(AudioStreamError::Fail(msg))
        })
    }
}

/// Get the guild name.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn get_guild_name_info(ctx: Context<'_>) -> Result<(), Error> {
    let _id = ctx.serenity_context().shard_id;
    ctx.say(format!(
        "The name of this guild is: {}",
        ctx.partial_guild().await.unwrap().name
    ))
    .await?;

    Ok(())
}

/// Sends the searching message after a play command is sent.
/// Also defers the interaction so we won't timeout.
#[cfg(not(tarpaulin_include))]
pub async fn send_search_message(ctx: Context<'_>) -> Result<Message, CrackedError> {
    let embed = CreateEmbed::default().description(format!("{}", CrackedMessage::Search));
    send_embed_response_poise(ctx, embed).await
}

/// Play a song next
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("next", "pn", "Pn", "insert", "ins", "push")
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
#[poise::command(slash_command, prefix_command, guild_only, aliases("s", "S"))]
pub async fn search(
    ctx: Context<'_>,
    #[rest]
    #[description = "search query."]
    query: String,
) -> Result<(), Error> {
    play_internal(ctx, Some("search".to_string()), None, Some(query)).await
}

/// Play a song.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only, aliases("p", "P"))]
pub async fn play(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    query: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, None, None, query).await
}

/// Play a song with more options
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, guild_only, aliases("opt"))]
pub async fn optplay(
    ctx: Context<'_>,
    #[description = "Play mode"] mode: Option<String>,
    #[description = "File to play."] file: Option<serenity::Attachment>,
    #[rest]
    #[description = "song link or search query."]
    query_or_url: Option<String>,
) -> Result<(), Error> {
    play_internal(ctx, mode, file, query_or_url).await
}

/// Does the actual playing of the song, all the other commands use this.
#[cfg(not(tarpaulin_include))]
async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // let search_msg = send_search_message(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // FIXME: This should be generalized.
    let prefix = ctx.prefix();
    let is_prefix = ctx.prefix() != "/";

    let msg = get_msg(mode.clone(), query_or_url, is_prefix);

    if msg.is_none() && file.is_none() {
        let embed = CreateEmbed::default()
            .description(format!("{}", CrackedError::Other("No query provided!")));
        send_embed_response_poise(ctx, embed).await?;
        return Ok(());
    }

    let (mode, msg) = get_mode(is_prefix, msg.clone(), mode);

    // TODO: Maybe put into it's own function?
    let url = match file.clone() {
        Some(file) => file.url.as_str().to_owned().to_string(),
        None => msg.clone(),
    };
    let url = url.as_str();

    tracing::warn!(target: "PLAY", "url: {}", url);

    // reply with a temporary message while we fetch the source
    // needed because interactions must be replied within 3s and queueing takes longer
    let mut search_msg = send_search_message(ctx).await?;

    ctx.data().add_msg_to_cache(guild_id, search_msg.clone());

    tracing::debug!("search response msg: {:?}", search_msg);

    let call = get_call_with_fail_msg(ctx).await?;

    // determine whether this is a link or a query string
    let query_type = get_query_type_from_url(ctx, url, file).await?;

    // FIXME: Decide whether we're using this everywhere, or not.
    // Don't like the inconsistency.
    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    // FIXME: Super hacky, fix this shit.
    let move_on = match_mode(ctx, call.clone(), mode, query_type.clone(), &mut search_msg).await?;

    if !move_on {
        return Ok(());
    }

    let _volume = {
        let mut settings = ctx.data().guild_settings_map.write().unwrap(); // .clone();
        let guild_settings = settings.entry(guild_id).or_insert_with(|| {
            GuildSettings::new(
                guild_id,
                Some(prefix),
                get_guild_name(ctx.serenity_context(), guild_id),
            )
        });
        guild_settings.volume
    };

    // let queue = call.lock().await.queue().current_queue().clone();
    // tracing::warn!("guild_settings: {:?}", guild_settings);
    // refetch the queue after modification
    // FIXME: I'm beginning to think that this walking of the queue is what's causing the performance issues.
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();
    // queue.first().map(|t| t.set_volume(volume).unwrap());
    // queue.iter().for_each(|t| t.set_volume(volume).unwrap());
    drop(handler);

    let embed = match queue.len().cmp(&1) {
        Ordering::Greater => {
            let estimated_time = calculate_time_until_play(&queue, mode).await.unwrap();

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
                        .description(format!("{}", CrackedMessage::PlaylistQueued))
                },
                (QueryType::File(_x_), y) => {
                    tracing::error!("QueryType::File, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
                (QueryType::YoutubeSearch(_x), y) => {
                    tracing::error!("QueryType::YoutubeSearch, mode: {:?}", y);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
                (x, y) => {
                    tracing::error!("{:?} {:?} {:?}", x, y, mode);
                    let track = queue.first().unwrap();
                    create_now_playing_embed(track).await
                },
            }
        },
        Ordering::Equal => {
            tracing::warn!("Only one track in queue, just playing it.");
            let track = queue.first().unwrap();
            create_now_playing_embed(track).await
            // print_queue(queue).await;
        },
        Ordering::Less => {
            tracing::warn!("No tracks in queue, this only happens when an interactive search is done with an empty queue.");
            CreateEmbed::default()
                .description("No tracks in queue!")
                .footer(CreateEmbedFooter::new("No tracks in queue!"))
        },
    };

    edit_embed_response(ctx, embed, search_msg.clone())
        .await
        .map(|_| ())
}

/// Edit the embed response of the given message.
#[cfg(not(tarpaulin_include))]
async fn edit_embed_response(
    ctx: Context<'_>,
    embed: CreateEmbed,
    mut msg: Message,
) -> Result<Message, Error> {
    match get_interaction(ctx) {
        Some(interaction) => interaction
            .edit_response(
                &ctx.serenity_context().http,
                EditInteractionResponse::new().add_embed(embed),
            )
            .await
            .map_err(Into::into),
        None => msg
            .edit(
                ctx.serenity_context().http.clone(),
                EditMessage::new().embed(embed),
            )
            .await
            .map(|_| msg)
            .map_err(Into::into),
    }
}

#[allow(dead_code)]
/// Print the current queue to the logs
async fn print_queue(queue: Vec<TrackHandle>) {
    for track in queue.iter() {
        let metadata = get_track_metadata(track).await;
        tracing::warn!(
            "Track {}: {} - {}, State: {:?}, Ready: {:?}",
            metadata.title.unwrap_or("".to_string()).red(),
            metadata.artist.unwrap_or("".to_string()).white(),
            metadata.album.unwrap_or("".to_string()).blue(),
            track.get_info().await.unwrap().playing,
            track.get_info().await.unwrap().ready,
        );
    }
}

/// Download a file and upload it as an mp3.
async fn download_file_ytdlp_mp3(url: &str) -> Result<(Output, AuxMetadata), Error> {
    let metadata = YoutubeDl::new(
        reqwest::ClientBuilder::new().use_rustls_tls().build()?,
        url.to_string(),
    )
    .aux_metadata()
    .await?;

    let args = [
        "--extract-audio",
        "--audio-format",
        "mp3",
        "--audio-quality",
        "0",
        url,
    ];
    let child = Command::new("yt-dlp")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    tracing::warn!("yt-dlp");

    let output = child.wait_with_output().await?;
    Ok((output, metadata))
}

/// Download a file and upload it as an attachment.
async fn download_file_ytdlp(url: &str, mp3: bool) -> Result<(Output, AuxMetadata), Error> {
    if mp3 || url.contains("youtube.com") {
        return download_file_ytdlp_mp3(url).await;
    }

    let metadata = YoutubeDl::new(http_utils::get_client().clone(), url.to_string())
        .aux_metadata()
        .await?;

    let child = Command::new("yt-dlp")
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    tracing::warn!("yt-dlp");

    let output = child.wait_with_output().await?;
    Ok((output, metadata))
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

async fn match_mode<'a>(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    mode: Mode,
    query_type: QueryType,
    search_msg: &'a mut Message,
) -> Result<bool, Error> {
    // let is_prefix = ctx.prefix() != "/";
    // let user_id = ctx.author().id;
    // let user_id_i64 = ctx.author().id.get() as i64;
    let handler = call.lock().await;
    let queue_was_empty = handler.queue().is_empty();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    drop(handler);

    // let pool = ctx.data().database_pool.clone().unwrap();

    tracing::info!("mode: {:?}", mode);

    let reqwest_client = ctx.data().http_client.clone();
    match mode {
        Mode::Search => {
            // let search_results = match query_type.clone() {
            match query_type.clone() {
                QueryType::Keywords(keywords) => {
                    let search_results = YoutubeDl::new_search(reqwest_client, keywords)
                        .search(None)
                        .await?;
                    // let user_id = ctx.author().id;
                    let qt = yt_search_select(
                        ctx.serenity_context().clone(),
                        ctx.channel_id(),
                        search_results,
                    )
                    .await?;
                    let queue = enqueue_track_pgwrite(ctx, &call, &qt).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await
                    // match_mode(ctx, call.clone(), Mode::End, qt).await
                    // send_search_response(ctx, guild_id, user_id, keywords, search_results).await?;
                },
                QueryType::YoutubeSearch(query) => {
                    let search_results = YoutubeDl::new(reqwest_client, query.clone())
                        .search(None)
                        .await?;
                    let qt = yt_search_select(
                        ctx.serenity_context().clone(),
                        ctx.channel_id(),
                        search_results,
                    )
                    .await?;
                    // match_mode(ctx, call.clone(), Mode::End, qt).await
                    let queue = enqueue_track_pgwrite(ctx, &call, &qt).await?;
                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await

                    // let user_id = ctx.author().id;

                    // send_search_response(ctx, guild_id, user_id, query, search_results).await?;
                },
                _ => {
                    let embed = CreateEmbed::default()
                        .description(format!(
                            "{}",
                            CrackedError::Other("Something went wrong while parsing your query!")
                        ))
                        .footer(CreateEmbedFooter::new("Search failed!"));
                    send_embed_response_poise(ctx, embed).await?;
                    return Ok(false);
                },
            };
        },
        Mode::DownloadMKV => {
            let (status, file_name) =
                get_download_status_and_filename(query_type.clone(), false).await?;
            ctx.channel_id()
                .send_message(
                    ctx,
                    CreateMessage::new()
                        .content(format!("Download status {}", status))
                        .add_file(CreateAttachment::path(Path::new(&file_name)).await?),
                )
                .await?;

            return Ok(false);
        },
        Mode::DownloadMP3 => {
            let (status, file_name) =
                get_download_status_and_filename(query_type.clone(), true).await?;
            ctx.channel_id()
                .send_message(
                    ctx.http(),
                    CreateMessage::new()
                        .content(format!("Download status {}", status))
                        .add_file(CreateAttachment::path(Path::new(&file_name)).await?),
                )
                .await?;

            return Ok(false);
        },
        Mode::End => match query_type.clone() {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::Jump, QueryType::YoutubeSearch");

                let res = YoutubeDl::new_search(http_utils::get_client().clone(), query.clone())
                    .search(None)
                    .await?;
                let user_id = ctx.author().id;
                send_search_response(ctx, guild_id, user_id, query.clone(), res).await?;
            },
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::NewYoutubeDl(_) => {
                tracing::warn!("### Mode::End, QueryType::Keywords | QueryType::VideoLink");
                let queue = enqueue_track_pgwrite(ctx, &call, &query_type).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            },
            // FIXME
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::End, QueryType::PlaylistLink");
                // Let's use the new YouTube rust library for this
                let rusty_ytdl = RustyYoutubeClient::new()?;
                let playlist: Playlist = rusty_ytdl.get_playlist(url).await?;
                queue_yt_playlist(ctx, call, guild_id, playlist, search_msg).await?;
            },
            QueryType::SpotifyTracks(tracks) => {
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                queue_keyword_list_back(ctx, call, keywords_list, search_msg).await?;
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::End, QueryType::KeywordList");
                queue_keyword_list_back(ctx, call, keywords_list, search_msg).await?;
            },
            QueryType::File(file) => {
                tracing::trace!("Mode::End, QueryType::File");
                let queue = enqueue_track_pgwrite(ctx, &call, &QueryType::File(file)).await?;
                update_queue_messages(ctx.http(), ctx.data(), &queue, guild_id).await;
            },
            QueryType::None => {
                tracing::trace!("Mode::End, QueryType::None");
                let embed = CreateEmbed::default()
                    .description(format!("{}", CrackedError::Other("No query provided!")))
                    .footer(CreateEmbedFooter::new("No query provided!"));
                send_embed_response_poise(ctx, embed).await?;
                return Ok(false);
            },
        },
        Mode::Next => match query_type.clone() {
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
                tracing::trace!(
                    "Mode::Next, QueryType::Keywords | QueryType::VideoLink | QueryType::File"
                );
                let queue = insert_track(ctx, &call, &query_type, 1).await?;
                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            },
            // FIXME
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::Next, QueryType::PlaylistLink");
                let rusty_ytdl = RustyYoutubeClient::new()?;
                let playlist: Playlist = rusty_ytdl.get_playlist(url).await?;
                queue_yt_playlist_front(ctx, call, guild_id, playlist, search_msg).await?;
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::Other("failed to fetch playlist"))?;
                // let urls = vec!["".to_string()];

                // for (idx, url) in urls.into_iter().enumerate() {
                //     let queue =
                //         insert_track(ctx, &call, &QueryType::VideoLink(url), idx + 1).await?;
                //     update_queue_messages(
                //         &ctx.serenity_context().http,
                //         ctx.data(),
                //         &queue,
                //         guild_id,
                //     )
                //     .await;
                // }
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::Next, QueryType::KeywordList");
                let q_not_empty = if call.clone().lock().await.queue().is_empty() {
                    0
                } else {
                    1
                };
                queue_keyword_list_w_offset(ctx, call, keywords_list, q_not_empty, search_msg)
                    .await?;
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::trace!("Mode::Next, QueryType::KeywordList");
                let q_not_empty = if call.clone().lock().await.queue().is_empty() {
                    0
                } else {
                    1
                };
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                queue_keyword_list_w_offset(ctx, call, keywords_list, q_not_empty, search_msg)
                    .await?;
            },
            QueryType::YoutubeSearch(_) => {
                tracing::trace!("Mode::Next, QueryType::YoutubeSearch");
                return Err(CrackedError::Other("Not implemented yet!").into());
            },
            QueryType::None => {
                tracing::trace!("Mode::Next, QueryType::None");
                let embed = CreateEmbed::default()
                    .description(format!("{}", CrackedError::Other("No query provided!")))
                    .footer(CreateEmbedFooter::new("No query provided!"));
                send_embed_response_poise(ctx, embed).await?;
                return Ok(false);
            },
        },
        Mode::Jump => match query_type.clone() {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::Jump, QueryType::YoutubeSearch");
                tracing::error!("query: {}", query);
                return Err(CrackedError::Other("Not implemented yet!").into());
            },
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
                tracing::trace!(
                    "Mode::Jump, QueryType::Keywords | QueryType::VideoLink | QueryType::File"
                );
                let mut queue = enqueue_track_pgwrite(ctx, &call, &query_type).await?;

                if !queue_was_empty {
                    rotate_tracks(&call, 1).await.ok();
                    queue = force_skip_top_track(&call.lock().await).await?;
                }

                update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id)
                    .await;
            },
            QueryType::PlaylistLink(url) => {
                tracing::error!("Mode::Jump, QueryType::PlaylistLink");
                // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
                //     .await
                //     .ok_or(CrackedError::PlayListFail)?;
                // FIXME
                let _src = YoutubeDl::new(Client::new(), url);
                // .ok_or(CrackedError::Other("failed to fetch playlist"))?
                // .into_iter()
                // .for_each(|track| async {
                //     let _ = enqueue_track(&call, &QueryType::File(track)).await;
                // });
                let urls = vec!["".to_string()];
                let mut insert_idx = 1;

                for (i, url) in urls.into_iter().enumerate() {
                    let mut queue =
                        insert_track(ctx, &call, &QueryType::VideoLink(url), insert_idx).await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            },
            // FIXME
            QueryType::SpotifyTracks(tracks) => {
                let mut insert_idx = 1;
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();

                for (i, keywords) in keywords_list.into_iter().enumerate() {
                    let mut queue =
                        insert_track(ctx, &call, &QueryType::Keywords(keywords), insert_idx)
                            .await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            },
            // FIXME
            QueryType::KeywordList(keywords_list) => {
                tracing::error!("Mode::Jump, QueryType::KeywordList");
                let mut insert_idx = 1;

                for (i, keywords) in keywords_list.into_iter().enumerate() {
                    let mut queue =
                        insert_track(ctx, &call, &QueryType::Keywords(keywords), insert_idx)
                            .await?;

                    if i == 0 && !queue_was_empty {
                        queue = force_skip_top_track(&call.lock().await).await?;
                    } else {
                        insert_idx += 1;
                    }

                    update_queue_messages(
                        &ctx.serenity_context().http,
                        ctx.data(),
                        &queue,
                        guild_id,
                    )
                    .await;
                }
            },
            QueryType::None => {
                tracing::trace!("Mode::Next, QueryType::None");
                let embed = CreateEmbed::default()
                    .description(format!("{}", CrackedError::Other("No query provided!")))
                    .footer(CreateEmbedFooter::new("No query provided!"));
                send_embed_response_poise(ctx, embed).await?;
                return Ok(false);
            },
        },
        Mode::All | Mode::Reverse | Mode::Shuffle => match query_type.clone() {
            QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::VideoLink | QueryType::PlaylistLink");
                // FIXME
                let mut src = YoutubeDl::new(http_utils::get_client().clone(), url);
                let metadata = src.aux_metadata().await?;
                enqueue_track_pgwrite(ctx, &call, &QueryType::NewYoutubeDl((src, metadata)))
                    .await?;
                update_queue_messages(
                    &ctx.serenity_context().http,
                    ctx.data(),
                    &call.lock().await.queue().current_queue(),
                    guild_id,
                )
                .await;
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!(
                    "Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::KeywordList"
                );
                queue_keyword_list_back(ctx, call, keywords_list, search_msg).await?;
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::trace!(
                    "Mode::All | Mode::Reverse | Mode::Shuffle, QueryType::KeywordList"
                );
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                queue_keyword_list_back(ctx, call, keywords_list, search_msg).await?;
            },
            _ => {
                ctx.defer().await?; // Why did I do this?
                edit_response_poise(ctx, CrackedMessage::PlayAllFailed).await?;
                return Ok(false);
            },
        },
    }

    Ok(true)
}

use colored::Colorize;
/// Matches a url (or query string) to a QueryType
pub async fn get_query_type_from_url(
    ctx: Context<'_>,
    url: &str,
    file: Option<Attachment>,
) -> Result<Option<QueryType>, Error> {
    // determine whether this is a link or a query string
    tracing::warn!("url: {}", url);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") | Some("spotify.link") => {
                let final_url = http_utils::resolve_final_url(url).await?;
                tracing::warn!("spotify: {} -> {}", url, final_url);
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, &final_url).await?)
            },
            Some("cdn.discordapp.com") => {
                tracing::warn!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            },

            Some(other) => {
                let mut settings = ctx.data().guild_settings_map.write().unwrap().clone();
                let guild_settings = settings.entry(guild_id).or_insert_with(|| {
                    GuildSettings::new(
                        guild_id,
                        Some(ctx.prefix()),
                        get_guild_name(ctx.serenity_context(), guild_id),
                    )
                });
                if !guild_settings.allow_all_domains.unwrap_or(true) {
                    let is_allowed = guild_settings
                        .allowed_domains
                        .iter()
                        .any(|d| compare_domains(d, other));

                    let is_banned = guild_settings
                        .banned_domains
                        .iter()
                        .any(|d| compare_domains(d, other));

                    if is_banned || (guild_settings.banned_domains.is_empty() && !is_allowed) {
                        let message = CrackedMessage::PlayDomainBanned {
                            domain: other.to_string(),
                        };

                        send_response_poise_text(ctx, message).await?;
                    }
                }

                // Handle youtube playlist
                if url.contains("list=") {
                    tracing::warn!("{}: {}", "youtube playlist".blue(), url.underline().blue());
                    Some(QueryType::PlaylistLink(url.to_string()))
                } else {
                    tracing::warn!("{}: {}", "youtube video".blue(), url.underline().blue());
                    let rusty_ytdl = RustyYoutubeClient::new()?;
                    let res_info = rusty_ytdl.get_video_info(url.to_string()).await;
                    let metadata = match res_info {
                        Ok(info) => {
                            tracing::warn!("info: {:?}", info);
                            RustyYoutubeClient::video_info_to_aux_metadata(&info)
                        },
                        _ => {
                            tracing::warn!("info: None, falling back to yt-dlp");
                            AuxMetadata {
                                source_url: Some(url.to_string()),
                                ..AuxMetadata::default()
                            }
                        },
                    };
                    let yt = YoutubeDl::new(http_utils::get_client().clone(), url.to_string());
                    Some(QueryType::NewYoutubeDl((yt, metadata)))
                }
            },
            None => {
                // handle spotify:track:3Vr5jdQHibI2q0A0KW4RWk format?
                // TODO: Why is this a thing?
                if url.starts_with("spotify:") {
                    let parts = url.split(':').collect::<Vec<_>>();
                    let final_url =
                        format!("https://open.spotify.com/track/{}", parts.last().unwrap());
                    tracing::warn!("spotify: {} -> {}", url, final_url);
                    let spotify = SPOTIFY.lock().await;
                    let spotify =
                        verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                    Some(Spotify::extract(spotify, &final_url).await?)
                } else {
                    Some(QueryType::Keywords(url.to_string()))
                    //                None
                }
            },
        },
        Err(e) => {
            tracing::error!("Url::parse error: {}", e);
            Some(QueryType::Keywords(url.to_string()))
        },
    };

    let res = if let Some(QueryType::Keywords(_)) = query_type {
        let settings = ctx.data().guild_settings_map.write().unwrap().clone();
        let guild_settings = settings.get(&guild_id).unwrap();
        if !guild_settings.allow_all_domains.unwrap_or(true)
            && (guild_settings.banned_domains.contains("youtube.com")
                || (guild_settings.banned_domains.is_empty()
                    && !guild_settings.allowed_domains.contains("youtube.com")))
        {
            let message = CrackedMessage::PlayDomainBanned {
                domain: "youtube.com".to_string(),
            };

            send_response_poise_text(ctx, message).await?;
            Ok(None)
        } else {
            Result::Ok(query_type)
        }
    } else {
        Result::Ok(query_type)
    };
    res
}

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
    let metadata = get_track_metadata(top_track).await;

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

/// Enum for the requesting user of a track.
#[derive(Debug, Clone)]
pub enum RequestingUser {
    UserId(UserId),
}

/// We implement TypeMapKey for RequestingUser.
impl TypeMapKey for RequestingUser {
    type Value = RequestingUser;
}

/// Default implementation for RequestingUser.
impl Default for RequestingUser {
    /// Defualt is UserId(1).
    fn default() -> Self {
        let user = UserId::new(1);
        RequestingUser::UserId(user)
    }
}

/// AuxMetadata wrapper and utility functions.
#[derive(Debug, Clone)]
pub enum MyAuxMetadata {
    Data(AuxMetadata),
}

/// Implement TypeMapKey for MyAuxMetadata.
impl TypeMapKey for MyAuxMetadata {
    type Value = MyAuxMetadata;
}

/// Implement Default for MyAuxMetadata.
impl Default for MyAuxMetadata {
    fn default() -> Self {
        MyAuxMetadata::Data(AuxMetadata::default())
    }
}

/// Implement MyAuxMetadata.
impl MyAuxMetadata {
    /// Create a new MyAuxMetadata from AuxMetadata.
    pub fn new(metadata: AuxMetadata) -> Self {
        MyAuxMetadata::Data(metadata)
    }

    /// Get the internal metadata.
    pub fn metadata(&self) -> &AuxMetadata {
        match self {
            MyAuxMetadata::Data(metadata) => metadata,
        }
    }

    /// Create new MyAuxMetadata from &SpotifyTrack.
    pub fn from_spotify_track(track: &SpotifyTrack) -> Self {
        MyAuxMetadata::Data(AuxMetadata {
            track: Some(track.name()),
            artist: Some(track.artists_str()),
            album: Some(track.album_name()),
            date: None,
            start_time: Some(Duration::ZERO),
            duration: Some(track.duration()),
            channels: Some(2),
            channel: None,
            sample_rate: None,
            source_url: None,
            thumbnail: Some(track.name()),
            title: Some(track.name()),
        })
    }

    /// Set the source_url.
    pub fn with_source_url(self, source_url: String) -> Self {
        MyAuxMetadata::Data(AuxMetadata {
            source_url: Some(source_url),
            ..self.metadata().clone()
        })
    }

    /// Get a search query from the metadata for youtube.
    pub fn get_search_query(&self) -> String {
        let metadata = self.metadata();
        let title = metadata.title.clone().unwrap_or_default();
        let artist = metadata.artist.clone().unwrap_or_default();
        format!("{} {}", title, artist)
    }
}

/// Implementation to convert `[&SpotifyTrack]` to `[MyAuxMetadata]`.
impl From<&SpotifyTrack> for MyAuxMetadata {
    fn from(track: &SpotifyTrack) -> Self {
        MyAuxMetadata::from_spotify_track(track)
    }
}

/// Build an embed for the cure
async fn build_queued_embed(
    author_title: &str,
    track: &TrackHandle,
    estimated_time: Duration,
) -> CreateEmbed {
    // FIXME
    let metadata = {
        let map = track.typemap().read().await;
        let my_metadata = map.get::<MyAuxMetadata>().unwrap();

        match my_metadata {
            MyAuxMetadata::Data(metadata) => metadata.clone(),
        }
    };
    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();
    let meta_title = metadata.title.clone().unwrap_or(QUEUE_NO_TITLE.to_string());
    let source_url = metadata
        .source_url
        .clone()
        .unwrap_or(QUEUE_NO_SRC.to_string());

    // let title_text = &format!("[**{}**]({})", meta_title, source_url);

    let footer_text = format!(
        "{}{}\n{}{}",
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
        .footer(CreateEmbedFooter::new(footer_text))
}

// FIXME: Do you want to have a reqwest client we keep around and pass into
// this instead of creating a new one every time?
// FIXME: This is super expensive, literally we need to do this a lot better.
async fn get_download_status_and_filename(
    query_type: QueryType,
    mp3: bool,
) -> Result<(bool, String), Error> {
    // FIXME: Don't hardcode this.
    let prefix = "/data/downloads";
    let extension = if mp3 { "mp3" } else { "webm" };
    let client = http_utils::get_client().clone();
    tracing::warn!("query_type: {:?}", query_type);
    match query_type {
        QueryType::YoutubeSearch(_) => Err(Box::new(CrackedError::Other(
            "Download not valid with search results.",
        ))),
        QueryType::VideoLink(url) => {
            tracing::warn!("Mode::Download, QueryType::VideoLink");
            let (output, metadata) = download_file_ytdlp(&url, mp3).await?;
            let status = output.status.success();
            let url = metadata.source_url.unwrap();
            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            Ok((status, file_name))
        },
        QueryType::NewYoutubeDl((_src, metadata)) => {
            tracing::warn!("Mode::Download, QueryType::NewYoutubeDl");
            let url = metadata.source_url.unwrap();
            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            tracing::warn!("file_name: {}", file_name);
            let (output, _metadata) = download_file_ytdlp(&url, mp3).await?;
            let status = output.status.success();
            Ok((status, file_name))
        },
        QueryType::Keywords(query) => {
            tracing::warn!("In Keywords");
            let mut ytdl = YoutubeDl::new(client, format!("ytsearch:{}", query));
            let metadata = ytdl.aux_metadata().await.unwrap();
            let url = metadata.source_url.unwrap();
            let (output, metadata) = download_file_ytdlp(&url, mp3).await?;

            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            let status = output.status.success();
            Ok((status, file_name))
        },
        QueryType::File(file) => {
            tracing::warn!("In File");
            Ok((true, file.url.to_owned().to_string()))
        },
        QueryType::PlaylistLink(url) => {
            tracing::warn!("In PlaylistLink");
            let (output, metadata) = download_file_ytdlp(&url, mp3).await?;
            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            let status = output.status.success();
            Ok((status, file_name))
        },
        QueryType::SpotifyTracks(tracks) => {
            tracing::warn!("In SpotifyTracks");
            let keywords_list = tracks
                .iter()
                .map(|x| x.build_query())
                .collect::<Vec<String>>();
            let url = format!("ytsearch:{}", keywords_list.first().unwrap());
            let mut ytdl = YoutubeDl::new(client, url.clone());
            let metadata = ytdl.aux_metadata().await.unwrap();
            let (output, _metadata) = download_file_ytdlp(&url, mp3).await?;
            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            let status = output.status.success();
            Ok((status, file_name))
        },
        QueryType::KeywordList(keywords_list) => {
            tracing::warn!("In KeywordList");
            let url = format!("ytsearch:{}", keywords_list.join(" "));
            let mut ytdl = YoutubeDl::new(client, url.clone());
            tracing::warn!("ytdl: {:?}", ytdl);
            let metadata = ytdl.aux_metadata().await.unwrap();
            let (output, _metadata) = download_file_ytdlp(&url, mp3).await?;
            let file_name = format!(
                "{}/{} [{}].{}",
                prefix,
                metadata.title.unwrap(),
                url.split('=').last().unwrap(),
                extension,
            );
            let status = output.status.success();
            Ok((status, file_name))
        },
        QueryType::None => Err(Box::new(CrackedError::Other("No query provided!"))),
    }
}
/// Add tracks to the queue from aux_metadata.
#[cfg(not(tarpaulin_include))]
pub async fn queue_aux_metadata(
    ctx: Context<'_>,
    aux_metadata: &[MyAuxMetadata],
    mut msg: Message,
) -> Result<(), CrackedError> {
    use super::youtube::build_query_aux_metadata;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let search_results = aux_metadata;

    let client = &ctx.data().http_client;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;

    for metadata in search_results {
        let source_url = metadata.metadata().source_url.as_ref();
        let metadata_final = if source_url.is_none() || source_url.unwrap().is_empty() {
            let search_query = build_query_aux_metadata(metadata.metadata());
            let _ = msg
                .edit(
                    ctx,
                    EditMessage::default().content(format!("Queuing... {}", search_query)),
                )
                .await;

            let ytdl = RustyYoutubeClient::new_with_client(client.clone())?;
            let res = ytdl.one_shot(search_query).await?;
            let res = res.ok_or(CrackedError::Other("No results found"))?;
            let new_aux_metadata = RustyYoutubeClient::search_result_to_aux_metadata(&res);

            MyAuxMetadata::Data(new_aux_metadata)
        } else {
            metadata.clone()
        };

        let ytdl = YoutubeDl::new(
            client.clone(),
            metadata_final.metadata().source_url.clone().unwrap(),
        );

        let query_type = QueryType::NewYoutubeDl((ytdl, metadata_final.metadata().clone()));
        let queue = enqueue_track_pgwrite(ctx, &call, &query_type).await?;
        update_queue_messages(&ctx.serenity_context().http, ctx.data(), &queue, guild_id).await;
    }

    Ok(())
}
