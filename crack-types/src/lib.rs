// ------------------------------------------------------------------
// Public types we use to simplify return and parameter types.
// ------------------------------------------------------------------
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

pub type Error = Box<dyn StdError + Send + Sync>;
pub type ArcTRwLock<T> = Arc<RwLock<T>>;
pub type ArcTMutex<T> = Arc<Mutex<T>>;
pub type ArcRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcTRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcMutDMap<K, V> = Arc<Mutex<HashMap<K, V>>>;
// pub type CrackedResult<T> = Result<T, crack_core::CrackedError>;
// pub type CrackedHowResult<T> = anyhow::Result<T, crack_core::CrackedError>;

// ------------------------------------------------------------------
// Public Re-exports
// ------------------------------------------------------------------
pub use rspotify::model::FullTrack;
pub use rusty_ytdl::search::SearchResult;
pub use rusty_ytdl::{VideoDetails, VideoInfo};
pub use serenity::all::Attachment;
pub use serenity::prelude::TypeMapKey;
pub use songbird::input::AuxMetadata;
pub use songbird::input::YoutubeDl;

/// play Mode enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
    DownloadMKV,
    DownloadMP3,
    Search,
}

/// New struct pattern to wrap the spotify track.
#[derive(Clone, Debug)]
pub struct SpotifyTrack {
    pub full_track: FullTrack,
}

/// Enum for type of possible queries we have to handle.
#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(Attachment),
    NewYoutubeDl((YoutubeDl, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

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

/// Function to get the full artist name from a [FullTrack].
pub fn full_track_artist_str(track: &FullTrack) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.clone())
        .collect::<Vec<String>>()
        .join(", ")
}

/// Does very simple video info to aux metadata conversion
pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
    video_details_to_aux_metadata(&video.video_details)
}

/// Does very simple video info to aux metadata conversion
pub fn video_details_to_aux_metadata(details: &VideoDetails) -> AuxMetadata {
    let mut metadata = AuxMetadata::default();
    println!("video_details_to_aux_metadata: {:?}", details.title);
    metadata.artist = None;
    metadata.album = None;
    metadata.date = Some(details.publish_date.clone());

    metadata.channels = Some(2);
    metadata.channel = Some(details.owner_channel_name.clone());
    metadata.duration = Some(Duration::from_secs(
        details.length_seconds.parse::<u64>().unwrap_or_default(),
    ));
    metadata.sample_rate = Some(48000);
    metadata.source_url = Some(details.video_url.clone());
    metadata.title = Some(details.title.clone());
    metadata.thumbnail = Some(details.thumbnails.first().unwrap().url.clone());

    metadata
}

/// Converts a duration into a human readable timestamp
pub fn get_human_readable_timestamp(duration: Option<Duration>) -> String {
    match duration {
        Some(duration) if duration == Duration::MAX => "∞".to_string(),
        Some(duration) => {
            let seconds = duration.as_secs() % 60;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = duration.as_secs() / 3600;

            if hours < 1 {
                format!("{:02}:{:02}", minutes, seconds)
            } else {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        },
        None => "∞".to_string(),
    }
}

/// Builds a fake [Author] for testing purposes.
pub fn build_fake_rusty_author() -> rusty_ytdl::Author {
    rusty_ytdl::Author {
        id: "id".to_string(),
        name: "name".to_string(),
        user: "user".to_string(),
        channel_url: "channel_url".to_string(),
        external_channel_url: "external_channel_url".to_string(),
        user_url: "user_url".to_string(),
        thumbnails: vec![],
        verified: false,
        subscriber_count: 0,
    }
}

/// Builds a fake [Embed] for testing purposes.
pub fn build_fake_rusty_embed() -> rusty_ytdl::Embed {
    rusty_ytdl::Embed {
        flash_secure_url: "flash_secure_url".to_string(),
        flash_url: "flash_url".to_string(),
        iframe_url: "iframe_url".to_string(),
        width: 0,
        height: 0,
    }
}

/// Builds a fake [VideoDetails] for testing purposes.
pub fn build_fake_rusty_video_details() -> rusty_ytdl::VideoDetails {
    rusty_ytdl::VideoDetails {
        author: Some(build_fake_rusty_author()),
        likes: 0,
        dislikes: 0,
        age_restricted: false,
        video_url: "video_url".to_string(),
        storyboards: vec![],
        chapters: vec![],
        embed: build_fake_rusty_embed(),
        title: "title".to_string(),
        description: "description".to_string(),
        length_seconds: "60".to_string(),
        owner_profile_url: "owner_profile_url".to_string(),
        external_channel_id: "external_channel_id".to_string(),
        is_family_safe: false,
        available_countries: vec![],
        is_unlisted: false,
        has_ypc_metadata: false,
        view_count: "0".to_string(),
        category: "category".to_string(),
        publish_date: "publish_date".to_string(),
        owner_channel_name: "owner_channel_name".to_string(),
        upload_date: "upload_date".to_string(),
        video_id: "video_id".to_string(),
        keywords: vec![],
        channel_id: "channel_id".to_string(),
        is_owner_viewing: false,
        is_crawlable: false,
        allow_ratings: false,
        is_private: false,
        is_unplugged_corpus: false,
        is_live_content: false,
        thumbnails: vec![rusty_ytdl::Thumbnail {
            url: "thumbnail_url".to_string(),
            width: 0,
            height: 0,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_human_readable_timestamp() {
        assert_eq!(get_human_readable_timestamp(None), "∞");
        assert_eq!(get_human_readable_timestamp(Some(Duration::MAX)), "∞");
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(0)),),
            "00:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(59)),),
            "00:59"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(60)),),
            "01:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(61)),),
            "01:01"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3599)),),
            "59:59"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3600)),),
            "01:00:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3601)),),
            "01:00:01"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3661)),),
            "01:01:01"
        );
    }

    #[test]
    fn test_video_details_to_aux_metadata() {
        let details = build_fake_rusty_video_details();
        let metadata = video_details_to_aux_metadata(&details);
        assert_eq!(metadata.title, Some("title".to_string()));
        assert_eq!(metadata.source_url, Some("video_url".to_string()));
        assert_eq!(metadata.channel, Some("owner_channel_name".to_string()));
        assert_eq!(metadata.duration, Some(Duration::from_secs(60)));
        assert_eq!(metadata.date, Some("publish_date".to_string()));
        assert_eq!(metadata.thumbnail, Some("thumbnail_url".to_string()));
    }

    #[test]
    fn test_video_info_to_aux_metadata() {
        let details = build_fake_rusty_video_details();
        let info = VideoInfo {
            video_details: details,
            dash_manifest_url: Some("dash_manifest_url".to_string()),
            hls_manifest_url: Some("hls_manifest_url".to_string()),
            formats: vec![],
            related_videos: vec![],
        };
        let metadata = video_info_to_aux_metadata(&info);
        assert_eq!(metadata.title, Some("title".to_string()));
        assert_eq!(metadata.source_url, Some("video_url".to_string()));
        assert_eq!(metadata.channel, Some("owner_channel_name".to_string()));
        assert_eq!(metadata.duration, Some(Duration::from_secs(60)));
        assert_eq!(metadata.date, Some("publish_date".to_string()));
        assert_eq!(metadata.thumbnail, Some("thumbnail_url".to_string()));
    }
}
