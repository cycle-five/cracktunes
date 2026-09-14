//! rusty_ytdl finds videos and reads their metadata; it never plays them.
//!
//! 🪤 The googlevideo URL rusty_ytdl streams from 403s (`c=ANDROID`), songbird
//! gets an empty stream, and symphonia reports "no suitable format reader
//! found". This module once made a rusty_ytdl search a songbird `Input`, and
//! `/play`'s fallback played through it until v0.12.1. That `Input` is gone,
//! and `clippy.toml` bans rusty_ytdl's `stream` methods: yt-dlp plays
//! everything (`queue::build_track`, `youtube::source_for_search_hit`).

use crate::errors::CrackedError;
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
    use crate::{http_utils, sources::youtube::search_query_to_source_and_metadata_rusty};
    use crack_types::QueryType;
    use rusty_ytdl::search::YouTube;
    use rusty_ytdl::RequestOptions;
    use songbird::input::YoutubeDl;

    // 🔑 THE `#[ignore]`d TESTS BELOW REACH LIVE YOUTUBE, so they answer a
    // question about YouTube's mood on a shared CI runner, not about this
    // code. Run them by hand when touching the search path:
    //
    //     cargo test -p crack-core --lib rusty_ytdl -- --ignored --nocapture
    //
    // `test_ytdl` was the one that could actually fail: it panics when a
    // search returns `Ok(None)` -- YouTube answering with zero results --
    // while treating a hard `Err` as acceptable. That inversion took the
    // whole build matrix down three times in one hour, and never once
    // pointed at a defect here.
    #[ignore = "hits live YouTube"]
    #[tokio::test]
    async fn test_ytdl() {
        let search = "The Night Chicago Died";
        let rusty_ytdl = YouTube::new().unwrap();
        let playlist = rusty_ytdl.search_one(search.to_string(), None).await;
        match playlist {
            Ok(Some(playlist)) => {
                let metadata = crack_types::metadata::search_result_to_aux_metadata(&playlist);
                println!("{:?}", metadata);
            },
            Ok(None) => panic!("search returned no result"),
            Err(e) => {
                println!("{:?}", e);
            },
        }
    }

    #[ignore = "hits live YouTube"]
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
        let rusty_yt = rusty_ytdl::search::YouTube::new_with_options(&req_opts).unwrap();
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

    #[ignore = "hits live YouTube"]
    #[tokio::test]
    async fn test_ytdl_serial() {
        let phrase = "Sign in to confirm you’re not a bot.";
        let searches = vec![
            "The Night Chicago Died",
            "The Devil Went Down to Georgia",
            "Hit That The Offspring",
            "Nightwish I Wish I had an Angel",
            "Oh Shit I'm Feeling It",
        ];
        let client = http_utils::get_client_old();
        for search in searches {
            let mut ytdl = YoutubeDl::new_search(client.clone(), search.to_string());
            let res = ytdl.search(Some(1)).await;
            if let Err(err) = res {
                let expected_err = err.to_string().contains(phrase);
                println!("{:?}\n{}\n", err, expected_err);
            }
        }
    }

    #[ignore]
    #[tokio::test]
    // Drives a bare songbird `Driver` with a raw input to test the source, not
    // the bot's queue.
    #[allow(clippy::disallowed_methods)]
    async fn test_rusty_ytdl_plays() {
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
