// ------------------------------------------------------------------
// Public Re-exports
// ------------------------------------------------------------------
pub use rspotify::model::FullTrack;
pub use rusty_ytdl::search::SearchResult;
pub use rusty_ytdl::{VideoDetails, VideoInfo};
pub use songbird::input::AuxMetadata;
pub use songbird::input::YoutubeDl;
pub use std::time::Duration;
pub use typemap_rev::TypeMapKey;

// ------------------------------------------------------------------
// Non-public imports
// ------------------------------------------------------------------
use crate::full_track_artist_str;
use crate::SpotifyTrack;

/// [`AuxMetadata`] wrapper and utility functions.
#[derive(Debug, Clone)]
pub struct NewAuxMetadata(pub AuxMetadata);

/// Implement [`TypeMapKey`] for [`NewAuxMetadata`].
impl TypeMapKey for NewAuxMetadata {
    type Value = NewAuxMetadata;
}

/// Implement [`AuxMetadata`] for [`NewAuxMetadata`].
impl From<NewAuxMetadata> for AuxMetadata {
    fn from(metadata: NewAuxMetadata) -> Self {
        let NewAuxMetadata(metadata) = metadata;
        metadata
    }
}

/// Implement [`Default`] for [`NewAuxMetadata`].
impl Default for NewAuxMetadata {
    fn default() -> Self {
        NewAuxMetadata(AuxMetadata::default())
    }
}

/// Implement `NewAuxMetadata`.
impl NewAuxMetadata {
    /// Create a new `NewAuxMetadata` from `AuxMetadata`.
    #[must_use]
    pub fn new(metadata: AuxMetadata) -> Self {
        NewAuxMetadata(metadata)
    }

    /// Get the internal metadata.
    #[must_use]
    pub fn metadata(&self) -> &AuxMetadata {
        &self.0
    }

    /// Create new `NewAuxMetadata` from &`SpotifyTrack`.
    #[must_use]
    pub fn from_spotify_track(track: &SpotifyTrack) -> Self {
        #[allow(clippy::cast_sign_loss)]
        let duration: Duration =
            Duration::from_millis(track.full_track.duration.num_milliseconds() as u64);
        let name = track.full_track.name.clone();
        let artists = full_track_artist_str(&track.full_track);
        let album = track.full_track.album.clone().name;
        NewAuxMetadata(AuxMetadata {
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

    /// Set the [`AuxMetadata::source_url`].
    #[must_use]
    pub fn with_source_url(self, source_url: String) -> Self {
        NewAuxMetadata(AuxMetadata {
            source_url: Some(source_url),
            ..self.metadata().clone()
        })
    }

    /// Get a search query from the metadata for youtube.
    #[must_use]
    pub fn get_search_query(&self) -> String {
        let metadata = self.metadata();
        let title = metadata.title.clone().unwrap_or_default();
        let artist = metadata.artist.clone().unwrap_or_default();
        format!("{artist} - {title}")
    }
}

/// Convert [`SpotifyTrack`] to [`NewAuxMetadata`].
impl From<&SpotifyTrack> for NewAuxMetadata {
    fn from(track: &SpotifyTrack) -> Self {
        NewAuxMetadata::from_spotify_track(track)
    }
}

/// Convert [`SpotifyTrack`] to [`NewAuxMetadata`].
impl From<&SearchResult> for NewAuxMetadata {
    fn from(search_result: &SearchResult) -> Self {
        let mut metadata = AuxMetadata::default();
        match search_result.clone() {
            SearchResult::Video(video) => {
                metadata.track = Some(video.title.clone());
                metadata.artist = None;
                metadata.album = None;
                metadata.date.clone_from(&video.uploaded_at);

                metadata.channels = Some(2);
                metadata.channel = Some(video.channel.name);
                metadata.duration = Some(Duration::from_millis(video.duration));
                metadata.sample_rate = Some(48000);
                metadata.source_url = Some(video.url);
                metadata.title = Some(video.title);
                metadata.thumbnail = video.thumbnails.first().map(|x| x.url.clone());
            },
            SearchResult::Playlist(playlist) => {
                metadata.title = Some(playlist.name);
                metadata.source_url = Some(playlist.url);
                metadata.duration = None;
                metadata.thumbnail = playlist.thumbnails.first().map(|x| x.url.clone());
            },
            SearchResult::Channel(_) => {},
        };
        NewAuxMetadata(metadata)
    }
}

/// Implementation to convert [`SearchResult`] to [`NewAuxMetadata`].
impl From<SearchResult> for NewAuxMetadata {
    fn from(search_result: SearchResult) -> Self {
        NewAuxMetadata::from(&search_result)
    }
}

/// Convert [`VideoInfo`] to [`NewAuxMetadata`].
impl From<&VideoInfo> for NewAuxMetadata {
    fn from(video: &VideoInfo) -> Self {
        NewAuxMetadata::from(&video.video_details)
    }
}

/// Convert [`VideoInfo`] to [`AuxMetadata`].
#[must_use]
pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
    video_details_to_aux_metadata(&video.video_details)
}

/// Convert [`VideoDetails`] to [`NewAuxMetadata`].
impl From<&VideoDetails> for NewAuxMetadata {
    fn from(video_details: &VideoDetails) -> Self {
        NewAuxMetadata(video_details_to_aux_metadata(video_details))
    }
}

/// Convert [`VideoDetails`] to [`AuxMetadata`].
#[must_use]
pub fn video_details_to_aux_metadata(video_details: &VideoDetails) -> AuxMetadata {
    AuxMetadata {
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
        thumbnail: video_details.thumbnails.first().map(|x| x.url.clone()),
        ..Default::default()
    }
}

/// Convert a [`rusty_ytdl::search::Video`] to [`AuxMetadata`].
#[must_use]
pub fn search_video_to_aux_metadata(video: &rusty_ytdl::search::Video) -> AuxMetadata {
    AuxMetadata {
        artist: Some(video.channel.name.clone()),
        date: video.uploaded_at.clone(),
        channels: Some(2),
        channel: Some(video.channel.name.clone()),
        duration: Some(Duration::from_millis(video.duration)),
        sample_rate: Some(48000),
        source_url: Some(video.url.clone()),
        title: Some(video.title.clone()),
        thumbnail: video.thumbnails.first().map(|x| x.url.clone()),
        ..Default::default()
    }
}

/// Convert [`SearchResult`] to [`AuxMetadata`].
#[must_use]
pub fn search_result_to_aux_metadata(res: &SearchResult) -> AuxMetadata {
    match res.clone() {
        SearchResult::Video(video) => search_video_to_aux_metadata(&video),
        SearchResult::Playlist(playlist) => AuxMetadata {
            title: Some(playlist.name),
            source_url: Some(playlist.url),
            thumbnail: playlist.thumbnails.first().map(|x| x.url.clone()),
            ..Default::default()
        },
        SearchResult::Channel(_) => AuxMetadata::default(),
    }
}
