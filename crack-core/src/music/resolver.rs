use crate::http_utils;
use crate::music::track::{Track, TrackCollection, TrackMetadata, TrackSource};
use crack_types::{
    metadata::search_result_to_aux_metadata, metadata::video_info_to_aux_metadata, CrackedError,
};
use rusty_ytdl::search::{Playlist, YouTube};
use rusty_ytdl::{RequestOptions, Video, VideoOptions};
use serenity::all::Attachment;
use serenity::small_fixed_array::FixedString;
use songbird::input::{HttpRequest, Input as SongbirdInput, YoutubeDl};
use songbird::tracks::Track as SongbirdTrack;
use url::Url;

/// Service for resolving queries to tracks
pub struct TrackResolver {
    /// HTTP client for making requests
    client: reqwest::Client,
    /// YouTube client for searching and fetching videos
    youtube: YouTube,
}

impl TrackResolver {
    /// Create a new track resolver with the given HTTP client
    pub fn new(client: reqwest::Client) -> Self {
        let request_options = RequestOptions {
            client: Some(client.clone()),
            ..Default::default()
        };
        let youtube =
            YouTube::new_with_options(&request_options).expect("Failed to create YouTube client");

        Self { client, youtube }
    }

    /// Create a new track resolver with the default HTTP client
    pub fn new_default() -> Self {
        Self::new(http_utils::get_client().clone())
    }

    /// Parse a query string into a Track
    pub async fn parse_query(&self, query_str: &str) -> Result<Track, CrackedError> {
        // Try to parse as URL first
        if let Ok(url) = Url::parse(query_str) {
            self.parse_url(url).await
        } else {
            // Treat as search query
            Ok(Track::new(TrackSource::YouTubeSearch(
                query_str.to_string(),
            )))
        }
    }

    /// Parse a URL into a Track
    async fn parse_url(&self, url: Url) -> Result<Track, CrackedError> {
        match url.host_str() {
            Some("www.youtube.com" | "youtube.com" | "youtu.be") => {
                // Check if it's a playlist
                if url.query_pairs().any(|(k, _)| k == "list") {
                    Ok(Track::new(TrackSource::YouTubePlaylist(url.to_string())))
                } else {
                    Ok(Track::new(TrackSource::YouTube(url.to_string())))
                }
            },
            Some("open.spotify.com") => {
                // Check if it's a playlist
                let path_segments: Vec<&str> = url.path_segments().unwrap().collect();
                if path_segments.len() >= 2 && path_segments[0] == "playlist" {
                    Ok(Track::new(TrackSource::SpotifyPlaylist(url.to_string())))
                } else {
                    Ok(Track::new(TrackSource::Spotify(url.to_string())))
                }
            },
            // Handle other domains as generic HTTP URLs
            _ => Ok(Track::new(TrackSource::Http(url.to_string()))),
        }
    }

    /// Parse a file attachment into a Track
    pub async fn parse_file(&self, file: Attachment) -> Result<Track, CrackedError> {
        Ok(Track::new(TrackSource::File(file)))
    }

    /// Resolve a Track to a playable input
    pub async fn resolve_to_input(&self, track: &Track) -> Result<SongbirdInput, CrackedError> {
        match &track.source {
            TrackSource::YouTube(url) => {
                let ytdl = YoutubeDl::new(self.client.clone(), url.clone());
                Ok(ytdl.into())
            },
            TrackSource::YouTubeSearch(query) => {
                let ytdl = YoutubeDl::new_search(self.client.clone(), query.clone());
                Ok(ytdl.into())
            },
            TrackSource::File(file) => {
                Ok(HttpRequest::new(self.client.clone(), file.url.clone().to_string()).into())
            },
            TrackSource::Http(url) => Ok(HttpRequest::new(self.client.clone(), url.clone()).into()),
            TrackSource::Spotify(url) => {
                // Convert Spotify URL to YouTube search
                // This is simplified - actual implementation would extract track info
                let search_query = format!("{} audio", url);
                let ytdl = YoutubeDl::new_search(self.client.clone(), search_query);
                Ok(ytdl.into())
            },
            TrackSource::SpotifyPlaylist(url) => {
                // For playlists, we'd typically resolve each track individually
                // But for this example, we'll just return the first track
                let search_query = format!("{} playlist", url);
                let ytdl = YoutubeDl::new_search(self.client.clone(), search_query);
                Ok(ytdl.into())
            },
            TrackSource::YouTubePlaylist(url) => {
                // For playlists, we'd typically resolve each track individually
                // But for this example, we'll just return the first track
                let ytdl = YoutubeDl::new(self.client.clone(), url.clone());
                Ok(ytdl.into())
            },
        }
    }

    /// Resolve a Track to a fully populated Track with metadata
    pub async fn resolve(&self, track: &mut Track) -> Result<(), CrackedError> {
        // Clone the source to avoid borrowing issues
        let source = track.source.clone();

        // Handle different source types without recursion
        match source {
            TrackSource::YouTube(url) => self.resolve_youtube_video(track, &url).await,
            TrackSource::YouTubeSearch(query) => self.resolve_youtube_search(track, &query).await,
            TrackSource::YouTubePlaylist(url) => {
                // Just resolve the first track for now
                self.resolve_youtube_video(track, &url).await
            },
            TrackSource::Spotify(url) => {
                // Convert to YouTube search and resolve
                let search_query = format!("{} audio", url);
                // First update the source
                track.source = TrackSource::YouTubeSearch(search_query.clone());
                // Then resolve the search
                self.resolve_youtube_search(track, &search_query).await
            },
            TrackSource::SpotifyPlaylist(url) => {
                // Convert to YouTube search and resolve
                let search_query = format!("{} playlist", url);
                // First update the source
                track.source = TrackSource::YouTubeSearch(search_query.clone());
                // Then resolve the search
                self.resolve_youtube_search(track, &search_query).await
            },
            TrackSource::File(file) => {
                // For files, we don't have much metadata
                track.metadata = TrackMetadata {
                    title: Some(file.filename.clone()),
                    source_url: Some(file.url.clone()),
                    ..Default::default()
                };
                Ok(())
            },
            TrackSource::Http(url) => {
                // For HTTP URLs, we don't have much metadata
                track.metadata = TrackMetadata {
                    title: Some(FixedString::from_string_trunc(url.to_string())),
                    source_url: Some(FixedString::from_string_trunc(url.to_string())),
                    ..Default::default()
                };
                Ok(())
            },
        }
    }

    /// Resolve a YouTube video URL to a Track
    async fn resolve_youtube_video(
        &self,
        track: &mut Track,
        url: &str,
    ) -> Result<(), CrackedError> {
        let video_options = VideoOptions {
            request_options: RequestOptions {
                client: Some(self.client.clone()),
                ..Default::default()
            },
            ..Default::default()
        };

        let video = Video::new_with_options(url, video_options)?;
        let video_info = video.get_info().await?;
        let metadata = video_info_to_aux_metadata(&video_info);

        track.metadata = TrackMetadata::from(metadata);
        Ok(())
    }

    /// Resolve a YouTube search query to a Track
    async fn resolve_youtube_search(
        &self,
        track: &mut Track,
        query: &str,
    ) -> Result<(), CrackedError> {
        let search_result = self.youtube.search_one(query, None).await?;

        if let Some(result) = search_result {
            let metadata = search_result_to_aux_metadata(&result);
            let source_url = metadata.source_url.clone().unwrap_or_default();
            track.metadata = TrackMetadata::from(metadata);

            // Update source to the actual video URL
            track.source = TrackSource::YouTube(source_url);
            Ok(())
        } else {
            Err(CrackedError::Other("No search results found"))
        }
    }

    /// Resolve a YouTube playlist URL to a collection of Tracks
    pub async fn resolve_playlist(&self, url: &str) -> Result<TrackCollection, CrackedError> {
        let playlist = Playlist::get(url.to_string(), None).await?;

        let mut tracks = Vec::with_capacity(playlist.videos.len());

        for video in playlist.videos {
            let mut track = Track::new(TrackSource::YouTube(video.url.clone()));
            // We could resolve each track here, but that would be expensive
            // Instead, we'll just set the basic metadata
            track.metadata = TrackMetadata {
                title: Some(FixedString::from_string_trunc(video.title.clone())),
                source_url: Some(FixedString::from_string_trunc(video.url.clone())),
                thumbnail: video
                    .thumbnails
                    .first()
                    .map(|t| FixedString::from_string_trunc(t.url.clone())),
                ..Default::default()
            };
            tracks.push(track);
        }

        Ok(TrackCollection::with_tracks(tracks))
    }

    /// Resolve a Track to a playable SongbirdTrack
    pub async fn resolve_to_songbird_track(
        &self,
        track: &mut Track,
    ) -> Result<SongbirdTrack, CrackedError> {
        // First, resolve the track to get metadata
        self.resolve(track).await?;

        // Then, resolve to input
        let input = self.resolve_to_input(track).await?;

        // Finally, convert to songbird track
        Ok(track.to_songbird_track(input))
    }
}
