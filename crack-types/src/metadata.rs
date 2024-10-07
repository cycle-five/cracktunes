// ------------------------------------------------------------------
// Public Re-exports
// ------------------------------------------------------------------
pub use rspotify::model::FullTrack;
pub use rusty_ytdl::search::SearchResult;
pub use rusty_ytdl::{VideoDetails, VideoInfo};
pub use serenity::prelude::TypeMapKey;
pub use songbird::input::AuxMetadata;
pub use songbird::input::YoutubeDl;
pub use std::time::Duration;

// ------------------------------------------------------------------
// Non-public imports
// ------------------------------------------------------------------
use crate::full_track_artist_str;
use crate::SpotifyTrack;

/// AuxMetadata wrapper and utility functions.
#[derive(Debug, Clone)]
pub struct MyAuxMetadata(pub AuxMetadata);

/// Implement TypeMapKey for MyAuxMetadata.
impl TypeMapKey for MyAuxMetadata {
    type Value = MyAuxMetadata;
}

/// Implement From<AuxMetadata> for MyAuxMetadata.
impl From<MyAuxMetadata> for AuxMetadata {
    fn from(metadata: MyAuxMetadata) -> Self {
        let MyAuxMetadata(metadata) = metadata;
        metadata
    }
}

/// Implement Default for MyAuxMetadata.
impl Default for MyAuxMetadata {
    fn default() -> Self {
        MyAuxMetadata(AuxMetadata::default())
    }
}

/// Implement MyAuxMetadata.
impl MyAuxMetadata {
    /// Create a new MyAuxMetadata from AuxMetadata.
    pub fn new(metadata: AuxMetadata) -> Self {
        MyAuxMetadata(metadata)
    }

    /// Get the internal metadata.
    pub fn metadata(&self) -> &AuxMetadata {
        &self.0
    }

    /// Create new MyAuxMetadata from &SpotifyTrack.
    pub fn from_spotify_track(track: &SpotifyTrack) -> Self {
        let duration: Duration =
            Duration::from_millis(track.full_track.duration.num_milliseconds() as u64);
        let name = track.full_track.name.clone();
        let artists = full_track_artist_str(&track.full_track);
        let album = track.full_track.album.clone().name;
        MyAuxMetadata(AuxMetadata {
            track: Some(name.clone()),
            artist: Some(artists),
            album: Some(album),
            date: None,
            start_time: Some(Duration::ZERO),
            duration: Some(duration),
            channels: Some(2),
            channel: None,
            sample_rate: None,
            source_url: None,
            thumbnail: Some(name.clone()),
            title: Some(name),
        })
    }

    /// Set the source_url.
    pub fn with_source_url(self, source_url: String) -> Self {
        MyAuxMetadata(AuxMetadata {
            source_url: Some(source_url),
            ..self.metadata().clone()
        })
    }

    /// Get a search query from the metadata for youtube.
    pub fn get_search_query(&self) -> String {
        let metadata = self.metadata();
        let title = metadata.title.clone().unwrap_or_default();
        let artist = metadata.artist.clone().unwrap_or_default();
        format!("{} - {}", artist, title)
    }
}

/// Implementation to convert `[&SpotifyTrack]` to `[MyAuxMetadata]`.
impl From<&SpotifyTrack> for MyAuxMetadata {
    fn from(track: &SpotifyTrack) -> Self {
        MyAuxMetadata::from_spotify_track(track)
    }
}

impl From<&SearchResult> for MyAuxMetadata {
    fn from(search_result: &SearchResult) -> Self {
        let mut metadata = AuxMetadata::default();
        match search_result.clone() {
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
        MyAuxMetadata(metadata)
    }
}

impl From<SearchResult> for MyAuxMetadata {
    fn from(search_result: SearchResult) -> Self {
        MyAuxMetadata::from(&search_result)
    }
}

/// Convert a `[VideoInfo]` to `[AuxMetadata]`.
pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
    video_details_to_aux_metadata(&video.video_details)
}

/// Convert a `[VideoDetails]` to `[AuxMetadata]`.
pub fn video_details_to_aux_metadata(video_details: &VideoDetails) -> AuxMetadata {
    AuxMetadata {
        artist: None,
        album: None,
        date: Some(video_details.publish_date.clone()),
        channels: Some(2),
        channel: Some(video_details.owner_channel_name.clone()),
        duration: Some(Duration::from_secs(
            video_details
                .length_seconds
                .parse::<u64>()
                .unwrap_or_default(),
        )),
        sample_rate: Some(48000),
        source_url: Some(video_details.video_url.clone()),
        title: Some(video_details.title.clone()),
        thumbnail: Some(video_details.thumbnails.first().unwrap().url.clone()),
        ..Default::default()
    }
}

/// Convert a `[rusty_ytdl::search::Video]` to `[AuxMetadata]`.
pub fn search_video_to_aux_metadata(video: &rusty_ytdl::search::Video) -> AuxMetadata {
    AuxMetadata {
        artist: Some(video.channel.name.clone()),
        date: video.uploaded_at.clone(),
        channels: Some(2),
        channel: Some(video.channel.name.clone()),
        duration: Some(Duration::from_secs(video.duration)),
        sample_rate: Some(48000),
        source_url: Some(video.url.clone()),
        title: Some(video.title.clone()),
        thumbnail: Some(video.thumbnails.first().unwrap().url.clone()),
        ..Default::default()
    }
}

/// Convert a `[SearchResult]` to `[AuxMetadata]`.
pub fn search_result_to_aux_metadata(res: &SearchResult) -> AuxMetadata {
    match res.clone() {
        SearchResult::Video(video) => search_video_to_aux_metadata(&video),
        SearchResult::Playlist(playlist) => AuxMetadata {
            title: Some(playlist.name),
            source_url: Some(playlist.url),
            thumbnail: Some(playlist.thumbnails.first().unwrap().url.clone()),
            ..Default::default()
        },
        _ => Default::default(),
    }
}
