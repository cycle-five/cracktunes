use serenity::all::UserId;
use serenity::small_fixed_array::FixedString;
use songbird::input::{AuxMetadata, Input as SongbirdInput};
use songbird::tracks::Track as SongbirdTrack;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::utils::TrackData;
use crack_types::NewAuxMetadata;

/// Metadata for a track
#[derive(Clone, Debug, Default)]
pub struct TrackMetadata {
    /// Title of the track
    pub title: Option<FixedString>,
    /// URL to the source of the track
    pub source_url: Option<FixedString>,
    /// URL to a thumbnail image for the track
    pub thumbnail: Option<FixedString>,
    /// Duration of the track
    pub duration: Option<Duration>,
    /// Artist of the track
    pub artist: Option<FixedString>,
    /// Channel or uploader of the track
    pub channel: Option<FixedString>,
}

impl From<AuxMetadata> for TrackMetadata {
    fn from(metadata: AuxMetadata) -> Self {
        Self {
            title: metadata.title.map(|x| FixedString::from_string_trunc(x)),
            source_url: metadata
                .source_url
                .map(|x| FixedString::from_string_trunc(x)),
            thumbnail: metadata
                .thumbnail
                .map(|x| FixedString::from_string_trunc(x)),
            duration: metadata.duration,
            artist: metadata.artist.map(|x| FixedString::from_string_trunc(x)),
            channel: metadata.channel.map(|x| FixedString::from_string_trunc(x)),
        }
    }
}

impl From<NewAuxMetadata> for TrackMetadata {
    fn from(metadata: NewAuxMetadata) -> Self {
        Self::from(metadata.0)
    }
}

/// Source of a track
#[derive(Clone, Debug)]
pub enum TrackSource {
    /// YouTube video
    YouTube(String),
    /// YouTube search query
    YouTubeSearch(String),
    /// YouTube playlist
    YouTubePlaylist(String),
    /// Spotify track
    Spotify(String),
    /// Spotify playlist
    SpotifyPlaylist(String),
    /// File attachment
    File(serenity::all::Attachment),
    /// HTTP URL
    Http(String),
}

/// A track that can be played
#[derive(Clone, Debug)]
pub struct Track {
    /// Metadata for the track
    pub metadata: TrackMetadata,
    /// User who requested the track
    pub user_id: Option<UserId>,
    /// Source of the track
    pub source: TrackSource,
}

impl Track {
    /// Create a new track with the given source
    pub fn new(source: TrackSource) -> Self {
        Self {
            metadata: TrackMetadata::default(),
            user_id: None,
            source,
        }
    }

    /// Set the user ID for the track
    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Set the metadata for the track
    pub fn with_metadata(mut self, metadata: TrackMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Convert to a songbird Track
    pub fn to_songbird_track(&self, input: SongbirdInput) -> SongbirdTrack {
        let metadata = self.metadata.clone();
        let track_data = Arc::new(TrackData {
            user_id: Arc::new(RwLock::new(self.user_id)),
            aux_metadata: Arc::new(RwLock::new(Some(AuxMetadata {
                title: metadata.title.map(|x| x.to_string()),
                source_url: metadata.source_url.map(|x| x.to_string()),
                thumbnail: metadata.thumbnail.map(|x| x.to_string()),
                duration: metadata.duration,
                artist: metadata.artist.map(|x| x.to_string()),
                channel: metadata.channel.map(|x| x.to_string()),
                ..Default::default()
            }))),
        });

        SongbirdTrack::new_with_data(input, track_data)
    }
}

/// A collection of tracks
#[derive(Clone, Debug, Default)]
pub struct TrackCollection {
    /// Tracks in the collection
    pub tracks: Vec<Track>,
}

impl TrackCollection {
    /// Create a new empty track collection
    pub fn new() -> Self {
        Self { tracks: Vec::new() }
    }

    /// Create a new track collection with the given tracks
    pub fn with_tracks(tracks: Vec<Track>) -> Self {
        Self { tracks }
    }

    /// Add a track to the collection
    pub fn add_track(&mut self, track: Track) {
        self.tracks.push(track);
    }

    /// Get the number of tracks in the collection
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Check if the collection is empty
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

impl From<Vec<Track>> for TrackCollection {
    fn from(tracks: Vec<Track>) -> Self {
        Self { tracks }
    }
}

impl IntoIterator for TrackCollection {
    type Item = Track;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.tracks.into_iter()
    }
}
