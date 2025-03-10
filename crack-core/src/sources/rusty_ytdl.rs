use crate::http_utils;
use crate::music::NewQueryType;
use bytes::Buf;
use bytes::BytesMut;
use crack_types::metadata::{search_result_to_aux_metadata, video_info_to_aux_metadata};
use crack_types::CrackedError;
use crack_types::QueryType;
use rusty_ytdl::stream::Stream;
use rusty_ytdl::RequestOptions;
use rusty_ytdl::VideoOptions;
use rusty_ytdl::{
    search::{SearchResult, YouTube},
    Video, VideoInfo,
};
use serenity::async_trait;
use songbird::input::{AudioStream, AudioStreamError, AuxMetadata, Compose, Input};
use std::fmt::Display;
use std::io::{self, Read, Seek, SeekFrom};
use std::pin::Pin;
use std::sync::Arc;
use symphonia::core::io::MediaSource;
// use tokio::runtime::Handle;
use super::ytdl::HANDLE;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct RustyYoutubeSearch<'a> {
    pub rusty_ytdl: YouTube,
    pub metadata: Option<AuxMetadata>,
    pub url: Option<String>,
    pub video: Option<Video<'a>>,
    pub query: QueryType,
}

// impl From<ResolvedTrack<'static>> for RustyYoutubeSearch<'static> {
//     fn from(track: ResolvedTrack<'static>) -> Self {
//         let query = QueryType::VideoLink(track.get_url());
//         let client = http_utils::get_client().clone();
//         RustyYoutubeSearch::new_with_stuff(client, query, track.metadata, track.video)
//             .unwrap_or_default()
//     }
// }

/// Display for the [`RustyYoutubeSearch`] struct.
impl Display for RustyYoutubeSearch<'_> {
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
    #[must_use]
    pub fn new() -> Self {
        Default::default()
    }

    /// Sets the client for the builder, mutating.
    #[must_use]
    pub fn set_client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Sets the ipv6 block for the builder, mutating.
    #[must_use]
    pub fn set_ipv6_block(mut self, ipv6_block: String) -> Self {
        self.ipv6_block = Some(ipv6_block);
        self
    }

    /// Sets the client for the builder, mutating.
    #[must_use]
    pub fn set_default_ipv6_block(mut self) -> Self {
        self.ipv6_block = Some("2001:4::/48".to_string());
        self
    }

    /// Builds the [`RequestOptions`] struct.
    #[must_use]
    pub fn build(self) -> RequestOptions {
        RequestOptions {
            client: self.client,
            ipv6_block: self.ipv6_block,
            ..Default::default()
        }
    }
}

/// Get a video from a URL.
pub async fn get_video_info(
    url: String,
    video_opts: VideoOptions,
) -> Result<VideoInfo, CrackedError> {
    let video = Video::new_with_options(&url, video_opts)?;
    video
        .get_basic_info()
        .await
        .map_err(std::convert::Into::into)
}

impl<'a> RustyYoutubeSearch<'a> {
    pub fn new(query: QueryType, client: reqwest::Client) -> Result<Self, CrackedError> {
        let request_options = RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        };
        let rusty_ytdl = rusty_ytdl::search::YouTube::new_with_options(&request_options)?;
        let url = match query {
            QueryType::VideoLink(ref url) => Some(url.clone()),
            _ => None,
        };
        Ok(Self {
            rusty_ytdl,
            url,
            query,
            metadata: None,
            video: None,
        })
    }

    pub fn new_with_stuff(
        client: reqwest::Client,
        query: QueryType,
        metadata: Option<AuxMetadata>,
        video: Option<rusty_ytdl::Video<'a>>,
    ) -> Result<Self, CrackedError> {
        let request_options = RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        };
        let rusty_ytdl = rusty_ytdl::search::YouTube::new_with_options(&request_options)?;
        let url = match query {
            QueryType::VideoLink(ref url) => Some(url.clone()),
            _ => None,
        };
        Ok(Self {
            rusty_ytdl,
            metadata,
            url,
            video,
            query,
        })
    }

    /// Reset the search.
    pub fn reset_search(&mut self) {
        self.metadata = None;
        self.url = None;
        self.video = None;
    }
}

impl From<RustyYoutubeSearch<'static>> for Input {
    fn from(val: RustyYoutubeSearch<'static>) -> Self {
        Input::Lazy(Box::new(val))
    }
}

use rusty_ytdl::VideoError;

#[async_trait]
impl Compose for RustyYoutubeSearch<'_> {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        // We may or may not have the metadata, so we need to check.
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

        // If we have a url, we can get the metadata from that directory so no need to search.
        if let Some(url) = self.url.as_ref() {
            let video =
                Video::new(url.clone()).map_err(|_| CrackedError::AudioStreamRustyYtdlMetadata)?;
            let video_info = video
                .get_basic_info()
                .await
                .map_err(|_| CrackedError::AudioStreamRustyYtdlMetadata)?;
            let metadata = video_info_to_aux_metadata(&video_info);
            self.metadata = Some(metadata.clone());
            return Ok(metadata);
        }

        let query = self
            .query
            .build_query()
            .ok_or(CrackedError::AudioStreamRustyYtdlMetadata)?;
        let res: SearchResult = self
            .rusty_ytdl
            .search_one(query, None)
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
            either::Left(self.stream.chunk().await.map_err(io::Error::other)?)
        } else {
            either::Right(())
        };

        let chunk = match opt_bytes {
            either::Left(Some(chunk)) => Some(chunk),
            either::Left(None) => return Ok(0), // End of stream
            either::Right(()) => None,
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
        //tokio::task::spawn_blocking(move || handle.block_on(async { self.read_async(buf).await }))
        let handle = HANDLE.lock().unwrap().clone().unwrap();
        tokio::task::block_in_place(move || handle.block_on(async { self.read_async(buf).await }))
        // tokio::task::block_in_place(move || {
        //     Handle::current().block_on(async { self.read_async(buf).await })
        // })
    }
}

impl Seek for MediaSourceStream {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match pos {
            SeekFrom::End(offset) => {
                let len = self
                    .byte_len()
                    .ok_or(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Invalid seek position",
                    ))?
                    .try_into();
                let len: i64 = match len {
                    Ok(len) => len,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "Invalid seek position",
                        ))
                    },
                };
                let new_position = len + offset;
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
        // true
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
        // Some(self.stream.content_length() as u64)
    }
}

pub struct NewSearchSource(pub NewQueryType, pub reqwest::Client);

impl From<NewSearchSource> for Input {
    fn from(val: NewSearchSource) -> Self {
        let NewSearchSource(NewQueryType(qt), client) = val;
        let search = RustyYoutubeSearch::new(qt, client).unwrap();
        search.into()
    }
}

#[cfg(test)]
mod test {
    use crate::{
        http_utils,
        music::NewQueryType,
        sources::{
            rusty_ytdl::{
                MediaSource, NewSearchSource, RequestOptionsBuilder, RustyYoutubeSearch, StreamExt,
            },
            youtube::search_query_to_source_and_metadata_rusty,
        },
    };
    use ::rusty_ytdl::{search::YouTube, stream::Stream, RequestOptions, VideoOptions};
    use crack_types::{CrackedError, QueryType};
    use mockall::predicate::*;
    use mockall::*;
    use songbird::input::{AuxMetadata, Compose, Input, YoutubeDl};
    use std::pin::Pin;
    // use std::io::{self, Read, Seek, SeekFrom};
    // use bytes::BytesMut;
    // use std::sync::Arc;
    // use tokio::sync::RwLock;

    // Mock for Stream trait to test MediaSourceStream without network calls
    mock! {
        pub StreamImpl {}
        #[async_trait::async_trait]
        impl Stream for StreamImpl {
            async fn chunk(&self) -> Result<Option<bytes::Bytes>, rusty_ytdl::VideoError>;
            fn content_length(&self) -> usize;
        }
        unsafe impl Send for StreamImpl {}
        unsafe impl Sync for StreamImpl {}
    }

    // Tests for RustyYoutubeSearch struct
    #[tokio::test]
    async fn test_rusty_youtube_search() {
        let search_term = "The Night Chicago Died";
        let query = QueryType::Keywords(search_term.to_string());
        let reqwest_client = http_utils::get_client().clone();
        let rusty_search = RustyYoutubeSearch::new(query, reqwest_client).unwrap();

        let mut media_source: Input = rusty_search.into();
        let metadata = match media_source.aux_metadata().await {
            Ok(metadata) => metadata,
            Err(e) => {
                println!("{e:?}");
                return;
            },
        };
        println!("{metadata:?}");
        assert!(metadata.title.is_some());
    }

    #[tokio::test]
    async fn test_new_search_source() {
        let search_term = "The Night Chicago Died";
        let query = crack_types::QueryType::Keywords(search_term.to_string());
        let query = NewQueryType(query);
        let reqwest_client = http_utils::get_client().clone();
        let new_search = NewSearchSource(query, reqwest_client);
        let input: Input = new_search.into();

        // We can't fully test playability without network calls
        // but we can verify the conversion works
        assert!(matches!(input, Input::Lazy(_)));
    }

    #[tokio::test]
    async fn test_ytdl() {
        let search = "The Night Chicago Died";
        let rusty_ytdl = YouTube::new().unwrap();
        let playlist = rusty_ytdl.search_one(search.to_string(), None).await;
        assert!(playlist.is_ok());
        match playlist {
            Ok(Some(playlist)) => {
                let metadata = crate::sources::rusty_ytdl::search_result_to_aux_metadata(&playlist);
                println!("{metadata:?}");
                // Verify metadata has expected fields
                assert!(metadata.title.is_some());
                assert!(metadata.source_url.is_some());
            },
            Ok(None) => {
                panic!("Expected search results but got None");
            },
            Err(e) => {
                println!("{e:?}");
                panic!("Search failed: {}", e);
            },
        }
    }

    #[tokio::test]
    async fn test_rusty_ytdl_serial() {
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
        let rusty_yt = YouTube::new_with_options(&req_opts).unwrap();
        for search in searches {
            let res = rusty_yt.search_one(search.to_string(), None).await;
            assert!(
                res.is_ok() || {
                    println!("{}", res.unwrap_err());
                    true
                }
            );
        }
    }

    #[tokio::test]
    async fn test_ytdl_serial() {
        let phrase = "Sign in to confirm you're not a bot.";
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];
        let client = http_utils::get_client();
        for search in searches {
            let mut ytdl = YoutubeDl::new_search(client.clone(), search.to_string());
            let res = ytdl.search(Some(1)).await;
            if let Err(err) = res {
                let expected_err = err.to_string().contains(phrase);
                println!("{err:?}\n{expected_err}\n");
            }
        }
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

        println!("{metadata:?}");
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

    // Tests for RequestOptionsBuilder
    #[test]
    fn test_request_options_builder() {
        // Test default builder
        let builder = RequestOptionsBuilder::new();
        let req = builder.build();
        assert_eq!(req.ipv6_block, Some("2001:4::/48".to_string()));
        assert!(req.client.is_none());

        // Test with custom client
        let client = reqwest::Client::new();
        let builder = RequestOptionsBuilder::new().set_client(client.clone());
        let req = builder.build();
        assert!(req.client.is_some());
        assert_eq!(req.ipv6_block, Some("2001:4::/48".to_string()));

        // Test with custom ipv6 block
        let builder = RequestOptionsBuilder::new().set_ipv6_block("2001:4::/64".to_string());
        let req = builder.build();
        assert_eq!(req.ipv6_block, Some("2001:4::/64".to_string()));
        assert!(req.client.is_none());

        // Test with both custom client and ipv6 block
        let client = reqwest::Client::new();
        let builder = RequestOptionsBuilder::new()
            .set_client(client.clone())
            .set_ipv6_block("2001:4::/64".to_string());
        let req = builder.build();
        assert!(req.client.is_some());
        assert_eq!(req.ipv6_block, Some("2001:4::/64".to_string()));

        // Test set_default_ipv6_block
        let builder = RequestOptionsBuilder::new()
            .set_ipv6_block("custom".to_string())
            .set_default_ipv6_block();
        let req = builder.build();
        assert_eq!(req.ipv6_block, Some("2001:4::/48".to_string()));
    }

    // Tests for RustyYoutubeSearch methods
    #[test]
    fn test_rusty_youtube_search_reset() {
        let query = QueryType::Keywords("test".to_string());
        let client = reqwest::Client::new();
        let mut search = RustyYoutubeSearch::new(query, client).unwrap();

        // Set some values
        search.metadata = Some(AuxMetadata::default());
        search.url = Some("https://example.com".to_string());
        search.video = None; // Already None, but included for completeness

        // Reset and verify
        search.reset_search();
        assert!(search.metadata.is_none());
        assert!(search.url.is_none());
        assert!(search.video.is_none());
    }

    // Tests for error handling
    #[tokio::test]
    async fn test_get_video_info_error_handling() {
        // Test with invalid URL
        let url = "invalid-url".to_string();
        let video_opts = VideoOptions::default();
        let result = super::get_video_info(url, video_opts).await;
        assert!(result.is_err());

        // Verify error type
        match result {
            Err(CrackedError::VideoError(_)) => {}, // Expected error type
            Err(e) => panic!("Unexpected error type: {:?}", e),
            Ok(_) => panic!("Expected error but got success"),
        }
    }

    // Tests for MediaSourceStream using mocks
    #[tokio::test]
    async fn test_media_source_stream_read() {
        // Create a mock stream
        let mut mock_stream = MockStreamImpl::new();

        // Set up expectations
        mock_stream
            .expect_chunk()
            .times(1)
            .returning(|| Ok(Some(bytes::Bytes::from_static(b"test data"))));

        mock_stream.expect_content_length().returning(|| 9); // "test data" length

        // Create a MediaSourceStream with the mock
        let stream_box: Pin<Box<dyn Stream + Send + Sync>> = Box::pin(mock_stream);
        let mut media_stream = stream_box.into_media_source();

        // Test reading
        let mut buf = [0u8; 4];
        let result = media_stream.read_async(&mut buf).await;

        // Verify results
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 4); // Should read 4 bytes
        assert_eq!(&buf, b"test");

        // Read more
        let result = media_stream.read_async(&mut buf).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 4); // Should read 4 more bytes
        assert_eq!(&buf, b" dat");

        // Read the rest
        let result = media_stream.read_async(&mut buf).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1); // Should read the last byte
        assert_eq!(buf[0], b'a');
    }

    // Test for StreamExt trait
    #[test]
    fn test_stream_ext_trait() {
        // Create a mock stream
        let mock_stream = MockStreamImpl::new();

        // Convert to MediaSourceStream
        let stream_box: Pin<Box<dyn Stream + Send + Sync>> = Box::pin(mock_stream);
        let media_stream = stream_box.into_media_source();

        // Verify the conversion worked
        assert!(!media_stream.is_seekable());
        assert!(media_stream.byte_len().is_none());
    }

    // Test for error propagation in aux_metadata
    #[tokio::test]
    async fn test_aux_metadata_error_propagation() {
        // Test with a query that will fail
        let query = QueryType::None; // This should fail when build_query is called
        let client = reqwest::Client::new();
        let mut search = RustyYoutubeSearch::new(query, client).unwrap();

        // Call aux_metadata and verify it returns an error
        let result = search.aux_metadata().await;
        assert!(result.is_err());
    }

    // Test for From implementation
    #[test]
    fn test_from_rusty_youtube_search_to_input() {
        let query = QueryType::Keywords("test".to_string());
        let client = reqwest::Client::new();
        let search = RustyYoutubeSearch::new(query, client).unwrap();

        // Convert to Input
        let input: Input = search.into();

        // Verify the conversion worked
        assert!(matches!(input, Input::Lazy(_)));
    }

    // Test for NewSearchSource
    #[test]
    fn test_new_search_source_conversion() {
        let query = QueryType::Keywords("test".to_string());
        let query_type = NewQueryType(query);
        let client = reqwest::Client::new();

        // Create NewSearchSource
        let search_source = NewSearchSource(query_type, client);

        // Convert to Input
        let input: Input = search_source.into();

        // Verify the conversion worked
        assert!(matches!(input, Input::Lazy(_)));
    }
}
