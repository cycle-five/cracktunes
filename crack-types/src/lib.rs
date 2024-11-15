// ------------------------------------------------------------------
// Modules
// ------------------------------------------------------------------
pub mod http;
pub use http::*;
pub mod metadata;
pub use metadata::*;
pub mod reply_handle;
pub use reply_handle::*;
// ------------------------------------------------------------------
// Non-public imports
// ------------------------------------------------------------------
use songbird::Call;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
// ------------------------------------------------------------------
// Public types we use to simplify return and parameter types.
// ------------------------------------------------------------------

pub type Error = Box<dyn StdError + Send + Sync>;
pub type ArcTRwLock<T> = Arc<RwLock<T>>;
pub type ArcTMutex<T> = Arc<Mutex<T>>;
pub type ArcRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcTRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcMutDMap<K, V> = Arc<Mutex<HashMap<K, V>>>;
// pub type CrackedResult<T> = Result<T, crack_core::CrackedError>;
// pub type CrackedHowResult<T> = anyhow::Result<T, crack_core::CrackedError>;
pub type SongbirdCall = Arc<Mutex<Call>>;

// ------------------------------------------------------------------
// Public Re-exports
// ------------------------------------------------------------------
pub use serenity::all::Attachment;
pub use thiserror::Error as ThisError;

/// Custom error type for track resolve errors.
#[derive(ThisError, Debug)]
pub enum TrackResolveError {
    #[error("No track found")]
    NotFound,
    #[error("Query is empty")]
    EmptyQuery,
    #[error("Error: {0}")]
    Other(String),
    #[error("Unknown resolve error")]
    Unknown,
    #[error("Unknown query type")]
    UnknownQueryType,
}

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
// QueryType -> Keywords       -> ResolvedTrack
//           -> VideoLink      -> ResolvedTrack
//           -> File           -> ResolvedTrack
//           -> NewYoutubeDl   -> ResolvedTrack        X
//           -> SpotifyTracks  -> Vec<ResolvedTrack>
//           -> PlaylistLink   -> Vec<ResolvedTrack>
//           -> KeywordList    -> Vec<ResolvedTrack>
//           -> YoutubeSearch  -> Vec<ResolvedTrack>
#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(Attachment),
    NewYoutubeDl((YoutubeDl<'static>, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

/// [`Default`] implementation for [`QueryType`].
impl Default for QueryType {
    fn default() -> Self {
        QueryType::None
    }
}

/// Function to get the full artist name from a [`FullTrack`].
pub fn full_track_artist_str(track: &FullTrack) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.clone())
        .collect::<Vec<String>>()
        .join(", ")
}

// use humantime::format_duration;
/// Converts a duration into a human readable timestamp
pub fn get_human_readable_timestamp(duration: Option<Duration>) -> String {
    // let duration = match duration {
    //     Some(duration) if duration == Duration::MAX => return "∞".to_string(),
    //     Some(duration) => duration,
    //     None => return "∞".to_string(),
    // };
    // let formatted = format_duration(duration);
    // formatted.substring(11, formatted.len() - 1).to_string()
    match duration {
        Some(duration) if duration == Duration::MAX => "∞".to_string(),
        Some(duration) => {
            let seconds = duration.as_secs() % 60;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = duration.as_secs() / 3600;

            if hours < 1 {
                format!("{minutes:02}:{seconds:02}")
            } else {
                format!("{hours:02}:{minutes:02}:{seconds:02}")
            }
        },
        None => "∞".to_string(),
    }
}

/// Builds a fake [`rusty_ytdl::Author`] for testing purposes.
#[must_use]
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

/// Builds a fake [`rusty_ytdl::Embed`] for testing purposes.
#[must_use]
pub fn build_fake_rusty_embed() -> rusty_ytdl::Embed {
    rusty_ytdl::Embed {
        flash_secure_url: "flash_secure_url".to_string(),
        flash_url: "flash_url".to_string(),
        iframe_url: "iframe_url".to_string(),
        width: 0,
        height: 0,
    }
}

/// Builds a fake [`VideoDetails`] for testing purposes.
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
