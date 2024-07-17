use super::queue::{queue_track_back, queue_track_front};
use super::{queue_keyword_list_back, queue_query_list_offset};
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::interface::create_search_response;
use crate::CrackedResult;
use crate::{
    commands::{check_banned_domains, MyAuxMetadata},
    errors::{verify, CrackedError},
    http_utils,
    messaging::{
        interface::{send_no_query_provided, send_search_failed},
        message::CrackedMessage,
        messages::SPOTIFY_AUTH_FAILED,
    },
    sources::{
        rusty_ytdl::RustyYoutubeClient,
        spotify::{Spotify, SpotifyTrack, SPOTIFY},
    },
    utils::{edit_response_poise, yt_search_select},
    Context, Error,
};
use ::serenity::all::{Attachment, CreateAttachment, CreateMessage};
use colored::Colorize;
use poise::serenity_prelude as serenity;
use rusty_ytdl::search::{Playlist, SearchOptions, SearchType};
use songbird::{
    input::{AuxMetadata, Compose as _, HttpRequest, Input as SongbirdInput, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{
    ops::Deref,
    path::Path,
    process::{Output, Stdio},
    sync::Arc,
};
use tokio::{process::Command, sync::Mutex};
use url::Url;

#[derive(Clone, Debug)]
/// Enum for type of possible queries we have to handle
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

pub struct Queries {
    queries: Vec<QueryType>,
}

impl Queries {
    pub fn new(queries: Vec<QueryType>) -> Self {
        Self { queries }
    }

    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queries.len()
    }

    pub fn iter(&self) -> std::slice::Iter<QueryType> {
        self.queries.iter()
    }
}

impl Deref for Queries {
    type Target = Vec<QueryType>;

    fn deref(&self) -> &Self::Target {
        &self.queries
    }
}

impl From<Vec<String>> for Queries {
    fn from(v: Vec<String>) -> Self {
        let queries = v.into_iter().map(QueryType::Keywords).collect();
        Queries::new(queries)
    }
}

impl From<Vec<SpotifyTrack>> for Queries {
    fn from(v: Vec<SpotifyTrack>) -> Self {
        let queries = v
            .into_iter()
            .map(|x| QueryType::Keywords(x.build_query()))
            .collect();
        Queries::new(queries)
    }
}

impl From<Playlist> for Queries {
    fn from(v: Playlist) -> Self {
        let queries = v
            .videos
            .into_iter()
            .map(|x| QueryType::VideoLink(x.url))
            .collect();
        Queries::new(queries)
    }
}

impl From<Queries> for Vec<QueryType> {
    fn from(q: Queries) -> Vec<QueryType> {
        q.queries
    }
}

impl QueryType {
    /// Build a query string from the query type.
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
    // FIXME: This is super expensive, literally we need to do this a lot better.
    pub async fn get_download_status_and_filename(
        &self,
        mp3: bool,
    ) -> Result<(bool, String), Error> {
        // FIXME: Don't hardcode this.
        let prefix = "/data/downloads";
        let extension = if mp3 { "mp3" } else { "webm" };
        let client = http_utils::get_client().clone();
        // tracing::warn!("query_type: {:?}", query_type);
        match self {
            QueryType::YoutubeSearch(_) => Err(Box::new(CrackedError::Other(
                "Download not valid with search results.",
            ))),
            QueryType::VideoLink(url) => {
                tracing::warn!("Mode::Download, QueryType::VideoLink");
                let (output, metadata) = download_file_ytdlp(url, mp3).await?;
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
                let url = metadata.source_url.as_ref().unwrap();
                let file_name = format!(
                    "{}/{} [{}].{}",
                    prefix,
                    metadata.title.as_ref().unwrap(),
                    url.split('=').last().unwrap(),
                    extension,
                );
                tracing::warn!("file_name: {}", file_name);
                let (output, _metadata) = download_file_ytdlp(url, mp3).await?;
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
                let (output, metadata) = download_file_ytdlp(url, mp3).await?;
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

    pub async fn mode_download(&self, ctx: Context<'_>, mp3: bool) -> Result<bool, CrackedError> {
        let (status, file_name) = self.get_download_status_and_filename(mp3).await?;
        ctx.channel_id()
            .send_message(
                ctx,
                CreateMessage::new()
                    .content(format!("Download status {}", status))
                    .add_file(CreateAttachment::path(Path::new(&file_name)).await?),
            )
            .await?;

        Ok(false)
    }

    pub async fn mode_search(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        match self {
            QueryType::Keywords(keywords) => {
                self.mode_search_keywords(ctx, call, keywords.clone()).await
            },
            QueryType::SpotifyTracks(tracks) => {
                self.mode_search_keywords(
                    ctx,
                    call,
                    tracks
                        .iter()
                        .map(|x| x.build_query())
                        .collect::<Vec<String>>()
                        .join(" "),
                )
                .await
            },
            QueryType::YoutubeSearch(query) => {
                self.mode_search_keywords(ctx, call, query.clone()).await
            },
            _ => send_search_failed(&ctx).await.map(|_| Vec::new()),
        }
    }

    pub async fn mode_search_keywords(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        keywords: String,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        let reqwest_client = ctx.data().http_client.clone();
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
        queue_track_back(ctx, &call, &qt).await
        // update_queue_messages(ctx, ctx.data(), &queue, guild_id).await
    }

    pub async fn mode_next(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_msg: &mut serenity::Message,
    ) -> Result<bool, CrackedError> {
        match self {
            QueryType::Keywords(_)
            | QueryType::VideoLink(_)
            | QueryType::File(_)
            | QueryType::NewYoutubeDl(_) => {
                tracing::info!("Mode::Next, QueryType::Keywords|VideoLink|File|NewYoutubeDl");
                queue_track_front(ctx, &call, self).await?;
            },
            // FIXME
            QueryType::PlaylistLink(url) => {
                let _guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
                let rusty_ytdl = RustyYoutubeClient::new()?;
                let playlist: Playlist = rusty_ytdl.get_playlist(url.clone()).await?;
                queue_query_list_offset(ctx, call, Queries::from(playlist).to_vec(), 1, search_msg)
                    .await?;
            },
            QueryType::KeywordList(keywords_list) => {
                queue_query_list_offset(
                    ctx,
                    call,
                    Queries::from(keywords_list.clone()).to_vec(),
                    1,
                    search_msg,
                )
                .await?;
            },
            QueryType::SpotifyTracks(tracks) => {
                // let keywords_list = tracks
                //     .iter()
                //     .map(|x| x.build_query())
                //     .collect::<Vec<String>>();
                queue_query_list_offset(
                    ctx,
                    call,
                    Queries::from(tracks.clone()).to_vec(),
                    1,
                    search_msg,
                )
                .await?;
            },
            QueryType::YoutubeSearch(_) => {
                return Err(CrackedError::Other("Not implemented yet!"));
            },
            QueryType::None => {
                return Ok(false);
            },
        }
        Ok(true)
    }

    pub async fn mode_end(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_msg: &mut crate::Message,
    ) -> Result<bool, CrackedError> {
        let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::End, QueryType::YoutubeSearch");

                let res = YoutubeDl::new_search(http_utils::get_client().clone(), query.clone())
                    .search(None)
                    .await?;
                let user_id = ctx.author().id;
                create_search_response(&ctx, guild_id, user_id, query.clone(), res).await?;
                Ok(true)
            },
            QueryType::Keywords(_) | QueryType::VideoLink(_) | QueryType::NewYoutubeDl(_) => {
                tracing::warn!("### Mode::End, QueryType::Keywords | QueryType::VideoLink");
                match queue_track_back(ctx, &call, self).await {
                    Ok(_) => (),
                    Err(e) => {
                        tracing::error!("queue_track_back error: {:?}", e);
                        return Ok(false);
                    },
                };
                Ok(true)
            },
            // FIXME
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::End, QueryType::PlaylistLink");
                // Let's use the new YouTube rust library for this
                let rusty_ytdl = RustyYoutubeClient::new()?;
                let playlist: Playlist = rusty_ytdl.get_playlist(url.clone()).await?;
                queue_keyword_list_back(ctx, call, Queries::from(playlist).to_vec(), search_msg)
                    .await?;
                // queue_yt_playlist(ctx, call, guild_id, playlist, search_msg).await?;
                Ok(true)
            },
            QueryType::SpotifyTracks(tracks) => {
                let queries = tracks
                    .iter()
                    .map(|x| QueryType::Keywords(x.build_query()))
                    .collect::<Vec<QueryType>>();
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::trace!("Mode::End, QueryType::KeywordList");
                let queries = keywords_list
                    .iter()
                    .map(|x| QueryType::Keywords(x.clone()))
                    .collect::<Vec<QueryType>>();
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            QueryType::File(file) => {
                tracing::trace!("Mode::End, QueryType::File");
                let _queue = queue_track_back(ctx, &call, &QueryType::File(file.clone())).await?;
                // update_queue_messages(ctx.http(), ctx.data(), &queue, guild_id).await;
                Ok(true)
            },
            QueryType::None => send_no_query_provided(&ctx).await.map(|_| false),
        }
    }

    pub async fn mode_rest(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        search_msg: &mut crate::Message,
    ) -> Result<bool, CrackedError> {
        match self {
            QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => {
                // FIXME
                let mut src = YoutubeDl::new(http_utils::get_client().clone(), url.clone());
                let metadata = src.aux_metadata().await?;
                queue_track_back(ctx, &call, &QueryType::NewYoutubeDl((src, metadata))).await?;
                Ok(true)
            },
            QueryType::KeywordList(keywords_list) => {
                let queries = keywords_list
                    .iter()
                    .map(|x| QueryType::Keywords(x.clone()))
                    .collect::<Vec<QueryType>>();
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            QueryType::SpotifyTracks(tracks) => {
                let queries = tracks
                    .iter()
                    .map(|x| QueryType::Keywords(x.build_query()))
                    .collect::<Vec<QueryType>>();
                queue_keyword_list_back(ctx, call, queries, search_msg).await?;
                Ok(true)
            },
            _ => {
                ctx.defer().await?; // Why did I do this?
                edit_response_poise(&ctx, CrackedMessage::PlayAllFailed).await?;
                Ok(false)
            },
        }
    }

    pub async fn mode_jump(
        &self,
        _ctx: Context<'_>,
        _call: Arc<Mutex<Call>>,
    ) -> Result<bool, CrackedError> {
        Err(CrackedError::Other("Not implemented yet!"))
        // match self {
        //     QueryType::YoutubeSearch(query) => {
        //         return Err(CrackedError::Other("Not implemented yet!").into());
        //     },
        //     QueryType::Keywords(_)
        //     | QueryType::VideoLink(_)
        //     | QueryType::File(_)
        //     | QueryType::NewYoutubeDl(_) => {
        //         let mut queue = enqueue_track_pgwrite(ctx, &call, &query_type).await?;

        //         if !queue_was_empty {
        //             rotate_tracks(&call, 1).await.ok();
        //             queue = force_skip_top_track(&call.lock().await).await?;
        //         }
        //     },
        //     QueryType::PlaylistLink(url) => {
        //         tracing::error!("Mode::Jump, QueryType::PlaylistLink");
        //         // let urls = YouTubeRestartable::ytdl_playlist(&url, mode)
        //         //     .await
        //         //     .ok_or(CrackedError::PlayListFail)?;
        //         // FIXME
        //         let _src = YoutubeDl::new(Client::new(), url);
        //         // .ok_or(CrackedError::Other("failed to fetch playlist"))?
        //         // .into_iter()
        //         // .for_each(|track| async {
        //         //     let _ = enqueue_track(&call, &QueryType::File(track)).await;
        //         // });
        //         let urls = vec!["".to_string()];
        //         let mut insert_idx = 1;

        //         for (i, url) in urls.into_iter().enumerate() {
        //             let mut queue =
        //                 insert_track(ctx, &call, &QueryType::VideoLink(url), insert_idx).await?;

        //             if i == 0 && !queue_was_empty {
        //                 queue = force_skip_top_track(&call.lock().await).await?;
        //             } else {
        //                 insert_idx += 1;
        //             }
        //         }
        //     },
        //     // FIXME
        //     QueryType::SpotifyTracks(tracks) => {
        //         let mut insert_idx = 1;
        //         let keywords_list = tracks
        //             .iter()
        //             .map(|x| x.build_query())
        //             .collect::<Vec<String>>();

        //         for (i, keywords) in keywords_list.into_iter().enumerate() {
        //             let mut queue =
        //                 insert_track(ctx, &call, &QueryType::Keywords(keywords), insert_idx)
        //                     .await?;

        //             if i == 0 && !queue_was_empty {
        //                 queue = force_skip_top_track(&call.lock().await).await?;
        //             } else {
        //                 insert_idx += 1;
        //             }
        //         }
        //     },
        //     // FIXME
        //     QueryType::KeywordList(keywords_list) => {
        //         let mut insert_idx = 1;

        //         for (i, keywords) in keywords_list.into_iter().enumerate() {
        //             let mut queue =
        //                 insert_track(ctx, &call, &QueryType::Keywords(keywords), insert_idx)
        //                     .await?;

        //             if i == 0 && !queue_was_empty {
        //                 queue = force_skip_top_track(&call.lock().await).await?;
        //             } else {
        //                 insert_idx += 1;
        //             }
        //         }
        //     },
        //     QueryType::None => {
        //         let embed = CreateEmbed::default()
        //             .description(format!("{}", CrackedError::Other("No query provided!")))
        //             .footer(CreateEmbedFooter::new("No query provided!"));
        //         send_embed_response_poise(ctx, embed).await?;
        //         return Ok(false);
        //     },
        // }
    }

    pub async fn get_track_source_and_metadata(
        &self,
    ) -> CrackedResult<(SongbirdInput, Vec<MyAuxMetadata>)> {
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
                // video_info_to_source_and_metadata(client.clone(), query.clone()).await
                let mut ytdl = YoutubeDl::new(client, query.clone());
                let metadata = ytdl.aux_metadata().await?;
                let my_metadata = MyAuxMetadata::Data(metadata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::Keywords(query) => {
                tracing::warn!("In Keywords");
                // video_info_to_source_and_metadata(client.clone(), query.clone()).await
                let mut ytdl = YoutubeDl::new_search(client, query.clone());
                let metadata = ytdl.aux_metadata().await?;
                let my_metadata = MyAuxMetadata::Data(metadata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::File(file) => {
                tracing::warn!("In File");
                Ok((
                    HttpRequest::new(client, file.url.to_owned()).into(),
                    vec![MyAuxMetadata::default()],
                ))
            },
            QueryType::NewYoutubeDl(data) => {
                let (ytdl, aux_metadata) = data.clone();
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
                let metdata = match ytdl.aux_metadata().await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::error!("yt-dlp error: {}", e);
                        return Err(CrackedError::AudioStream(e));
                    },
                };
                let my_metadata = MyAuxMetadata::Data(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::None => unimplemented!(),
        }
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

/// Returns the QueryType for a given URL (or query string, or file attachment)
pub async fn query_type_from_url(
    ctx: Context<'_>,
    url: &str,
    file: Option<Attachment>,
) -> Result<Option<QueryType>, Error> {
    tracing::info!("url: {}", url);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let query_type = match Url::parse(url) {
        Ok(url_data) => match url_data.host_str() {
            Some("open.spotify.com") | Some("spotify.link") => {
                let final_url = http_utils::resolve_final_url(url).await?;
                tracing::info!(
                    "spotify: {} -> {}",
                    url.underline().blue(),
                    final_url.underline().bright_blue()
                );
                let spotify = SPOTIFY.lock().await;
                let spotify = verify(spotify.as_ref(), CrackedError::Other(SPOTIFY_AUTH_FAILED))?;
                Some(Spotify::extract(spotify, &final_url).await?)
            },
            Some("cdn.discordapp.com") => {
                tracing::info!("{}: {}", "attachement file".blue(), url.underline().blue());
                Some(QueryType::File(file.unwrap()))
            },
            Some("www.youtube.com") => {
                // Handle youtube playlist
                if url.contains("playlist") {
                    tracing::warn!("{}: {}", "youtube playlist".blue(), url.underline().blue());
                    Some(QueryType::PlaylistLink(url.to_string()))
                } else {
                    Some(QueryType::VideoLink(url.to_string()))
                }
            },
            // For all other domains fall back to yt-dlp.
            Some(other) => {
                tracing::warn!("query_type_from_url: domain: {other}, using yt-dlp");
                tracing::warn!(
                    "query_type_from_use: {}: {}",
                    "LINK".blue(),
                    url.underline().blue()
                );
                let mut ytdl = YoutubeDl::new(ctx.data().http_client.clone(), url.to_string());
                // This can fail whenever yt-dlp cannot parse a track from the URL.
                let metadata = match ytdl.aux_metadata().await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::error!("yt-dlp error: {}", e);
                        return Err(CrackedError::AudioStream(e).into());
                    },
                };
                Some(QueryType::NewYoutubeDl((ytdl, metadata)))
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
                }
            },
        },
        Err(e) => {
            tracing::error!("Url::parse error: {}", e);
            Some(QueryType::Keywords(url.to_string()))
        },
    };
    let guild_settings = ctx
        .data()
        .get_guild_settings(guild_id)
        .await
        .ok_or(CrackedError::NoGuildSettings)?;
    check_banned_domains(&guild_settings, query_type).map_err(Into::into)
}
