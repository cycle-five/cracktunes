use super::queue::{queue_track_back, queue_track_front};
use super::{queue_keyword_list_back, queue_query_list_offset};
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::interface::create_search_response;
use crate::sources::rusty_ytdl::NewSearchSource;
use crate::utils::MUSIC_SEARCH_SUFFIX;
use crate::{
    commands::check_banned_domains,
    errors::{verify, CrackedError},
    http_utils,
    messaging::{
        interface::{send_no_query_provided, send_search_failed},
        message::CrackedMessage,
        messages::SPOTIFY_AUTH_FAILED,
    },
    sources::spotify::{Spotify, SPOTIFY},
    utils::{edit_response_poise, yt_search_select},
    Context, CrackedResult, Error,
};
use ::serenity::all::{Attachment, CreateAttachment, CreateMessage};
use ::serenity::small_fixed_array::FixedString;
use colored::Colorize;
use crack_types::metadata::{search_result_to_aux_metadata, video_info_to_aux_metadata};
use crack_types::{NewAuxMetadata, SpotifyTrack};
use futures::future;
use itertools::Itertools;
use poise::{serenity_prelude as serenity, ReplyHandle};
use rusty_ytdl::search::{Playlist, SearchOptions, SearchType, YouTube};
use rusty_ytdl::{RequestOptions, Video, VideoOptions};
use songbird::{
    input::{AuxMetadata, Compose as _, HttpRequest, Input as SongbirdInput, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::str::FromStr;
use std::{
    ops::Deref,
    path::Path,
    process::{Output, Stdio},
    sync::Arc,
};
use tokio::{process::Command, sync::Mutex};
use url::Url;

pub const PLAYLIST_SEARCH_LIMIT: u64 = 30;

#[derive(Clone, Debug)]
/// Enum for type of possible queries we have to handle
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(serenity::Attachment),
    NewYoutubeDl((YoutubeDl<'static>, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

/// HACK
impl From<QueryType> for crack_types::QueryType {
    fn from(q: QueryType) -> Self {
        match q {
            QueryType::Keywords(keywords) => crack_types::QueryType::Keywords(keywords),
            QueryType::KeywordList(keywords_list) => {
                crack_types::QueryType::KeywordList(keywords_list)
            },
            QueryType::VideoLink(url) => crack_types::QueryType::VideoLink(url),
            QueryType::SpotifyTracks(tracks) => crack_types::QueryType::SpotifyTracks(tracks),
            QueryType::PlaylistLink(url) => crack_types::QueryType::PlaylistLink(url),
            QueryType::File(file) => crack_types::QueryType::File(file),
            QueryType::NewYoutubeDl((src, metadata)) => {
                crack_types::QueryType::NewYoutubeDl((src, metadata))
            },
            QueryType::YoutubeSearch(query) => crack_types::QueryType::YoutubeSearch(query),
            QueryType::None => crack_types::QueryType::None,
        }
    }
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
use crate::sources::spotify::SpotifyTrackTrait;
use crate::sources::youtube::search_query_to_source_and_metadata;
impl QueryType {
    /// Build a query string from the query type.
    pub fn build_query(&self) -> Option<String> {
        let base = self.build_query_base();
        base.map(|x| format!("{} {}", x, MUSIC_SEARCH_SUFFIX))
    }

    pub fn build_query_base(&self) -> Option<String> {
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
            QueryType::PlaylistLink(url) => Some(url.to_string()),
            QueryType::File(file) => Some(file.url.to_string()),
            QueryType::NewYoutubeDl((_src, metadata)) => metadata.source_url.clone(),
            QueryType::YoutubeSearch(query) => Some(query.clone()),
            QueryType::None => None,
        }
    }

    /// Build a query string for a explicit result from the query type.
    pub fn build_query_explicit(&self, query: Option<String>) -> Option<String> {
        query.map(|x| format!("{} explicit", x))
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
        //let client = http_utils::get_client().clone();
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
                let mut ytdl = YoutubeDl::new(
                    http_utils::get_client_old().clone(),
                    format!("ytsearch:{}", query),
                );
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
                let mut ytdl = YoutubeDl::new(http_utils::get_client_old().clone(), url.clone());
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
                let mut ytdl = YoutubeDl::new(http_utils::get_client_old().clone(), url.clone());
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
                ctx.http(),
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
            _ => send_search_failed(&ctx).await.map(|_| vec![]),
        }
    }

    pub async fn mode_search_keywords(
        &self,
        ctx: Context<'_>,
        call: Arc<Mutex<Call>>,
        keywords: String,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        //let reqwest_client = ctx.data().http_client.clone();
        let search_results = YoutubeDl::new_search(http_utils::get_client_old().clone(), keywords)
            .search(None)
            .await?
            .collect::<Vec<_>>();

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
        search_reply: ReplyHandle<'_>,
    ) -> Result<bool, CrackedError> {
        let search_msg = &mut search_reply.into_message().await?;
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
                let playlist: Playlist =
                    rusty_ytdl::search::Playlist::get(url.clone(), None).await?;
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

    pub async fn mode_end<'ctx>(
        &self,
        ctx: Context<'ctx>,
        call: Arc<Mutex<Call>>,
        search_reply: ReplyHandle<'ctx>,
    ) -> Result<bool, CrackedError> {
        let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
        let search_msg = &mut search_reply.clone().into_message().await?;
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::trace!("Mode::End, QueryType::YoutubeSearch");

                let res =
                    YoutubeDl::new_search(http_utils::get_client_old().clone(), query.clone())
                        .search(None)
                        .await?
                        .collect()
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
                        return Err(e);
                    },
                };
                Ok(true)
            },
            // FIXME
            QueryType::PlaylistLink(url) => {
                tracing::trace!("Mode::End, QueryType::PlaylistLink");
                // Let's use the new YouTube rust library for this
                let playlist: Playlist =
                    rusty_ytdl::search::Playlist::get(url.clone(), None).await?;
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
        search_reply: ReplyHandle<'_>,
    ) -> Result<bool, CrackedError> {
        let search_msg = &mut search_reply.into_message().await?;
        match self {
            QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => {
                // FIXME
                let mut src = YoutubeDl::new(http_utils::get_client_old().clone(), url.clone());
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
    }

    pub fn get_query_source(&self, client: reqwest::Client) -> songbird::input::Input {
        NewSearchSource(self.clone(), client).into()
    }

    pub async fn get_track_metadata(
        &self,
        ytclient: YouTube,
        reqclient: reqwest::Client,
    ) -> CrackedResult<Vec<NewAuxMetadata>> {
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let search_options = SearchOptions {
                    limit: 5,
                    search_type: SearchType::All,
                    ..Default::default()
                };

                let search_results = ytclient.search(query, Some(&search_options)).await?;
                Ok(search_results
                    .into_iter()
                    .map(NewAuxMetadata::from)
                    .collect_vec())
            },
            QueryType::VideoLink(query) => {
                let video_options = VideoOptions {
                    request_options: RequestOptions {
                        client: Some(reqclient.clone()),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let video = Video::new_with_options(query.clone(), video_options)?;
                let video_info = video.get_info().await?;
                let metadata = video_info_to_aux_metadata(&video_info);

                Ok(vec![NewAuxMetadata(metadata)])
            },
            QueryType::Keywords(query) => ytclient
                .search_one(query.clone(), None)
                .await?
                .map(NewAuxMetadata::from)
                .map(|metadata| vec![metadata])
                .ok_or(CrackedError::Other("No search results found!")),
            QueryType::File(_file) => {
                // FIXME: Maybe try to parse some metadata from the file?
                Ok(vec![NewAuxMetadata::default()])
            },
            QueryType::NewYoutubeDl(_data) => {
                // FIXME: Maybe just throw an error? This doesn't really make since because it's a different yt client...
                Ok(vec![])
            },
            QueryType::PlaylistLink(url) => {
                // FIXME: What limit should we use?
                let search_options = SearchOptions {
                    limit: 30,
                    search_type: SearchType::Playlist,
                    ..Default::default()
                };

                let search_results = ytclient.search(url, Some(&search_options)).await?;
                Ok(search_results
                    .into_iter()
                    .map(NewAuxMetadata::from)
                    .collect_vec())
            },
            QueryType::SpotifyTracks(tracks) => {
                let keywords_list = tracks
                    .iter()
                    .map(|x| ytclient.search_one(x.build_query(), None));
                let metadatas = future::join_all(keywords_list)
                    .await
                    .into_iter()
                    .filter_map_ok(|x| x.map(NewAuxMetadata::from))
                    .flatten()
                    .collect_vec();
                Ok(metadatas)
            },
            QueryType::KeywordList(keywords_list) => {
                let mut metadatas = Vec::with_capacity(keywords_list.len());
                for keyword in keywords_list {
                    let res = ytclient.search_one(keyword, None).await?.unwrap();
                    let my_metadata = search_result_to_aux_metadata(&res);
                    let my_metadata = NewAuxMetadata(my_metadata);
                    metadatas.push(my_metadata);
                }
                Ok(metadatas)
            },
            QueryType::None => unimplemented!(),
        }
    }

    /// Get the source (playable track) for this query.
    pub async fn get_track_source(
        &self,
        client_old: reqwest::Client,
    ) -> CrackedResult<songbird::input::Input> {
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let ytdl = YoutubeDl::new_search(client_old, query.clone());
                Ok(ytdl.into())
            },
            QueryType::VideoLink(url) => {
                tracing::warn!("In VideoLink");
                let ytdl = YoutubeDl::new(client_old, url.clone());
                Ok(ytdl.into())
            },
            QueryType::Keywords(query) => {
                let ytdl = YoutubeDl::new(client_old, query.clone());
                Ok(ytdl.into())
            },
            QueryType::File(file) => Ok(HttpRequest::new(client_old, file.url.to_string()).into()),
            QueryType::NewYoutubeDl(data) => Ok(data.clone().0.into()),
            QueryType::PlaylistLink(_)
            | QueryType::SpotifyTracks(_)
            | QueryType::KeywordList(_)
            | QueryType::None => unimplemented!(),
        }
    }

    /// Get the source and metadata for a given query type. This is the expensive part.
    pub async fn get_track_source_and_metadata(
        &self,
        client: Option<reqwest::Client>,
    ) -> CrackedResult<(SongbirdInput, Vec<NewAuxMetadata>)> {
        use colored::Colorize;
        let client = client.unwrap_or_else(|| http_utils::get_client().clone());
        let client_old = http_utils::get_client_old().clone();
        tracing::warn!("{}", format!("query_type: {:?}", self).red());
        match self {
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let mut ytdl = YoutubeDl::new_search(client_old, query.clone());
                let mut res = Vec::new();
                let asdf = ytdl.search(None).await?;
                for metadata in asdf {
                    let my_metadata = NewAuxMetadata(metadata);
                    res.push(my_metadata);
                }
                Ok((ytdl.into(), res))
            },
            QueryType::VideoLink(url) => {
                tracing::warn!("In VideoLink");
                let mut ytdl = YoutubeDl::new(client_old, url.clone());
                let metadata = ytdl.aux_metadata().await?;
                let input = ytdl.into();
                // let client = crate::http_utils::get_client();
                // let search =
                //     crate::sources::youtube::get_rusty_search(client.clone(), query.clone())
                //         .await?;
                // search.
                // This call, this is what does all the work
                // let mut input = self.get_query_source(client.clone());
                // let metadata = input
                //     .aux_metadata()
                //     .await
                //     .map_err(CrackedError::AuxMetadataError)?;
                let my_metadata = NewAuxMetadata(metadata);
                Ok((input, vec![my_metadata]))
            },
            QueryType::Keywords(query) => {
                tracing::warn!("In Keywords");
                //get_rusty_search(client.clone(), query.clone()).await
                let (input, metadata) =
                    search_query_to_source_and_metadata(client.clone(), query.clone()).await?;
                Ok((input, metadata))
                // let mut ytdl = YoutubeDl::new_search(client, query.clone());
                // let metadata = ytdl.aux_metadata().await?;
                // let my_metadata = NewAuxMetadata(metadata);
                // Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::File(file) => {
                tracing::warn!("In File");
                Ok((
                    HttpRequest::new(client_old, file.url.to_string()).into(),
                    vec![NewAuxMetadata::default()],
                ))
            },
            QueryType::NewYoutubeDl(data) => {
                let (ytdl, aux_metadata) = data.clone();
                Ok((ytdl.into(), vec![NewAuxMetadata(aux_metadata)]))
            },
            QueryType::PlaylistLink(url) => {
                tracing::warn!("In PlaylistLink");
                let req_options = RequestOptions {
                    client: Some(client.clone()),
                    ..Default::default()
                };
                let rusty_ytdl = YouTube::new_with_options(&req_options)?;
                let search_options = SearchOptions {
                    limit: PLAYLIST_SEARCH_LIMIT,
                    search_type: SearchType::Playlist,
                    ..Default::default()
                };

                let res = rusty_ytdl.search(url, Some(&search_options)).await?;
                let mut metadata = Vec::with_capacity(res.len());
                for r in res {
                    metadata.push(NewAuxMetadata(search_result_to_aux_metadata(&r)));
                }
                let input = self.get_query_source(client.clone());
                Ok((input, metadata))
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::error!("In SpotifyTracks, this is broken");
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                let mut ytdl = YoutubeDl::new(
                    client_old.clone(),
                    format!("ytsearch:{}", keywords_list.first().unwrap()),
                );
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = NewAuxMetadata(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::warn!("In KeywordList");
                let mut ytdl = YoutubeDl::new(
                    client_old.clone(),
                    format!("ytsearch:{}", keywords_list.join(" ")),
                );
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = match ytdl.aux_metadata().await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::error!("yt-dlp error: {}", e);
                        return Err(CrackedError::AudioStream(e));
                    },
                };
                let my_metadata = NewAuxMetadata(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::None => unimplemented!(),
        }
    }
}

/// Download a file and upload it as an mp3.
async fn download_file_ytdlp_mp3(url: &str) -> Result<(Output, AuxMetadata), Error> {
    let metadata = YoutubeDl::new(http_utils::get_client_old().clone(), url.to_string())
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

    let metadata = YoutubeDl::new(http_utils::get_client_old().clone(), url.to_string())
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
                // We don't want to give up as long as we have a url.
                let final_url = http_utils::resolve_final_url(url)
                    .await
                    .unwrap_or(url.to_string());
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
                let opt_query = url_data
                    .query_pairs()
                    .filter_map(|(key, value)| {
                        if key == "list" || key == "playlist" {
                            tracing::warn!(
                                "{}: {}",
                                "youtube playlist".blue(),
                                url.underline().blue()
                            );
                            Some(QueryType::PlaylistLink(value.to_string()))
                        } else {
                            None
                        }
                    })
                    .next();
                match opt_query {
                    Some(query) => Some(query),
                    None => {
                        tracing::warn!("{}: {}", "youtube video".blue(), url.underline().blue());
                        Some(QueryType::VideoLink(url.to_string()))
                    },
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
                let mut ytdl =
                    YoutubeDl::new(http_utils::get_client_old().clone(), url.to_string());
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
