use crack_types::AuxMetadata;
use crack_types::Error;
use crack_types::QueryType;
use crack_types::SearchResult;
use once_cell::sync::Lazy;
use rusty_ytdl::{RequestOptions, VideoOptions};
use std::fmt::{self, Display, Formatter};
use thiserror::Error as ThisError;

pub use crack_types::{get_human_readable_timestamp, video_info_to_aux_metadata};
pub mod reply_handle_trait;
pub use reply_handle_trait::run as reply_handle_trait_run;

pub const NEW_FAILED: &str = "New failed";

static REQ_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    println!("Creating a new reqwest client...");
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .expect("Failed to build reqwest client")
});

static YOUTUBE_CLIENT: Lazy<rusty_ytdl::search::YouTube> = Lazy::new(|| {
    println!("Creating a new YouTube client...");
    let req_client = REQ_CLIENT.clone();
    let opts = RequestOptions {
        client: Some(req_client.clone()),
        ..Default::default()
    };
    rusty_ytdl::search::YouTube::new_with_options(&opts).expect("Failed to build YouTube client")
});

/// Custom error type for track resolve errors.
#[derive(ThisError, Debug)]
pub enum TrackResolveError {
    #[error("No track found")]
    NotFound,
    #[error("Error: {0}")]
    Other(String),
    #[error("Unknown resolve error")]
    Unknown,
}

/// Struct the holds a track who's had it's metadata queried,
/// and thus has a video URI associated with it, and it has all the
/// necessary metadata to be displayed in a music player interface.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ResolvedTrack {
    query: QueryType,
    metadata: Option<AuxMetadata>,
    video: Option<rusty_ytdl::Video>,
    details: Option<rusty_ytdl::VideoDetails>,
}

impl ResolvedTrack {
    /// Create a new ResolvedTrack
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            metadata: None,
            video: None,
            details: None,
        }
    }
}

/// Implement [Display] for [ResolvedTrack].
impl Display for ResolvedTrack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let metadata = self.metadata.as_ref().unwrap();
        let title = metadata.title.clone().unwrap_or_default();
        let url = metadata.source_url.clone().unwrap_or_default();
        let duration = get_human_readable_timestamp(metadata.duration);

        write!(f, "[{}]({}) • `{}`", title, url, duration)
    }
}

/// Client for resolving tracks, mostly holds other clients like reqwest and rusty_ytdl.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct CrackTrackClient {
    req_client: reqwest::Client,
    yt_client: rusty_ytdl::search::YouTube,
}

/// Implement [Default] for [CrackTrackClient].
impl Default for CrackTrackClient {
    fn default() -> Self {
        let req_client = reqwest::Client::new();
        let opts = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client: rusty_ytdl::search::YouTube::new_with_options(&opts).expect(NEW_FAILED),
        }
    }
}

/// Implement [CrackTrackClient].
impl CrackTrackClient {
    /// Create a new [CrackTrackClient].
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a new [CrackTrackClient] with a reqwest client and a rusty_ytdl client.
    pub fn new_with_clients(
        req_client: reqwest::Client,
        yt_client: rusty_ytdl::search::YouTube,
    ) -> Self {
        CrackTrackClient {
            req_client,
            yt_client,
        }
    }

    /// Create a new [CrackTrackClient] with a reqwest client.
    pub fn with_req_client(req_client: reqwest::Client) -> Self {
        let opts = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client: rusty_ytdl::search::YouTube::new_with_options(&opts).expect(NEW_FAILED),
        }
    }

    /// Resolve a track from a query. This does not start or ready the track for playback.
    pub async fn resolve_track(self, query: QueryType) -> Result<ResolvedTrack, Error> {
        let (video, details, metadata) = match query {
            QueryType::VideoLink(ref url) => {
                let request_options = RequestOptions {
                    client: Some(self.req_client.clone()),
                    ..Default::default()
                };
                let video_options = VideoOptions {
                    request_options: request_options.clone(),
                    ..Default::default()
                };
                let video = rusty_ytdl::Video::new_with_options(url, video_options)?;
                let info = video.get_info().await?;
                let metadata = video_info_to_aux_metadata(&info);

                (video, info.video_details, metadata)
            },
            _ => unimplemented!(),
        };
        let track = ResolvedTrack {
            query,
            metadata: Some(metadata),
            video: Some(video),
            details: Some(details),
        };
        Ok(track)
    }

    pub async fn resolve_search_one(self, query: &str) -> Result<ResolvedTrack, Error> {
        // let search_options = rusty_ytdl::search::SearchOptions {
        //     client: Some(self.req_client.clone()),
        //     ..Default::default()
        // };
        let search_results = self.yt_client.search_one(query, None).await?;
        let video = match search_results {
            Some(SearchResult::Video(result)) => result,
            _ => return Err(TrackResolveError::NotFound.into()),
        };
        let video_url = video.url.clone();
        let query = QueryType::VideoLink(video_url);
        self.resolve_track(query).await
    }

    /// Get a suggestion from a query. Passthrough to [rusty_ytdl::search::YouTube::suggestion].
    pub async fn suggestion(self, query: &str) -> Result<Vec<String>, Error> {
        self.yt_client
            .suggestion(query, None)
            .await
            .map_err(Into::into)
            .map(|res| res.into_iter().map(|x| x.replace("\"", "")).collect())
    }
}

/// Get a suggestion from a query. Passthrough to [rusty_ytdl::search::YouTube::suggestion].
pub async fn suggestion(query: &str) -> Result<Vec<String>, Error> {
    let client = YOUTUBE_CLIENT.clone();
    client
        .suggestion(query, None)
        .await
        .map_err(Into::into)
        .map(|res| res.into_iter().map(|x| x.replace("\"", "")).collect())
}

use clap::Parser;

/// Args struct for the CLI.
#[derive(Parser, Debug)]
#[command(
    name = "rusty_suggest",
    version = "1.0",
    author = "Cycle Five <cycle.five@proton.me>",
    about = "A simple CLI to get autocomplete suggestions from YouTube."
)]
struct Args {
    /// The name of the person to greet
    query: String,

    /// Prints extra information
    #[arg(short, long)]
    verbose: bool,
}

/// run function.
pub async fn run() {
    let args: Args = Args::parse();
    let suggestions = suggestion(&args.query).await.expect("No results");
    for x in suggestions {
        println!("{}", x);
    }
    // let mut queue = VecDeque::new();
    // let track = ResolvedTrack {
    //     query: QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string()),
    //     metadata: None,
    //     video: None,
    //     details: None,
    // };

    // queue.push_back(track);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run() {
        run();
    }

    #[test]
    fn test_new() {
        let track = ResolvedTrack::new(QueryType::VideoLink(
            "https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string(),
        ));
        assert_eq!(track.metadata, None);
        assert_eq!(track.video, None);
    }

    #[tokio::test]
    async fn test_resolve_track() {
        let query = QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string());
        let client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
        };

        let resolved = client.resolve_track(query).await.unwrap();

        let res = resolved.metadata.unwrap();
        assert_eq!(res.title.unwrap(), r#"Molly Nilsson "1995""#.to_string());
    }

    #[tokio::test]
    async fn test_suggestion() {
        let client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
        };

        let res = client.suggestion("molly nilsson").await;
        let res = res.expect("No results");
        let raw_res_want = vec![
            "molly nilsson",
            "molly nilsson tour",
            "molly nilsson i hope you die lyrics",
            "molly nilsson hey moon",
            "molly nilsson rym",
            "molly nilsson bandcamp",
            "molly nilsson i hope you die",
            "molly nilsson songs",
            "molly nilsson excalibur",
            "molly nilsson instagram",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<String>>();

        assert_eq!(res, raw_res_want);
        assert_eq!(res.len(), 10);
    }

    #[tokio::test]
    async fn test_suggestion_function() {
        let res = suggestion("molly nilsson").await;
        let res = res.expect("No results");
        assert_eq!(res.len(), 10);
    }
}
