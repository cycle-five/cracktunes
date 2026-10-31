//! rusty_ytdl finds videos and reads their metadata; it never plays them.
//!
//! 🪤 The googlevideo URL rusty_ytdl streams from 403s (`c=ANDROID`), songbird
//! gets an empty stream, and symphonia reports "no suitable format reader
//! found". This module once made a rusty_ytdl search a songbird `Input`, and
//! `/play`'s fallback played through it until v0.12.1. That `Input` is gone,
//! and `clippy.toml` bans rusty_ytdl's `stream` methods: yt-dlp plays
//! everything (`queue::build_track`, `youtube::source_for_search_hit`).

use crack_common::errors::CrackedError;
use crack_types::QueryType;
use rusty_ytdl::RequestOptions;
use rusty_ytdl::VideoOptions;
use rusty_ytdl::{search::YouTube, Video, VideoInfo};
use songbird::input::AuxMetadata;

#[derive(Clone, Debug)]
pub struct NewRustyRequest<'a> {
    // required in param
    pub query: QueryType,
    // optional in param
    pub url: Option<String>,
    // out params
    pub metadata: Option<AuxMetadata>,
    pub video: Option<Video<'a>>,
}

#[derive(Clone, Debug)]
pub struct NewRustyClient {
    pub req_client: reqwest::Client,
    pub rusty_ytdl: YouTube,
    pub req_opts: RequestOptions,
    pub vid_opts: VideoOptions,
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

/// Get a video from a URL.
pub async fn get_video_info(
    url: String,
    video_opts: VideoOptions,
) -> Result<VideoInfo, CrackedError> {
    let video = Video::new_with_options(&url, video_opts)?;
    video.get_basic_info().await.map_err(|e| e.into())
}

#[cfg(test)]
mod test {
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
