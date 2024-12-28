use crate::{queue::*, video_info_to_aux_metadata};
use crate::{Error, QueryType, SearchResult, SpotifyTrackTrait, TrackResolveError};
use dashmap::DashMap;
use futures::stream::{FuturesUnordered, StreamExt};
use rusty_ytdl::search::{self, YouTube};
use rusty_ytdl::{RequestOptions, VideoOptions};
use serenity::all::{AutocompleteChoice, GuildId};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Client for resolving tracks, mostly holds other clients like reqwest and rusty_ytdl.
#[derive(Clone, Debug)]
pub struct CrackTrackClient<'a> {
    pub req_client: reqwest::Client,
    yt_client: rusty_ytdl::search::YouTube,
    video_opts: VideoOptions,
    q: Arc<DashMap<GuildId, CrackTrackQueue<'a>>>,
}

/// Implement [Default] for [CrackTrackClient].
impl Default for CrackTrackClient<'_> {
    fn default() -> Self {
        let req_client = reqwest::Client::new();
        let yt_client = rusty_ytdl::search::YouTube::new().expect(NEW_FAILED);
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
            q: Arc::new(DashMap::new()),
        }
    }
}

impl Display for CrackTrackClient<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CrackTrackClient")
    }
}

impl<'a> CrackTrackClient<'a> {
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
            q: Arc::new(DashMap::new()),
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
            q: Arc::new(DashMap::new()),
        }
    }

    pub async fn resolve_query_to_tracks(
        &self,
        query: QueryType,
    ) -> Result<Vec<ResolvedTrack<'a>>, Error> {
        match query {
            QueryType::VideoLink(_) | QueryType::Keywords(_) => {
                self.resolve_track_many(vec![query]).await
            },
            QueryType::PlaylistLink(_) => {
                self.resolve_playlist(&query.build_query().unwrap_or_default())
                    .await
            },
            QueryType::KeywordList(keywords_list) => {
                let queries = keywords_list
                    .iter()
                    .map(|x| QueryType::Keywords(x.clone()))
                    .collect::<Vec<QueryType>>();
                self.resolve_track_many(queries).await
            },
            QueryType::NewYoutubeDl((_ytdl, opts)) => {
                let req_options = RequestOptions {
                    client: Some(self.req_client.clone()),
                    ..Default::default()
                };
                let video_options = VideoOptions {
                    request_options: req_options.clone(),
                    ..Default::default()
                };
                let video = rusty_ytdl::Video::new_with_options(
                    opts.clone().source_url.unwrap_or_default(),
                    video_options,
                )?;
                let info = video.get_info().await?;

                Ok(vec![ResolvedTrack::default()
                    .with_details(info.video_details)
                    .with_metadata(opts)
                    .with_video(video)])
            },
            QueryType::SpotifyTracks(tracks) => {
                let queries = tracks
                    .iter()
                    .map(|x| QueryType::Keywords(x.build_query()))
                    .collect::<Vec<QueryType>>();

                self.resolve_track_many(queries).await
            },
            _ => {
                tracing::error!("Query type not implemented: {query:?}");
                Err(TrackResolveError::UnknownQueryType.into())
            },
        }
    }

    /// Resolve many tracks from a `Vec` of queries.
    /// # Errors
    /// Returns an error if any track cannot be resolved.
    /// # TODO: Fix this so it can deal with failures.
    pub async fn resolve_track_many(
        &self,
        queries: Vec<QueryType>,
    ) -> Result<Vec<ResolvedTrack<'a>>, Error> {
        let mut queue = Vec::new();
        for query in queries {
            let track = self.resolve_track(query).await?;
            queue.push(track);
        }
        Ok(queue)
    }

    /// Resolve a track from a query. This does not start or ready the track for playback.
    /// # Errors
    /// Returns an error if the track cannot be resolved.
    pub async fn resolve_track(&self, query: QueryType) -> Result<ResolvedTrack<'a>, Error> {
        match query {
            QueryType::VideoLink(ref url) => self.resolve_url(url).await,
            QueryType::Keywords(ref keywords) => {
                let search_results = self.yt_client.search_one(keywords, None).await?;
                let video = match search_results {
                    Some(SearchResult::Video(result)) => result,
                    _ => return Err(TrackResolveError::NotFound.into()),
                };
                let video_url = video.url.clone();
                self.resolve_url(&video_url).await
            },
            _ => {
                tracing::error!("Query type not implemented: {query:?}");
                Err(TrackResolveError::UnknownQueryType.into())
            },
        }
    }

    /// Resolve a URL and return a single track.
    async fn resolve_url(&self, url: &str) -> Result<ResolvedTrack<'a>, Error> {
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

        Ok(ResolvedTrack::default()
            .with_details(info.video_details)
            .with_metadata(metadata)
            .with_video(video))
    }

    /// Resolve a search query and return a single track.
    pub async fn resolve_search_one(&self, query: &str) -> Result<ResolvedTrack<'a>, Error> {
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
    pub async fn resolve_search(&self, query: &str) -> Result<Vec<ResolvedTrack<'a>>, Error> {
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
            queue.push(video.into());
        }
        Ok(queue)
    }

    /// Resolve a search query and return a queue of tracks.
    pub async fn resolve_search_faster(
        &self,
        query: &str,
    ) -> Result<Vec<ResolvedTrack<'a>>, Error> {
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
        }
        while let Some(res) = tasks.next().await {
            let track = res?;
            queue.push(track);
        }
        Ok(queue)
    }

    /// Get a suggestion autocomplete from a search instead of the suggestion api.
    pub async fn resolve_suggestion_search(
        &self,
        query: &str,
    ) -> Result<Vec<AutocompleteChoice<'a>>, Error> {
        let tracks = self.resolve_search(query).await?;
        let autocomplete_choices: Vec<AutocompleteChoice<'a>> = tracks
            .iter()
            .map(|track| Cow::Owned(track.clone()))
            .collect::<Vec<Cow<'a, ResolvedTrack>>>()
            .into_iter()
            .map(|track| track.clone().autocomplete_option())
            .collect::<Vec<AutocompleteChoice<'a>>>();
        Ok(autocomplete_choices)
    }

    /// Resolve a playlist from a URL. Limit is set to 50 by default.
    pub async fn resolve_playlist<'b>(
        &self,
        url: &'b str,
    ) -> Result<Vec<ResolvedTrack<'a>>, Error> {
        self.resolve_playlist_limit(url, DEFAULT_PLAYLIST_LIMIT)
            .await
    }

    /// Resolve a playlist from a URL. Limit must be given, this is intended to be used primarily by
    /// a helper method in the CrackTrackClient.
    pub async fn resolve_playlist_limit<'b>(
        &self,
        url: &'b str,
        limit: u64,
    ) -> Result<Vec<ResolvedTrack<'a>>, Error> {
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
            let track = ResolvedTrack::default()
                .with_query(QueryType::VideoLink(video.url.clone()))
                .with_search_video(video);
            println!("Resolved: {}", track);
            queue.push(track);
        }
        Ok(queue)
    }

    /// Get a suggestion from a query. Passthrough to [`rusty_ytdl::search::YouTube::suggestion`].
    pub async fn suggestion(&self, query: &str) -> Result<Vec<String>, Error> {
        suggestion_yt(self.yt_client.clone(), query).await
    }

    pub fn ensure_queue(&self, guild: GuildId) -> CrackTrackQueue<'a> {
        if let Some(q) = self.q.get(&guild) {
            q.clone()
        } else {
            let q: &mut CrackTrackQueue<'a> = Box::leak(Box::new(CrackTrackQueue::new()));
            //let q = *q;
            self.q.insert(guild, q.clone());
            q.clone()
        }
    }

    /// Resolve a track from a query and enqueue it.
    pub async fn enqueue_query(
        &mut self,
        guild: GuildId,
        query: QueryType,
    ) -> Result<ResolvedTrack<'a>, Error> {
        let track = self.resolve_track(query).await?;
        let _ = self.ensure_queue(guild).push_back(track.clone()).await;
        Ok(track)
    }

    /// Enqueue a track internally.
    pub async fn enqueue_track(
        &mut self,
        guild: GuildId,
        track: ResolvedTrack<'a>,
    ) -> Result<ResolvedTrack<'a>, Error> {
        let _ = self.ensure_queue(guild).push_back(track.clone()).await;
        Ok(track)
    }

    /// Append vec of tracks to the queue.
    pub async fn append_queue(
        &mut self,
        guild: GuildId,
        tracks: Vec<ResolvedTrack<'a>>,
    ) -> Result<(), Error> {
        for track in tracks {
            let () = self.ensure_queue(guild).push_back(track).await;
        }
        Ok(())
    }

    /// Build the display string for the queue.
    /// This is separate because it needs to be used non-async,
    /// but must be created async.
    pub async fn build_display(&mut self, guild: GuildId) -> Result<(), Error> {
        self.ensure_queue(guild).build_display().await
    }

    /// Get the display string for the queue.
    pub fn get_display(&self, guild: GuildId) -> String {
        self.ensure_queue(guild).get_display()
    }

    /// Get the queue.
    pub async fn get_queue(&self, guild: GuildId) -> VecDeque<ResolvedTrack<'a>> {
        self.ensure_queue(guild).get_queue().await
    }
}

// /// Get a suggestion from a query. Use the global static client.
// /// # Errors
// /// Returns an error if the query fails.
// pub async fn suggestion(query: &str) -> Result<Vec<String>, Error> {
//     let client = YOUTUBE_CLIENT.clone();
//     suggestion_yt(client, query).await
// }

/// Get a suggestion from a query. Passthrough to [`rusty_ytdl::search::YouTube::suggestion`].
/// # Errors
/// Returns an error if the query fails.
pub async fn suggestion_yt(client: YouTube, query: &str) -> Result<Vec<String>, Error> {
    let query = query.replace('"', "");
    if query.is_empty() {
        return Ok(Vec::new());
    }
    client
        .suggestion(query, Some(search::LanguageTags::EN))
        .await
        .map_err(Into::into)
        .map(|res| res.into_iter().map(|x| x.replace('"', "")).collect())
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

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

        let req_client = reqwest::Client::new();
        let yt_client = rusty_ytdl::search::YouTube::new().expect(NEW_FAILED);
        let client = CrackTrackClient::new_with_clients(req_client, yt_client);

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
        let req_client = reqwest::Client::new();
        let yt_client = rusty_ytdl::search::YouTube::new().expect(NEW_FAILED);
        let client = CrackTrackClient::new_with_clients(req_client, yt_client);

        let res = client
            .resolve_suggestion_search("molly nilsson")
            .await
            .expect("No results");
        assert_eq!(res.len(), 5);
        println!("{res:?}");
        assert_eq!(
            res.iter()
                .filter(|&x| x.clone().name.contains("Molly Nilsson"))
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
        // let client = YOUTUBE_CLIENT.clone();
        let client = rusty_ytdl::search::YouTube::new().expect(NEW_FAILED);
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
        let guild = GuildId::new(1);
        let mut client = CrackTrackClient::new();

        let queries = vec![
            QueryType::VideoLink("https://www.youtube.com/watch?v=X9ukSm5gmKk".to_string()),
            QueryType::VideoLink("https://www.youtube.com/watch?v=u8ZiCfW02S8".to_string()),
            QueryType::VideoLink("https://www.youtube.com/watch?v=r-Ag3DJ_VUE".to_string()),
        ];
        for query in queries {
            if let Ok(track) = client.enqueue_query(guild, query).await {
                println!("Enqueued: {}", track);
                client
                    .build_display(guild)
                    .await
                    .expect("Failed to build display");
                let disp: String = client.get_display(guild);
                println!("{}", disp);
            } else {
                if !std::env::var("CI").is_ok() {
                    assert!(false);
                }
            }
        }

        client
            .build_display(guild)
            .await
            .expect("Failed to build display");

        let mut q = client.get_queue(guild).await;
        assert_eq!(q.len(), 3);
        let first = q.pop_front().unwrap();
        assert!(first.get_title().contains("Molly Nilsson"));
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
            let res = yt_url_type(&url);
            match res {
                QueryType::VideoLink(_) => assert!(!want),
                QueryType::PlaylistLink(_) => assert!(want),
                _ => assert!(false),
            }
        }
    }
}
