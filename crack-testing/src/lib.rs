pub mod queue;
pub use queue::*;

use crack_types::TrackResolveError;
use crack_types::{get_human_readable_timestamp, parse_url, video_info_to_aux_metadata};
use crack_types::{AuxMetadata, Error, QueryType, SearchResult};

use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use rusty_ytdl::{search, search::YouTube};
use rusty_ytdl::{RequestOptions, VideoDetails, VideoOptions};

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

pub const NEW_FAILED: &str = "New failed";
pub const DEFAULT_PLAYLIST_LIMIT: u64 = 50;

static REQ_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    println!("Creating a new reqwest client...");
    build_configured_reqwest_client()
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

/// Build a configured reqwest client for use in the CrackTrackClient.
pub fn build_configured_reqwest_client() -> reqwest::Client {
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .expect("Failed to build reqwest client")
}

/// Struct the holds a track who's had it's metadata queried,
/// and thus has a video URI associated with it, and it has all the
/// necessary metadata to be displayed in a music player interface.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct ResolvedTrack {
    query: QueryType,
    queued: bool,
    metadata: Option<AuxMetadata>,
    video: Option<rusty_ytdl::Video>,
    search_video: Option<rusty_ytdl::search::Video>,
    details: Option<rusty_ytdl::VideoDetails>,
}

impl ResolvedTrack {
    /// Create a new ResolvedTrack
    pub fn new(query: QueryType) -> Self {
        ResolvedTrack {
            query,
            ..Default::default()
        }
    }

    /// Get the title of the track.
    pub fn get_title(&self) -> String {
        if let Some(search_video) = &self.search_video {
            return search_video.title.clone();
        } else if let Some(metadata) = &self.metadata {
            return metadata.title.clone().unwrap_or_default();
        } else if let Some(details) = &self.details {
            return details.title.clone();
        } else {
            return "UNKNOWN_TITLE".to_string();
        }
    }

    /// Get the URL of the track.
    pub fn get_url(&self) -> String {
        if let Some(search_video) = &self.search_video {
            return search_video.url.clone();
        } else if let Some(metadata) = &self.metadata {
            return metadata.source_url.clone().unwrap_or_default();
        } else if let Some(details) = &self.details {
            return details.video_url.clone();
        } else {
            return "UNKNOWN_URL".to_string();
        }
    }

    /// Get the duration of the track.
    pub fn get_duration(&self) -> String {
        if let Some(metadata) = &self.metadata {
            return get_human_readable_timestamp(metadata.duration);
        } else if let Some(details) = &self.details {
            let duration =
                Duration::from_secs(details.length_seconds.parse::<u64>().unwrap_or_default());
            return get_human_readable_timestamp(Some(duration));
        } else if let Some(search_video) = &self.search_video {
            let duration = Duration::from_millis(search_video.duration);
            return get_human_readable_timestamp(Some(duration));
        } else {
            return "UNKNOWN_DURATION".to_string();
        }
    }
}

/// Implement [From] for [search::Video] to [ResolvedTrack].
impl From<search::Video> for ResolvedTrack {
    fn from(video: search::Video) -> Self {
        ResolvedTrack {
            query: QueryType::VideoLink(video.url.clone()),
            search_video: Some(video),
            ..Default::default()
        }
    }
}

/// Implement [From] for ([rusty_ytdl::Video], VideoDetails, AuxMetadata) to [ResolvedTrack].
impl From<(rusty_ytdl::Video, VideoDetails, AuxMetadata)> for ResolvedTrack {
    fn from(
        (video, video_details, aux_metadata): (rusty_ytdl::Video, VideoDetails, AuxMetadata),
    ) -> Self {
        ResolvedTrack {
            query: QueryType::VideoLink(video.get_video_url()),
            video: Some(video),
            metadata: Some(aux_metadata),
            details: Some(video_details),
            ..Default::default()
        }
    }
}

/// Implement [Display] for [ResolvedTrack].
impl Display for ResolvedTrack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let title = self.get_title();
        let url = self.get_url();
        let duration = self.get_duration();

        let url = if url.contains("youtube.com") {
            url
        } else {
            format!("https://www.youtube.com/watch?v={}", url)
        };
        write!(f, "[{}]({}) • `{}`", title, url, duration)
    }
}

/// Client for resolving tracks, mostly holds other clients like reqwest and rusty_ytdl.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct CrackTrackClient {
    req_client: reqwest::Client,
    yt_client: rusty_ytdl::search::YouTube,
    video_opts: VideoOptions,
    q: CrackTrackQueue,
}

/// Implement [Default] for [CrackTrackClient].
impl Default for CrackTrackClient {
    fn default() -> Self {
        let req_client = REQ_CLIENT.clone();
        let yt_client = YOUTUBE_CLIENT.clone();
        let req_options = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        let video_opts = VideoOptions {
            request_options: req_options.clone(),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client,
            video_opts,
            q: CrackTrackQueue::new(),
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
        let req_options = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        let video_opts = VideoOptions {
            request_options: req_options.clone(),
            ..Default::default()
        };
        CrackTrackClient {
            req_client,
            yt_client,
            video_opts,
            q: CrackTrackQueue::new(),
        }
    }

    /// Create a new [CrackTrackClient] with a reqwest client.
    pub fn with_req_client(req_client: reqwest::Client) -> Self {
        let opts = RequestOptions {
            client: Some(req_client.clone()),
            ..Default::default()
        };
        let video_opts = VideoOptions {
            request_options: opts.clone(),
            ..Default::default()
        };
        let yt_client = rusty_ytdl::search::YouTube::new_with_options(&opts).expect(NEW_FAILED);

        CrackTrackClient {
            req_client,
            yt_client,
            video_opts,
            q: CrackTrackQueue::new(),
        }
    }

    /// Resolve a track from a query. This does not start or ready the track for playback.
    pub async fn resolve_track(&self, query: QueryType) -> Result<ResolvedTrack, Error> {
        // Do we need the original query in the resolved track?
        let vid_tuple: (rusty_ytdl::Video, VideoDetails, AuxMetadata) = match query {
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
        Ok(vid_tuple.into())
    }

    pub async fn resolve_search_one(&self, query: &str) -> Result<ResolvedTrack, Error> {
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

    pub async fn resolve_search(&self, query: &str) -> Result<VecDeque<ResolvedTrack>, Error> {
        let search_results = self.yt_client.search(query, None).await?;
        let mut queue = VecDeque::new();
        for result in search_results {
            let video = match result {
                SearchResult::Video(video) => video,
                _ => continue,
            };
            let video_url = video.url.clone();
            let query = QueryType::VideoLink(video_url);
            let track = self.resolve_track(query).await?;
            queue.push_back(track);
        }
        Ok(queue)
    }

    /// Resolve a playlist from a URL. Limit is set to 50 by default.
    pub async fn resolve_playlist(&self, url: &str) -> Result<VecDeque<ResolvedTrack>, Error> {
        self.resolve_playlist_limit(url, DEFAULT_PLAYLIST_LIMIT)
            .await
    }

    /// Resolve a playlist from a URL. Limit must be given, this is intended to be used primarily by
    /// a helper method in the CrackTrackClient.
    pub async fn resolve_playlist_limit(
        &self,
        url: &str,
        limit: u64,
    ) -> Result<VecDeque<ResolvedTrack>, Error> {
        let req_options = RequestOptions {
            client: Some(self.req_client.clone()),
            ..Default::default()
        };
        let search_options = rusty_ytdl::search::PlaylistSearchOptions {
            limit,
            request_options: Some(req_options),
            ..Default::default()
        };
        let search_options = Some(&search_options);
        let res = rusty_ytdl::search::Playlist::get(url, search_options).await?;

        let mut queue = VecDeque::new();

        for video in res.videos {
            let track = ResolvedTrack {
                query: QueryType::VideoLink(video.url.clone()),
                search_video: Some(video),
                ..Default::default()
            };
            println!("Resolved: {}", track);
            queue.push_back(track);
        }
        Ok(queue)
    }

    /// Get a suggestion from a query. Passthrough to [rusty_ytdl::search::YouTube::suggestion].
    pub async fn suggestion(&self, query: &str) -> Result<Vec<String>, Error> {
        suggestion_yt(self.yt_client.clone(), query).await
    }

    /// Resolve a track from a query and enqueue it.
    pub async fn enqueue_query(&mut self, query: QueryType) -> Result<(), Error> {
        let track = self.clone().resolve_track(query).await?;
        let _ = self.q.push_back(track).await;
        Ok(())
    }

    /// Enqueue a track internally.
    pub async fn enqueue_track(&mut self, track: ResolvedTrack) -> Result<(), Error> {
        let _ = self.q.push_back(track).await;
        Ok(())
    }

    /// Build the display string for the queue.
    /// This is separate because it needs to be used non-async,
    /// but must be created async.
    pub async fn build_display(&mut self) -> Result<(), Error> {
        self.q.build_display().await
    }

    /// Get the display string for the queue.
    pub fn get_display(&self) -> String {
        self.q.get_display()
    }

    /// Get the queue.
    pub async fn get_queue(&self) -> VecDeque<ResolvedTrack> {
        self.q.get_queue().await
    }
}

/// Get a suggestion from a query. Use the global static client.
pub async fn suggestion(query: &str) -> Result<Vec<String>, Error> {
    //let client = CrackTrackClient::new();
    let client = YOUTUBE_CLIENT.clone();
    suggestion_yt(client, query).await
}

/// Get a suggestion from a query. Passthrough to [rusty_ytdl::search::YouTube::suggestion].
pub async fn suggestion_yt(client: YouTube, query: &str) -> Result<Vec<String>, Error> {
    client
        .suggestion(query, None)
        .await
        .map_err(Into::into)
        .map(|res| res.into_iter().map(|x| x.replace("\"", "")).collect())
}

/// Args struct for the CLI.
#[derive(Parser)]
#[command(
    version = "1.0",
    author = "Cycle Five <cycle.five@proton.me>",
    about = "A simple CLI to get autocomplete suggestions from YouTube."
)]
struct Cli {
    /// The command to run
    #[command(subcommand)]
    command: Commands,
}

/// The command to run.
#[derive(Subcommand)]
enum Commands {
    Suggest {
        /// The query to get suggestions for.
        query: String,
    },
    Resolve {
        /// URL of the video / playlist to resolve.
        #[arg(value_parser = parse_url)]
        url: url::Url,
    },
}

/// Match the CLI command and run the appropriate function.
async fn match_cli(cli: Cli) -> Result<(), Error> {
    match cli.command {
        Commands::Suggest { query } => {
            let res = suggestion(&query).await?;
            for suggestion in res {
                println!("{}", suggestion);
            }
        },
        Commands::Resolve { url } => {
            // let query = QueryType::VideoLink(url.to_string());
            let mut client = CrackTrackClient::new();
            let res = client.resolve_playlist(url.as_str()).await?;
            for track in res {
                let _ = client.enqueue_track(track).await;
            }
        },
    }
    Ok(())
}

/// Run the CLI.
#[cfg(not(tarpaulin_include))]
pub async fn run() -> Result<(), Error> {
    let cli: Cli = Cli::parse();
    match_cli(cli).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[tokio::test]
    async fn test_cli() {
        let cli = Cli::parse_from(vec!["crack_testing", "suggest", "molly nilsson"]);
        match match_cli(cli).await {
            Ok(_) => (),
            Err(e) => eprintln!("{}", e),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_cli2() {
        let cli = Cli::parse_from(vec![
            "crack_testing",
            "resolve",
            "https://www.youtube.com/playlist?list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP",
        ]);
        match_cli(cli).await.expect("asdf");
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
            ..Default::default()
        };

        let resolved = client.resolve_track(query).await;

        if env::var("CI").is_ok() {
            assert!(resolved.is_err());
        } else {
            let res = resolved.expect("Failed to resolve track");
            let metadata = res.metadata.expect("No metadata");
            let title = metadata.title.expect("No title");
            assert_eq!(title, r#"Molly Nilsson "1995""#.to_string());
        }
    }

    #[tokio::test]
    async fn test_suggestion() {
        let client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
            ..Default::default()
        };

        let res = client
            .suggestion("molly nilsson")
            .await
            .expect("No results");
        assert_eq!(res.len(), 10);
        assert_eq!(
            res.iter()
                .filter(|x| x.starts_with("molly nilsson"))
                .collect::<Vec<_>>()
                .len(),
            10
        );
    }

    #[tokio::test]
    async fn test_suggestion_function() {
        let _ = YOUTUBE_CLIENT.clone();
        let res = suggestion("molly nilsson").await;
        let res = res.expect("No results");
        assert_eq!(res.len(), 10);
    }

    #[tokio::test]
    async fn test_enqueue_query() {
        let mut client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
            ..Default::default()
        };

        let queries = vec![
            QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string()),
            QueryType::VideoLink("https://www.youtube.com/watch?v=u8ZiCfW02S8".to_string()),
            QueryType::VideoLink("https://www.youtube.com/watch?v=r-Ag3DJ_VUE".to_string()),
        ];
        for query in queries {
            client
                .enqueue_query(query)
                .await
                .expect("Failed to enqueue query");
        }

        client
            .build_display()
            .await
            .expect("Failed to build display");

        let disp: String = client.get_display();
        assert!(disp.contains("Molly Nilsson"));
    }
}
