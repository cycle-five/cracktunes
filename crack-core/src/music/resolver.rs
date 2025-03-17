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

    /// Try to extract track info from a Spotify URL
    fn extract_spotify_info(&self, url: &str) -> Option<(String, String)> {
        // Parse the URL
        let url = Url::parse(url).ok()?;

        // Get path segments
        let path_segments: Vec<&str> = url.path_segments()?.collect();

        // Check if we have enough segments
        if path_segments.len() < 2 {
            return None;
        }

        // Extract type and ID
        let item_type = path_segments[0];
        let item_id = path_segments[1];

        // Handle different types
        match item_type {
            "track" => {
                // For tracks, try to get the track name from the fragment
                let fragment = url.fragment().unwrap_or("");
                if fragment.contains("si=") {
                    // This is a track with a name in the fragment
                    let track_name = fragment.split("si=").next()?;
                    return Some((track_name.to_string(), "".to_string()));
                }

                // If we can't get the name, just return the ID
                Some((format!("spotify track {}", item_id), "".to_string()))
            },
            "album" => Some((format!("spotify album {}", item_id), "".to_string())),
            "artist" => Some((format!("spotify artist {}", item_id), "".to_string())),
            "playlist" => Some((format!("spotify playlist {}", item_id), "".to_string())),
            _ => None,
        }
    }

    /// Resolve a Track to a playable input
    pub async fn resolve_to_input(&self, track: &Track) -> Result<SongbirdInput, CrackedError> {
        match &track.source {
            TrackSource::YouTube(url) => {
                // Use get_rusty_search for direct YouTube URLs
                use crate::sources::youtube::get_rusty_search;
                let rusty_search = get_rusty_search(self.client.clone(), url.clone()).await;

                if let Ok(search) = rusty_search {
                    // If successful, return the rusty_ytdl input
                    Ok(search.into())
                } else {
                    // If it fails, fall back to yt-dlp
                    tracing::warn!(
                        "rusty_ytdl failed for YouTube URL, falling back to yt-dlp: {:?}",
                        rusty_search.err()
                    );
                    let ytdl = YoutubeDl::new(self.client.clone(), url.clone());
                    Ok(ytdl.into())
                }
            },
            TrackSource::YouTubeSearch(query) => {
                // Use search_query_to_source_and_metadata_rusty for search queries
                use crate::sources::youtube::search_query_to_source_and_metadata_rusty;
                let result = search_query_to_source_and_metadata_rusty(
                    self.client.clone(),
                    crack_types::QueryType::Keywords(query.clone()),
                )
                .await;

                match result {
                    Ok((input, _)) => Ok(input),
                    Err(e) => {
                        // This already includes a fallback to yt-dlp in the original implementation
                        tracing::warn!("Search failed: {:?}", e);
                        Err(e)
                    },
                }
            },
            TrackSource::File(file) => {
                // Files don't need special handling
                Ok(HttpRequest::new(self.client.clone(), file.url.clone().to_string()).into())
            },
            TrackSource::Http(url) => {
                // HTTP URLs don't need special handling
                Ok(HttpRequest::new(self.client.clone(), url.clone()).into())
            },
            TrackSource::Spotify(url) => {
                // Extract better search terms from Spotify URL
                let search_query = match self.extract_spotify_info(url) {
                    Some((title, artist)) => {
                        if artist.is_empty() {
                            format!("{} audio", title)
                        } else {
                            format!("{} {} audio", title, artist)
                        }
                    },
                    None => format!("{} audio", url),
                };

                // Use search_query_to_source_and_metadata_rusty for the search
                use crate::sources::youtube::search_query_to_source_and_metadata_rusty;
                let result = search_query_to_source_and_metadata_rusty(
                    self.client.clone(),
                    crack_types::QueryType::Keywords(search_query),
                )
                .await;

                match result {
                    Ok((input, _)) => Ok(input),
                    Err(e) => {
                        // This already includes a fallback to yt-dlp in the original implementation
                        tracing::warn!("Spotify search failed: {:?}", e);
                        Err(e)
                    },
                }
            },
            TrackSource::SpotifyPlaylist(url) => {
                // Extract better search terms from Spotify playlist URL
                let search_query = match self.extract_spotify_info(url) {
                    Some((title, _)) => title,
                    None => format!("{} playlist", url),
                };

                // Use search_query_to_source_and_metadata_rusty for the search
                use crate::sources::youtube::search_query_to_source_and_metadata_rusty;
                let result = search_query_to_source_and_metadata_rusty(
                    self.client.clone(),
                    crack_types::QueryType::Keywords(search_query),
                )
                .await;

                match result {
                    Ok((input, _)) => Ok(input),
                    Err(e) => {
                        // This already includes a fallback to yt-dlp in the original implementation
                        tracing::warn!("Spotify playlist search failed: {:?}", e);
                        Err(e)
                    },
                }
            },
            TrackSource::YouTubePlaylist(url) => {
                // For YouTube playlists, we can use the direct URL
                // But we should handle the case where we want to get all tracks in the playlist
                // For now, just use the URL directly
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
