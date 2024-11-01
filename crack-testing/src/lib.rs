pub mod queue;
use futures::StreamExt;
pub use queue::*;

//------------------------------------
// crack_types imports
//------------------------------------
use crack_types::TrackResolveError;
use crack_types::{get_human_readable_timestamp, parse_url, video_info_to_aux_metadata};
use crack_types::{AuxMetadata, Error, QueryType, SearchResult};
//------------------------------------
// External library imports
//------------------------------------
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;
use rusty_ytdl::{search, search::YouTube};
use rusty_ytdl::{RequestOptions, VideoDetails, VideoOptions};
//------------------------------------
// Standard library imports
//------------------------------------
use futures::stream::FuturesUnordered;
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

//------------------------------------
// Constants
//------------------------------------
pub const CREATING: &str = "Creating";
pub const DEFAULT_PLAYLIST_LIMIT: u64 = 50;
pub const EMPTY_QUEUE: &str = "Queue is empty or display not built.";
pub const NEW_FAILED: &str = "New failed";
pub const REQ_CLIENT_STR: &str = "Reqwest client";
pub const UNKNOWN_TITLE: &str = "Unknown title";
pub const UNKNOWN_URL: &str = "";
pub const UNKNOWN_DURATION: &str = "Unknown duration";
pub const YOUTUBE_CLIENT_STR: &str = "YouTube client";

//------------------------------------
// Module statics.
// I did this so that I could easily make sure only one instance of the client is created
// and that it's available to all functions in the module.
// I've read elsewhere that this is a bit of a bad practice, and that it's better to put
// the clients in a context struct and pass it around everywhere. Other than the potential
// problems from it getting out of hand if the module is too big, I don't see a problem with it.
//------------------------------------
static REQ_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    println!("{}: {}...", CREATING, REQ_CLIENT_STR);
    build_configured_reqwest_client()
});

static YOUTUBE_CLIENT: Lazy<rusty_ytdl::search::YouTube> = Lazy::new(|| {
    println!("{CREATING}: {YOUTUBE_CLIENT_STR}...");
    let req_client = REQ_CLIENT.clone();
    let opts = RequestOptions {
        client: Some(req_client.clone()),
        ..Default::default()
    };
    rusty_ytdl::search::YouTube::new_with_options(&opts)
        .unwrap_or_else(|_| panic!("{} {}", NEW_FAILED, YOUTUBE_CLIENT_STR))
});

static CRACK_TRACK_CLIENT: Lazy<CrackTrackClient> = Lazy::new(|| {
    println!("{CREATING}: CrackTrackClient...");
    CrackTrackClient::new_with_clients(REQ_CLIENT.clone(), YOUTUBE_CLIENT.clone())
});

/// Build a configured reqwest client for use in the CrackTrackClient.
pub fn build_configured_reqwest_client() -> reqwest::Client {
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .unwrap_or_else(|_| panic!("{} {}", NEW_FAILED, REQ_CLIENT_STR))
}

/// Struct the holds a track who's had it's metadata queried,
/// and thus has a video URI associated with it, and it has all the
/// necessary metadata to be displayed in a music player interface.
#[derive(Clone, Debug, Default)]
pub struct ResolvedTrack {
    // FIXME One of these three has the possibility of returning
    // the video id instead of the full URL. Need to figure out
    // which one and document why.
    details: Option<rusty_ytdl::VideoDetails>,
    metadata: Option<AuxMetadata>,
    search_video: Option<rusty_ytdl::search::Video>,
    #[allow(dead_code)]
    query: QueryType,
    #[allow(dead_code)]
    queued: bool,
    #[allow(dead_code)]
    video: Option<rusty_ytdl::Video>,
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
            search_video.title.clone()
        } else if let Some(metadata) = &self.metadata {
            metadata.title.clone().unwrap_or_default()
        } else if let Some(details) = &self.details {
            details.title.clone()
        } else {
            UNKNOWN_TITLE.to_string()
        }
    }

    /// Get the URL of the track.
    pub fn get_url(&self) -> String {
        let url = if let Some(search_video) = &self.search_video {
            search_video.url.clone()
        } else if let Some(metadata) = &self.metadata {
            metadata.source_url.clone().unwrap_or_default()
        } else if let Some(details) = &self.details {
            details.video_url.clone()
        } else {
            UNKNOWN_URL.to_string()
        };

        if url.contains("youtube.com") {
            url
        } else {
            format!("https://www.youtube.com/watch?v={}", url)
        }
    }

    /// Get the duration of the track.
    pub fn get_duration(&self) -> String {
        if let Some(metadata) = &self.metadata {
            get_human_readable_timestamp(metadata.duration)
        } else if let Some(details) = &self.details {
            let duration =
                Duration::from_secs(details.length_seconds.parse::<u64>().unwrap_or_default());
            get_human_readable_timestamp(Some(duration))
        } else if let Some(search_video) = &self.search_video {
            let duration = Duration::from_millis(search_video.duration);
            get_human_readable_timestamp(Some(duration))
        } else {
            UNKNOWN_DURATION.to_string()
        }
    }

    /// Get the metadata of the track.
    pub fn get_metdata(&self) -> Option<AuxMetadata> {
        self.metadata.clone()
    }

    pub fn suggest_string(&self) -> String {
        let title = self.get_title();
        //let url = self.get_url();
        let duration = self.get_duration();
        let mut str = format!("{} ~ `{}`", title, duration);
        str.truncate(100);
        str
    }
}

/// Implement [`From``] for [`search::Video`] to [`ResolvedTrack`].
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

/// Implement [`Display`] for [`ResolvedTrack`].
impl Display for ResolvedTrack {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let title = self.get_title();
        let url = self.get_url();
        let duration = self.get_duration();

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

    /// Create a new [`CrackTrackClient`] with a given [`reqwest::Client`].
    pub fn new_with_req_client(req_client: reqwest::Client) -> Self {
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
        //let vid_tuple: (rusty_ytdl::Video, VideoDetails, AuxMetadata) = match query {
        match query {
            QueryType::VideoLink(ref url) => {
                // let request_options = RequestOptions {
                //     client: Some(self.req_client.clone()),
                //     ..Default::default()
                // };
                // let video_options = VideoOptions {
                //     request_options: request_options.clone(),
                //     ..Default::default()
                // };
                // let video = rusty_ytdl::Video::new_with_options(url, video_options)?;
                // let info = video.get_info().await?;
                // let metadata = video_info_to_aux_metadata(&info);

                self.resolve_url(url).await
            },
            QueryType::Keywords(ref keywords) => {
                let search_results = self.yt_client.search_one(keywords, None).await?;
                let video = match search_results {
                    Some(SearchResult::Video(result)) => result,
                    _ => return Err(TrackResolveError::NotFound.into()),
                };
                let video_url = video.url.clone();
                self.resolve_url(&video_url).await
            },
            _ => unimplemented!(),
        }
    }

    async fn resolve_url(&self, url: &str) -> Result<ResolvedTrack, Error> {
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

        Ok(ResolvedTrack {
            query: QueryType::VideoLink(url.to_string()),
            video: Some(video),
            metadata: Some(metadata),
            details: Some(info.video_details),
            ..Default::default()
        })
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

    /// Resolve a search query and return a queue of tracks.
    pub async fn resolve_search(&self, query: &str) -> Result<Vec<ResolvedTrack>, Error> {
        let search_options = rusty_ytdl::search::SearchOptions {
            limit: 5,
            ..Default::default()
        };
        let search_results = self.yt_client.search(query, Some(&search_options)).await?;
        let mut queue = Vec::new();
        for result in search_results {
            let video = match result {
                SearchResult::Video(video) => video,
                _ => continue,
            };
            let video_url = video.url.clone();
            let query = QueryType::VideoLink(video_url);
            let track = self.resolve_track(query).await?;
            queue.push(track);
        }
        Ok(queue)
    }

    /// Resolve a search query and return a queue of tracks.
    pub async fn resolve_search_faster(&self, query: &str) -> Result<Vec<ResolvedTrack>, Error> {
        let search_options = rusty_ytdl::search::SearchOptions {
            limit: 5,
            ..Default::default()
        };
        let search_results = self.yt_client.search(query, Some(&search_options)).await?;
        let mut queue = Vec::new();
        let mut tasks =
            FuturesUnordered::<Pin<Box<dyn Future<Output = Result<ResolvedTrack, Error>>>>>::new();
        for result in search_results {
            let video = match result {
                SearchResult::Video(video) => video,
                _ => continue,
            };
            let video_url = video.url.clone();
            let query = QueryType::VideoLink(video_url);
            let track = self.resolve_track(query);
            tasks.push(Box::pin(track));
            // let track = self.resolve_track(query).await?;
            // queue.push(track);
        }
        while let Some(res) = tasks.next().await {
            let track = res?;
            queue.push(track);
        }
        Ok(queue)
    }

    /// Get a suggestion autocomplete from a search instead of the suggestion api.
    pub async fn resolve_suggestion_search(&self, query: &str) -> Result<Vec<String>, Error> {
        let tracks = self.resolve_search(query).await?;
        Ok(tracks
            .iter()
            .map(|track| track.suggest_string())
            .collect::<Vec<String>>())
    }

    /// Resolve a playlist from a URL. Limit is set to 50 by default.
    pub async fn resolve_playlist(&self, url: &str) -> Result<Vec<ResolvedTrack>, Error> {
        self.resolve_playlist_limit(url, DEFAULT_PLAYLIST_LIMIT)
            .await
    }

    /// Resolve a playlist from a URL. Limit must be given, this is intended to be used primarily by
    /// a helper method in the CrackTrackClient.
    pub async fn resolve_playlist_limit(
        &self,
        url: &str,
        limit: u64,
    ) -> Result<Vec<ResolvedTrack>, Error> {
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

        let mut queue = Vec::new();

        for video in res.videos {
            let track = ResolvedTrack {
                query: QueryType::VideoLink(video.url.clone()),
                search_video: Some(video),
                ..Default::default()
            };
            println!("Resolved: {}", track);
            queue.push(track);
        }
        Ok(queue)
    }

    /// Get a suggestion from a query. Passthrough to [`rusty_ytdl::search::YouTube::suggestion`].
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

    /// Append vec of tracks to the queue.
    pub async fn append_queue(&mut self, tracks: Vec<ResolvedTrack>) -> Result<(), Error> {
        for track in tracks {
            let _ = self.q.push_back(track).await;
        }
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
pub async fn suggestion2(query: &str) -> Result<Vec<String>, Error> {
    let client = CRACK_TRACK_CLIENT.clone();
    //let client = YOUTUBE_CLIENT.clone();
    client.resolve_suggestion_search(query).await
}

/// Get a suggestion from a query. Use the global static client.
pub async fn suggestion(query: &str) -> Result<Vec<String>, Error> {
    //let client = CrackTrackClient::new();
    let client = YOUTUBE_CLIENT.clone();
    suggestion_yt(client, query).await
}

/// Get a suggestion from a query. Passthrough to [rusty_ytdl::search::YouTube::suggestion].
pub async fn suggestion_yt(client: YouTube, query: &str) -> Result<Vec<String>, Error> {
    let query = query.replace("\"", "");
    if query.is_empty() {
        return Ok(Vec::new());
    }
    client
        .suggestion(query, Some(search::LanguageTags::EN))
        .await
        .map_err(Into::into)
        .map(|res| res.into_iter().map(|x| x.replace("\"", "")).collect())
}

/// Args struct for the CLI.
#[derive(Parser, Debug)]
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
#[derive(Subcommand, Debug)]
enum Commands {
    Suggest {
        /// The query to get suggestions for.
        query: String,
    },
    SuggestNew {
        /// The query to get suggestions for, second method.
        query: String,
    },
    Resolve {
        /// URL of the video / playlist to resolve.
        #[arg(value_parser = parse_url)]
        url: url::Url,
    },
    Query {
        /// The query to resolve.
        query: String,
    },
}

/// Get the query type from a youtube URL. Video or playlist.
async fn yt_url_type(url: &url::Url) -> Result<QueryType, Error> {
    if url.path().contains("playlist")
        || url.query_pairs().any(|(k, _)| k == "list") && url.path().contains("watch")
    {
        Ok(QueryType::PlaylistLink(url.to_string()))
    } else {
        Ok(QueryType::VideoLink(url.to_string()))
    }
}

/// Match the CLI command and run the appropriate function.
#[tracing::instrument]
async fn match_cli(cli: Cli) -> Result<(), Error> {
    let mut client = CrackTrackClient::new();
    match cli.command {
        Commands::Suggest { query } => {
            let res = suggestion(&query).await?;
            tracing::info!("Suggestions: {res:?}");
            // for suggestion in res {
            //     println!("{}", suggestion);
            // }
        },
        Commands::SuggestNew { query } => {
            let res = suggestion2(&query).await?;
            tracing::info!("Suggestions: {res:?}");
        },
        Commands::Resolve { url } => {
            let tracks = match yt_url_type(&url).await? {
                QueryType::VideoLink(url) => {
                    vec![client.resolve_track(QueryType::VideoLink(url)).await?]
                },
                QueryType::PlaylistLink(url) => client.resolve_playlist(url.as_str()).await?,
                _ => {
                    tracing::error!("Unknown URL type: {url}");
                    Vec::new()
                },
            };
            client.append_queue(tracks).await?;
        },
        Commands::Query { query } => {
            // let mut client = CrackTrackClient::new();
            let queries = query.split(",");
            for query in queries {
                let res = client.resolve_search_one(query).await?;
                println!("Resolved: {}", res);
                let _ = client.enqueue_track(res).await;
            }
        },
    }
    //client.build_display().await?;
    //println!("{}", client.get_display());
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

    // #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[tokio::test]
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
        if env::var("CI").is_ok() {
            return;
        }

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
    async fn test_suggestion2() {
        if env::var("CI").is_ok() {
            return;
        }
        let client = CrackTrackClient {
            req_client: reqwest::Client::new(),
            yt_client: rusty_ytdl::search::YouTube::new().expect(NEW_FAILED),
            ..Default::default()
        };

        let res = client
            .resolve_suggestion_search("molly nilsson")
            .await
            .expect("No results");
        assert_eq!(res.len(), 5);
        println!("{res:?}");
        assert_eq!(
            res.iter()
                .filter(|x| x.contains("Molly Nilsson"))
                .collect::<Vec<_>>()
                .len(),
            5
        );
    }

    #[tokio::test]
    async fn test_suggestion() {
        if env::var("CI").is_ok() {
            return;
        }
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
        if env::var("CI").is_ok() {
            return;
        }
        let client = YOUTUBE_CLIENT.clone();
        let res = suggestion_yt(client.clone(), "molly nilsson").await;
        if env::var("CI").is_ok() {
            assert!(res.is_err());
        } else {
            let res = res.expect("No results");
            assert_eq!(res.len(), 10);
        }
    }

    #[tokio::test]
    async fn test_enqueue_query() {
        if env::var("CI").is_ok() {
            return;
        }
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
            let res = client.enqueue_query(query).await;
            if std::env::var("CI").is_ok() {
                assert!(res.is_err());
            } else {
                let _ = res.expect("query failed");
            }
        }

        client
            .build_display()
            .await
            .expect("Failed to build display");

        let disp: String = client.get_display();
        assert!(disp.contains("Molly Nilsson"));
    }

    #[tokio::test]
    async fn test_yt_url_type() {
        let urls = vec![
            "https://www.youtube.com/watch?v=X9ukSm5gmKk",
            "https://www.youtube.com/watch?v=X9ukSm5gmKk&list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP",
            "https://www.youtube.com/playlist?list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP",
        ];
        let want_playlist = vec![false, true, true];
        let urls = urls
            .iter()
            .map(|x| url::Url::parse(x).expect("Failed to parse URL"))
            .collect::<Vec<_>>();

        for (url, want) in urls.iter().zip(want_playlist) {
            let res = yt_url_type(&url).await.expect("Failed to get URL type");
            match res {
                QueryType::VideoLink(_) => assert!(!want),
                QueryType::PlaylistLink(_) => assert!(want),
                _ => assert!(false),
            }
        }
    }
}
