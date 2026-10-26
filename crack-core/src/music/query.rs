use crate::sources::youtube::search_query_to_source_and_metadata;
use crate::utils::MUSIC_SEARCH_SUFFIX;
use crate::{
    errors::CrackedError,
    http_utils,
    messaging::messages::{PLAY_ALL_FAILED, SPOTIFY_LOOKUP_FAILED},
    sources::sleevenote,
    CrackedResult, Error,
};
use colored::Colorize;
use crack_types::metadata::{search_result_to_aux_metadata, video_info_to_aux_metadata};
use crack_types::{ytdl_for_url, NewAuxMetadata, QueryType, SpotifyTrack};
use futures::future;
use itertools::Itertools;
use rusty_ytdl::search::{Playlist, SearchOptions, SearchType, YouTube};
use rusty_ytdl::{RequestOptions, Video, VideoOptions};
use songbird::input::{AuxMetadata, Compose as _, HttpRequest, Input as SongbirdInput, YoutubeDl};
use std::{
    ops::Deref,
    process::{Output, Stdio},
};
use tokio::process::Command;

pub use super::query_modes::query_type_from_url;

pub const PLAYLIST_SEARCH_LIMIT: u64 = 30;

/// Default cap on playlist entries pulled in when playing a playlist link;
/// `playlist_limit` in cracktunes.toml overrides it (see
/// `BotConfig::get_playlist_limit`).
///
/// For end-of-queue plays (`mode_end`, `resolve_playlist_limit`) pages are
/// read about 100 entries per request (~600 ms each), so the cap costs one
/// request per ~100 entries rather than one lookup per track. `/playnext`
/// (`mode_next`) still resolves every entry (`rusty_ytdl::Playlist::get`, then
/// `resolve_track_many`), so a higher cap costs more lookups there.
pub const DEFAULT_PLAYLIST_LIMIT: u64 = 300;

// #[derive(Clone, Debug)]
// /// Enum for type of possible queries we have to handle
// pub enum QueryType {
//     Keywords(String),
//     KeywordList(Vec<String>),
//     VideoLink(String),
//     SpotifyTracks(Vec<SpotifyTrack>),
//     PlaylistLink(String),
//     File(serenity::Attachment),
//     NewYoutubeDl((YoutubeDl<'static>, AuxMetadata)),
//     YoutubeSearch(String),
//     None,
// }

#[derive(Clone, Debug)]
pub struct NewQueryType(pub crack_types::QueryType);

/// Newtype for [`QueryType`] to implement From for [`QueryType`].
impl From<crack_types::QueryType> for NewQueryType {
    fn from(q: crack_types::QueryType) -> Self {
        NewQueryType(q)
    }
}

/// HACK
impl From<NewQueryType> for crack_types::QueryType {
    fn from(q: NewQueryType) -> Self {
        let NewQueryType(qt) = q;
        match qt {
            QueryType::Keywords(keywords) => crack_types::QueryType::Keywords(keywords),
            QueryType::KeywordList(keywords_list) => {
                crack_types::QueryType::KeywordList(keywords_list)
            },
            QueryType::VideoLink(url) => crack_types::QueryType::VideoLink(url),
            QueryType::SpotifyTracks(tracks) => crack_types::QueryType::SpotifyTracks(tracks),
            QueryType::PlaylistLink(url) => crack_types::QueryType::PlaylistLink(url),
            QueryType::SpotifyLink(url) => crack_types::QueryType::SpotifyLink(url),
            QueryType::File(file) => crack_types::QueryType::File(file),
            QueryType::NewYoutubeDl((src, metadata)) => {
                crack_types::QueryType::NewYoutubeDl((src, metadata))
            },
            QueryType::YoutubeSearch(query) => crack_types::QueryType::YoutubeSearch(query),
            QueryType::None => crack_types::QueryType::None,
        }
    }
}

/// A [`QueryType::SpotifyLink`] that reached playback without being resolved.
///
/// Resolution happens at the entry points -- [`query_type_from_url`] for
/// `/play`, `gp_submit` for the game -- because it needs an HTTP call to
/// sleevenote and a message written for the user, neither of which belongs
/// this far down. Holding one here means an entry point was added without
/// resolving, so it is logged as our bug rather than played as a URL that
/// cannot be played.
pub(crate) fn unresolved_spotify_link(url: &str) -> CrackedError {
    tracing::error!("spotify link reached playback unresolved: {url}");
    CrackedError::Other(SPOTIFY_LOOKUP_FAILED)
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

    pub fn iter(&self) -> std::slice::Iter<'_, QueryType> {
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
impl NewQueryType {
    /// Build a query string from the query type.
    pub fn build_query(&self) -> Option<String> {
        let base = self.build_query_base();
        base.map(|x| format!("{} {}", x, MUSIC_SEARCH_SUFFIX))
    }

    pub fn build_query_base(&self) -> Option<String> {
        let NewQueryType(qt) = self;
        match qt {
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
            QueryType::SpotifyLink(url) => Some(url.clone()),
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
        let NewQueryType(qt) = self;
        match qt {
            QueryType::YoutubeSearch(_) => Err(Box::new(CrackedError::Other(
                "Download not valid with search results.",
            ))),
            QueryType::SpotifyLink(_) => Err(Box::new(CrackedError::Other(
                "Download not valid with Spotify links.",
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
                    url.split('=').next_back().unwrap(),
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
                    url.split('=').next_back().unwrap(),
                    extension,
                );
                tracing::warn!("file_name: {}", file_name);
                let (output, _metadata) = download_file_ytdlp(url, mp3).await?;
                let status = output.status.success();
                Ok((status, file_name))
            },
            QueryType::Keywords(query) => {
                tracing::warn!("In Keywords");
                let mut ytdl =
                    YoutubeDl::new_search(http_utils::get_client_old().clone(), query.clone());
                let metadata = ytdl.aux_metadata().await?;
                let url = metadata.source_url.ok_or(CrackedError::EmptySearchResult)?;
                let (output, metadata) = download_file_ytdlp(&url, mp3).await?;

                let file_name = format!(
                    "{}/{} [{}].{}",
                    prefix,
                    metadata.title.unwrap(),
                    url.split('=').next_back().unwrap(),
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
                    url.split('=').next_back().unwrap(),
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
                let search = keywords_list
                    .first()
                    .ok_or(CrackedError::EmptySearchResult)?
                    .clone();
                let mut ytdl = YoutubeDl::new_search(http_utils::get_client_old().clone(), search);
                let metadata = ytdl.aux_metadata().await?;
                // Download the hit, not the search: `download_file_ytdlp` takes a link.
                let url = metadata
                    .source_url
                    .clone()
                    .ok_or(CrackedError::EmptySearchResult)?;
                let (output, _metadata) = download_file_ytdlp(&url, mp3).await?;
                let file_name = format!(
                    "{}/{} [{}].{}",
                    prefix,
                    metadata.title.unwrap(),
                    url.split('=').next_back().unwrap(),
                    extension,
                );
                let status = output.status.success();
                Ok((status, file_name))
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::warn!("In KeywordList");
                let mut ytdl = YoutubeDl::new_search(
                    http_utils::get_client_old().clone(),
                    keywords_list.join(" "),
                );
                tracing::warn!("ytdl: {:?}", ytdl);
                let metadata = ytdl.aux_metadata().await?;
                // Download the hit, not the search: `download_file_ytdlp` takes a link.
                let url = metadata
                    .source_url
                    .clone()
                    .ok_or(CrackedError::EmptySearchResult)?;
                let (output, _metadata) = download_file_ytdlp(&url, mp3).await?;
                let file_name = format!(
                    "{}/{} [{}].{}",
                    prefix,
                    metadata.title.unwrap(),
                    url.split('=').next_back().unwrap(),
                    extension,
                );
                let status = output.status.success();
                Ok((status, file_name))
            },
            QueryType::None => Err(Box::new(CrackedError::Other("No query provided!"))),
        }
    }

    pub async fn get_track_metadata(
        &self,
        ytclient: YouTube,
        reqclient: reqwest::Client,
    ) -> CrackedResult<Vec<NewAuxMetadata>> {
        let NewQueryType(qt) = self;
        match qt {
            QueryType::SpotifyLink(url) => Err(unresolved_spotify_link(url)),
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
        let NewQueryType(qt) = self;
        match qt {
            QueryType::SpotifyLink(url) => Err(unresolved_spotify_link(url)),
            QueryType::YoutubeSearch(query) => {
                tracing::error!("In YoutubeSearch");
                let ytdl = YoutubeDl::new_search(client_old, query.clone());
                Ok(ytdl.into())
            },
            QueryType::VideoLink(url) => {
                tracing::warn!("In VideoLink");
                let ytdl = ytdl_for_url(client_old, url)?;
                Ok(ytdl.into())
            },
            // 🔒 `new_search`, never `new`: songbird passes `new`'s string to
            // yt-dlp as a bare positional argument, so keywords starting with
            // `-` would be read as an option (see `canonical_youtube_playlist_url`).
            QueryType::Keywords(query) => {
                let ytdl = YoutubeDl::new_search(client_old, query.clone());
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
        let NewQueryType(qt) = self;
        match qt {
            QueryType::SpotifyLink(url) => Err(unresolved_spotify_link(url)),
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
                let mut ytdl = ytdl_for_url(client_old, url)?;
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
                // 🪤 This is `ready_query`'s fallback for when `ct_client` fails, so it
                // must not depend on rusty_ytdl alone: it asks yt-dlp when rusty_ytdl
                // can't answer. It once searched with rusty_ytdl only, and a rusty_ytdl
                // failure failed both paths.
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
                // yt-dlp plays it, never rusty_ytdl: see `source_for_search_hit`.
                let input = ytdl_for_url(client_old, url)?.into();
                Ok((input, metadata))
            },
            QueryType::SpotifyTracks(tracks) => {
                tracing::error!("In SpotifyTracks, this is broken");
                let keywords_list = tracks
                    .iter()
                    .map(|x| x.build_query())
                    .collect::<Vec<String>>();
                let search = keywords_list
                    .first()
                    .ok_or(CrackedError::EmptySearchResult)?
                    .clone();
                let mut ytdl = YoutubeDl::new_search(client_old.clone(), search);
                tracing::warn!("ytdl: {:?}", ytdl);
                let metdata = ytdl.aux_metadata().await.unwrap();
                let my_metadata = NewAuxMetadata(metdata);
                Ok((ytdl.into(), vec![my_metadata]))
            },
            QueryType::KeywordList(keywords_list) => {
                tracing::warn!("In KeywordList");
                let mut ytdl = YoutubeDl::new_search(client_old.clone(), keywords_list.join(" "));
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
    let metadata = ytdl_for_url(http_utils::get_client_old().clone(), url)?
        .aux_metadata()
        .await?;

    let args = [
        "--extract-audio",
        "--audio-format",
        "mp3",
        "--audio-quality",
        "0",
        // `--` so the URL can never be read as an option.
        "--",
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

    let metadata = ytdl_for_url(http_utils::get_client_old().clone(), url)?
        .aux_metadata()
        .await?;

    // `--` so the URL can never be read as an option.
    let child = Command::new("yt-dlp")
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    tracing::warn!("yt-dlp");

    let output = child.wait_with_output().await?;
    Ok((output, metadata))
}

/// Resolve a Spotify link into the keyword searches that will play it.
///
/// One song becomes [`QueryType::Keywords`] and several become a
/// [`QueryType::KeywordList`], which the queueing path already resolves
/// concurrently. The distinction is not cosmetic: the reply is built from the
/// query type, so a single track sent as a one-element list would be announced
/// as "added playlist to queue" instead of showing the track that was queued.
///
/// The counts are logged rather than dropped: a playlist that resolved 3 of 40
/// items is not the same event as one that simply has 3 songs.
///
/// A shortfall comes back beside the query rather than being logged and
/// forgotten. Since sleevenote serves partial listings, a link that used to
/// fail outright now succeeds quietly missing songs, and the room has no way to
/// know unless we say so.
pub(crate) async fn spotify_query(
    url: &str,
) -> Result<(QueryType, Option<ListingShortfall>), CrackedError> {
    let resolution = sleevenote::resolve_spotify(url).await?;
    tracing::info!(
        "{}: {} {} -> {} playable, {} unresolved, {} episode(s) skipped",
        "spotify".blue(),
        resolution.media_type.noun(),
        resolution.name.underline().bright_blue(),
        resolution.len(),
        resolution.unresolved,
        resolution.episodes_skipped,
    );
    let shortfall = ListingShortfall::of(&resolution);
    let mut queries = resolution.queries();
    if queries.len() == 1 {
        // `swap_remove` over `into_iter().next()` so the `Vec` is not rebuilt
        // for the common case of a single-track link.
        return Ok((QueryType::Keywords(queries.swap_remove(0)), shortfall));
    }
    Ok((QueryType::KeywordList(queries), shortfall))
}

/// A listing that came back short of what Spotify said it held.
///
/// Only constructed when there is something to report: a whole listing, or one
/// whose page declared no total, produces `None` rather than a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListingShortfall {
    /// Items sleevenote saw, playable or not.
    pub seen: u64,
    /// Items Spotify declared the listing held.
    pub declared: u32,
    /// Declared items sleevenote never saw. Always non-zero.
    pub missing: u64,
}

impl ListingShortfall {
    /// The shortfall worth telling a user about, if any.
    #[must_use]
    pub fn of(resolution: &sleevenote::SpotifyResolution) -> Option<Self> {
        match (resolution.shortfall(), resolution.declared) {
            (Some(missing), Some(declared)) if missing > 0 => Some(Self {
                seen: resolution.items_seen,
                declared,
                missing,
            }),
            _ => None,
        }
    }
}

/// A query built from a URL, with whatever the resolution wants said about it.
#[derive(Debug)]
pub struct ResolvedQuery {
    /// The query the queueing path will run.
    pub query: NewQueryType,
    /// Set only when a Spotify listing came back short. See [`ListingShortfall`].
    pub shortfall: Option<ListingShortfall>,
}

/// What `mode:all` (and `reverse`, `shuffle`) queues from a query.
#[derive(Debug)]
pub(crate) enum PlayAll<'q> {
    /// A video or playlist link, queued through yt-dlp.
    Link(&'q str),
    /// A list of searches, queued as one batch.
    Searches(Vec<QueryType>),
}

/// 🔑 Anything but a link or a list is an error, not `Ok(false)`. The error
/// takes `/play`'s search placeholder down and is the reply; `Ok(false)` let
/// `/play` go on to edit the placeholder into "Now playing" or "No tracks in
/// queue!" for a request that queued nothing, over the notice.
pub(crate) fn play_all_plan(qt: &QueryType) -> Result<PlayAll<'_>, CrackedError> {
    match qt {
        QueryType::VideoLink(url) | QueryType::PlaylistLink(url) => Ok(PlayAll::Link(url)),
        QueryType::KeywordList(keywords) => Ok(PlayAll::Searches(
            keywords
                .iter()
                .map(|x| QueryType::Keywords(x.clone()))
                .collect(),
        )),
        QueryType::SpotifyTracks(tracks) => Ok(PlayAll::Searches(
            tracks
                .iter()
                .map(|x| QueryType::Keywords(x.build_query()))
                .collect(),
        )),
        _ => Err(CrackedError::Other(PLAY_ALL_FAILED)),
    }
}

#[cfg(test)]
mod play_all_tests {
    use super::*;

    #[test]
    fn play_all_refuses_plain_keywords_with_the_notice_as_the_error() {
        let err = play_all_plan(&QueryType::Keywords("some song".to_owned())).unwrap_err();
        assert_eq!(
            err.to_string(),
            "⚠️ Cannot fetch playlist via keywords! Try passing this command an URL."
        );
    }

    #[test]
    fn play_all_queues_a_playlist_link_as_a_link() {
        let url = "https://www.youtube.com/playlist?list=PL0123456789";
        assert!(matches!(
            play_all_plan(&QueryType::PlaylistLink(url.to_owned())),
            Ok(PlayAll::Link(u)) if u == url
        ));
    }

    #[test]
    fn play_all_queues_a_keyword_list_as_searches_in_order() {
        let list = QueryType::KeywordList(vec!["one".to_owned(), "two".to_owned()]);
        let Ok(PlayAll::Searches(queries)) = play_all_plan(&list) else {
            panic!("a keyword list is searches");
        };
        let words: Vec<_> = queries
            .iter()
            .map(|q| match q {
                QueryType::Keywords(k) => k.as_str(),
                other => panic!("not a search: {other:?}"),
            })
            .collect();
        assert_eq!(words, ["one", "two"]);
    }
}
