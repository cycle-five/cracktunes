use crate::http_utils;
use crate::{commands::play_utils::QueryType, errors::CrackedError};
use bytes::Buf;
use bytes::BytesMut;
use rusty_ytdl::stream::Stream;
use rusty_ytdl::RequestOptions;
use rusty_ytdl::VideoOptions;
use rusty_ytdl::{
    search::{Playlist, SearchResult, YouTube},
    Video, VideoInfo,
};
use serenity::async_trait;
use songbird::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, Input, YoutubeDl};
use std::io::{self, Read, Seek, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use std::{fmt::Display, time::Duration};
use symphonia::core::io::MediaSource;
use tokio::sync::RwLock;

use super::ytdl::HANDLE;
/// Hacky, why did I do this? `AsString`
pub trait AsString {
    fn as_string(&self) -> String;
}

/// Implement the `AsString` trait for the `SearchResult` enum.
impl AsString for SearchResult {
    fn as_string(&self) -> String {
        match self {
            SearchResult::Video(video) => video.title.clone(),
            SearchResult::Playlist(playlist) => playlist.name.clone(),
            SearchResult::Channel(channel) => channel.name.clone(),
        }
    }
}

/// Implement the `AsString` trait for the `VideoInfo` struct.
impl AsString for VideoInfo {
    fn as_string(&self) -> String {
        self.video_details.title.clone()
    }
}

/// Implement the `AsString` trait for the `Playlist` struct.
impl AsString for Playlist {
    fn as_string(&self) -> String {
        self.name.clone()
    }
}

/// Implement the `AsString` trait for the `YouTube` struct.
impl AsString for YouTube {
    fn as_string(&self) -> String {
        "YouTube".to_string()
    }
}

/// Implement the `AsString` trait for the `YoutubeDl` struct.
impl AsString for YoutubeDl {
    fn as_string(&self) -> String {
        "YoutubeDl".to_string()
    }
}

#[derive(Clone, Debug)]
pub struct RustyYoutubeSearch {
    pub rusty_ytdl: YouTube,
    pub metadata: Option<AuxMetadata>,
    pub url: Option<String>,
    pub video: Option<Video>,
    pub query: QueryType,
}

type RustyYoutube = YouTube;
type YtdlpYoutube = YoutubeDl;
type YouTubeClient = either::Either<RustyYoutube, YtdlpYoutube>;
/// More general struct to wrap the search instances. Name this better.
#[derive(Clone, Debug)]
pub struct FastYoutubeSearch {
    pub query: QueryType,
    pub reqwest_client: reqwest::Client,
    pub ytdl_client: YouTubeClient,
    pub url: Option<String>,
    pub metadata: Option<AuxMetadata>,
    pub video: Option<Arc<VideoInfo>>,
}

impl FastYoutubeSearch {
    pub fn new(query: QueryType, client: reqwest::Client) -> Result<Self, CrackedError> {
        let req_opts = RequestOptionsBuilder::new()
            .set_client(client.clone())
            .build();
        let ytdl_client = YouTube::new_with_options(&req_opts)?;
        Ok(Self {
            query,
            reqwest_client: client,
            ytdl_client: either::Left(ytdl_client),
            url: None,
            metadata: None,
            video: None,
        })
    }

    pub fn reset_search(&mut self) {
        self.metadata = None;
        self.url = None;
        self.video = None;
    }
}

impl Display for FastYoutubeSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"FastYT: Query: {:?}
            ytdl: {:?}"#,
            self.query.build_query(),
            either::for_both!(&self.ytdl_client, ytdl => ytdl.as_string()),
        )
    }
}

impl Display for RustyYoutubeSearch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RustyYoutubeSearch({:?}, {:?}, {:?})",
            self.rusty_ytdl, self.metadata, self.query
        )
    }
}

/// Builder for the [`RequestOptions`] struct.
pub struct RequestOptionsBuilder {
    pub client: Option<reqwest::Client>,
    pub ipv6_block: Option<String>,
}

/// Default for the [`RequestOptions`] struct.
impl Default for RequestOptionsBuilder {
    fn default() -> Self {
        Self {
            client: None,
            ipv6_block: Some("2001:4::/48".to_string()),
        }
    }
}

/// Implementation of the builder for the [`RequestOptions`] struct.
impl RequestOptionsBuilder {
    /// Creates a default builder.
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the client for the builder, mutating.
    pub fn set_client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Sets the ipv6 block for the builder, mutating.
    pub fn set_ipv6_block(mut self, ipv6_block: String) -> Self {
        self.ipv6_block = Some(ipv6_block);
        self
    }

    /// Sets the client for the builder, mutating.
    pub fn set_default_ipv6_block(mut self) -> Self {
        self.ipv6_block = Some("2001:4::/48".to_string());
        self
    }

    /// Builds the [`RequestOptions`] struct.
    pub fn build(self) -> RequestOptions {
        RequestOptions {
            client: self.client,
            ipv6_block: self.ipv6_block,
            ..Default::default()
        }
    }
}

/// Convert a `SearchResult` to `AuxMetadata`.
pub fn search_result_to_aux_metadata(res: &SearchResult) -> AuxMetadata {
    let mut metadata = AuxMetadata::default();
    match res.clone() {
        SearchResult::Video(video) => {
            metadata.track = Some(video.title.clone());
            metadata.artist = None;
            metadata.album = None;
            metadata.date = video.uploaded_at.clone();

            metadata.channels = Some(2);
            metadata.channel = Some(video.channel.name);
            metadata.duration = Some(Duration::from_millis(video.duration));
            metadata.sample_rate = Some(48000);
            metadata.source_url = Some(video.url);
            metadata.title = Some(video.title);
            metadata.thumbnail = Some(video.thumbnails.first().unwrap().url.clone());
        },
        SearchResult::Playlist(playlist) => {
            metadata.title = Some(playlist.name);
            metadata.source_url = Some(playlist.url);
            metadata.duration = None;
            metadata.thumbnail = Some(playlist.thumbnails.first().unwrap().url.clone());
        },
        _ => {},
    };
    metadata
}

pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
    let mut metadata = AuxMetadata::default();
    tracing::info!(
        "video_info_to_aux_metadata: {:?}",
        video.video_details.title
    );
    let details = &video.video_details;
    metadata.artist = None;
    metadata.album = None;
    metadata.date = Some(details.publish_date.clone());

    metadata.channels = Some(2);
    metadata.channel = Some(details.owner_channel_name.clone());
    metadata.duration = Some(Duration::from_secs(
        details.length_seconds.parse::<u64>().unwrap_or_default(),
    ));
    metadata.sample_rate = Some(48000);
    metadata.source_url = Some(details.video_url.clone());
    metadata.title = Some(details.title.clone());
    metadata.thumbnail = Some(details.thumbnails.first().unwrap().url.clone());

    metadata
}

/// Get a video from a URL.
pub async fn get_video_info(url: String) -> Result<VideoInfo, CrackedError> {
    // let vid_options = VideoOptions {
    //     request_options: RequestOptions {
    //         client: Some(self.client.clone()),
    //         ..Default::default()
    //     },
    //     ..Default::default()
    // };
    // let video = Video::new_with_options(&url, vid_options)?;
    let video = Video::new(&url)?;
    video.get_basic_info().await.map_err(|e| e.into())
}

impl RustyYoutubeSearch {
    pub fn new(query: QueryType, client: reqwest::Client) -> Result<Self, CrackedError> {
        let request_options = RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        };
        let rusty_ytdl = rusty_ytdl::search::YouTube::new_with_options(&request_options)?;
        Ok(Self {
            rusty_ytdl,
            metadata: None,
            query,
            url: None,
            video: None,
        })
    }

    /// Reset the search.
    pub fn reset_search(&mut self) {
        self.metadata = None;
        self.url = None;
        self.video = None;
    }
}

impl From<RustyYoutubeSearch> for Input {
    fn from(val: RustyYoutubeSearch) -> Self {
        Input::Lazy(Box::new(val))
    }
}

use rusty_ytdl::VideoError;

#[async_trait]
impl Compose for RustyYoutubeSearch {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        // let query_str = self
        //     .query
        //     .build_query()
        //     .unwrap_or("Rick Astley Never Gonna Give You Up".to_string());
        // let search_res = self
        //     .rusty_ytdl
        //     .search_one(query_str, None)
        //     .await
        //     .unwrap_or_default();
        // let search_res = search_res.ok_or_else(|| CrackedError::AudioStreamRustyYtdlMetadata)?;
        // let search_video = match search_res {
        //     SearchResult::Video(video) => video,
        //     SearchResult::Playlist(playlist) => {
        //         let video = playlist.videos.first().unwrap();
        //         video.clone()
        //     },
        //     _ => {
        //         return Err(Into::into(CrackedError::AudioStreamRustyYtdlMetadata));
        //     },
        // };
        if self.metadata.is_none() {
            self.aux_metadata().await?;
        }
        let vid_options = VideoOptions {
            request_options: RequestOptions {
                client: Some(http_utils::get_client().clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        let url = self.url.as_ref().unwrap();
        Video::new_with_options(url.clone(), vid_options)
            .map_err(CrackedError::from)?
            .stream()
            .await
            .map(|input| {
                // let stream = AsyncAdapterStream::new(input, 64 * 1024);
                let stream = Box::into_pin(input).into_media_source();

                AudioStream {
                    input: Box::new(stream) as Box<dyn MediaSource>,
                    hint: None,
                }
            })
            .map_err(|e| AudioStreamError::from(CrackedError::from(e)))
    }

    fn should_create_async(&self) -> bool {
        true
    }

    /// Returns, and caches if isn't already, the metadata for the search.
    async fn aux_metadata(&mut self) -> Result<AuxMetadata, AudioStreamError> {
        if let Some(meta) = self.metadata.as_ref() {
            return Ok(meta.clone());
        }

        let res: SearchResult = self
            .rusty_ytdl
            .search_one(self.query.build_query().unwrap(), None)
            .await
            .map_err(|e| {
                <CrackedError as Into<AudioStreamError>>::into(
                    <VideoError as Into<CrackedError>>::into(e),
                )
            })?
            .ok_or_else(|| AudioStreamError::from(CrackedError::AudioStreamRustyYtdlMetadata))?;
        let metadata = search_result_to_aux_metadata(&res);

        self.metadata = Some(metadata.clone());
        self.url = Some(metadata.source_url.clone().unwrap());

        Ok(metadata)

        // self.metadata
        //     .clone()
        //     .ok_or_else(|| AudioStreamError::from(CrackedError::AudioStreamRustyYtdlMetadata))
    }
}

pub trait StreamExt {
    fn into_media_source(self: Pin<Box<Self>>) -> MediaSourceStream;
}

impl StreamExt for dyn Stream + Sync + Send {
    fn into_media_source(self: Pin<Box<Self>>) -> MediaSourceStream
    where
        Self: Sync + Send + 'static,
    {
        MediaSourceStream {
            stream: self,
            buffer: Arc::new(RwLock::new(BytesMut::new())),
            position: Arc::new(RwLock::new(0)),
        }
    }
}

pub struct MediaSourceStream {
    stream: Pin<Box<dyn Stream + Sync + Send>>,
    buffer: Arc<RwLock<BytesMut>>,
    position: Arc<RwLock<u64>>,
}

impl MediaSourceStream {
    async fn read_async(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let opt_bytes = if self.buffer.read().await.is_empty() {
            either::Left(
                self.stream
                    .chunk()
                    .await
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?,
            )
        } else {
            either::Right(())
        };

        let chunk = match opt_bytes {
            either::Left(Some(chunk)) => Some(chunk),
            either::Left(None) => return Ok(0), // End of stream
            either::Right(_) => None,
        };

        let mut buffer = self.buffer.write().await;
        let mut position = self.position.write().await;

        if let Some(chunk) = chunk {
            buffer.extend_from_slice(&chunk);
        }

        let len = std::cmp::min(buf.len(), buffer.len());
        buf[..len].copy_from_slice(&buffer[..len]);
        buffer.advance(len);
        *position += len as u64;

        Ok(len)
    }
}

impl Read for MediaSourceStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // Get the current tokio runtime
        let handle = HANDLE.lock().unwrap().clone().unwrap();
        tokio::task::block_in_place(move || handle.block_on(async { self.read_async(buf).await }))
    }
}

impl Seek for MediaSourceStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::End(offset) => {
                let len = self.byte_len().ok_or(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid seek position",
                ))?;
                let new_position = len as i64 + offset;
                if new_position < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid seek position",
                    ));
                }
                let mut position = self.position.blocking_write();
                *position = new_position as u64;
                Ok(*position)
            },
            SeekFrom::Start(offset) => {
                let mut position = self.position.blocking_write();
                *position = offset;
                Ok(*position)
            },
            SeekFrom::Current(offset) => {
                let mut position = self.position.blocking_write();
                let new_position = (*position as i64) + offset;
                if new_position < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid seek position",
                    ));
                }
                *position = new_position as u64;
                Ok(*position)
            },
        }
    }
}

/// Implementation of [`MediaSource`] for the [`MediaSourceStream`] struct.
/// FIXME: Does this need to be seekable?
impl MediaSource for MediaSourceStream {
    fn is_seekable(&self) -> bool {
        true
        //false
    }

    fn byte_len(&self) -> Option<u64> {
        // None
        Some(self.stream.content_length() as u64)
        // Some(0)
    }
}

pub struct NewSearchSource(pub QueryType, pub reqwest::Client);

impl From<NewSearchSource> for Input {
    fn from(val: NewSearchSource) -> Self {
        let search = RustyYoutubeSearch::new(val.0, val.1).unwrap();
        search.into()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        commands::play_utils::QueryType,
        http_utils,
        sources::{
            rusty_ytdl::{NewSearchSource, RustyYoutubeSearch},
            youtube::search_query_to_source_and_metadata_rusty,
        },
    };
    use rusty_ytdl::search::YouTube;
    use rusty_ytdl::RequestOptions;
    use songbird::input::{Input, YoutubeDl};

    #[tokio::test]
    async fn test_rusty_youtube_search() {
        let search_term = "The Night Chicago Died";
        let query = QueryType::Keywords(search_term.to_string());
        let reqwest_client = http_utils::get_client().clone();
        let rusty_search = RustyYoutubeSearch::new(query, reqwest_client).unwrap();
        // let rusty_search = FastYoutubeSearch {
        //     reqwest_client,
        //     ytdl_client: either::Left(rusty_ytdl),
        //     metadata: None,
        //     url: None,
        //     video: None,
        // };

        let mut media_source: Input = rusty_search.into();
        let metadata = media_source.aux_metadata().await.unwrap();
        println!("{:?}", metadata);
        assert!(metadata.title.is_some());
    }

    #[test]
    fn test_new_search_source() {
        let search_term = "The Night Chicago Died";
        let query = QueryType::Keywords(search_term.to_string());
        let reqwest_client = http_utils::get_client().clone();
        let new_search = NewSearchSource(query, reqwest_client);
        let input: Input = new_search.into();
        println!("{:?}", input.live().is_none());
        assert!(!input.is_playable());
    }

    #[tokio::test]
    async fn test_ytdl() {
        let search = "The Night Chicago Died";
        let rusty_ytdl = YouTube::new().unwrap();
        let playlist = rusty_ytdl.search_one(search.to_string(), None).await;
        match playlist {
            Ok(Some(playlist)) => {
                let metadata = crate::sources::rusty_ytdl::search_result_to_aux_metadata(&playlist);
                println!("{:?}", metadata);
            },
            Ok(None) => {
                assert!(false)
            },
            Err(e) => {
                println!("{:?}", e);
                assert!(e.to_string().contains("Your IP is likely being blocked"))
            },
        }
    }

    #[tokio::test]
    async fn test_rusty_ytdl() {
        let searches = vec!["the night chicago died", "Oh Shit I'm Feeling It"];

        let rusty_ytdl = YouTube::new().unwrap();
        for search in searches {
            let res = rusty_ytdl.search_one(search.to_string(), None).await;
            println!("{res:?}");
            assert!(
                res.is_ok()
                    || res
                        .unwrap_err()
                        .to_string()
                        .contains("Your IP is likely being blocked")
            );
        }
    }

    #[tokio::test]
    async fn test_rusty_ytdl_serial() {
        // let url = "https://www.youtube.com/watch?v=6n3pFFPSlW4".to_string();
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];

        let client = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .cookie_store(true)
            .build()
            .unwrap();
        let req_opts = RequestOptions {
            client: Some(client),
            ..Default::default()
        };
        let rusty_yt = rusty_ytdl::search::YouTube::new_with_options(&req_opts).unwrap();
        for search in searches {
            let res = rusty_yt.search_one(search.to_string(), None).await;
            assert!(
                res.is_ok() || {
                    println!("{}", res.unwrap_err().to_string());
                    true
                }
            );
        }
    }

    #[tokio::test]
    async fn test_ytdl_serial() {
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];
        let mut res_all = Vec::with_capacity(searches.len());
        let client = http_utils::get_client_old();
        for search in searches {
            let mut ytdl = YoutubeDl::new_search(client.clone(), search.to_string());
            let res = ytdl.search(Some(1)).await;
            println!("{:?}", res);
            if res.is_err() {
                assert!(res
                    .as_ref()
                    .unwrap_err()
                    .to_string()
                    .contains("Your IP is likely being blocked by Youtube"))
            }
            res_all.push(res);
        }

        println!("{:?}", res_all);
    }

    #[ignore]
    #[tokio::test]
    async fn test_rusty_ytdl_plays() {
        use crate::sources::rusty_ytdl::QueryType;
        let client = http_utils::get_client().clone();
        let (input, metadata) = search_query_to_source_and_metadata_rusty(
            client,
            QueryType::Keywords("The Night Chicago Died".to_string()),
        )
        .await
        .unwrap();

        println!("{:?}", metadata);
        println!("{:?}", input.is_playable());

        let mut driver = songbird::driver::Driver::default();

        let handle = driver.play_input(input);

        let callback = handle.seek(std::time::Duration::from_secs(30));
        let res = callback.result().unwrap();

        assert_eq!(
            res,
            std::time::Duration::from_secs(30),
            "Seek timestamp is not 30 seconds",
        );
    }

    // #[tokio::test]
    // async fn test_can_play_ytdl() {
    //     let url = "https://www.youtube.com/watch?v=p-L0NpaErkk".to_string();
    // }

    // RequestOptionsBuilder tests
    #[test]
    fn test_request_options_builder() {
        let builder = crate::sources::rusty_ytdl::RequestOptionsBuilder::new();
        let req = builder.build();
        assert_eq!(req.ipv6_block, Some("2001:4::/48".to_string()));

        let client = reqwest::Client::new();
        let builder = crate::sources::rusty_ytdl::RequestOptionsBuilder::new()
            .set_client(client.clone())
            .set_ipv6_block("2001:4::/64".to_string());
        let req = builder.build();
        assert_eq!(req.ipv6_block, Some("2001:4::/64".to_string()));
    }
}
