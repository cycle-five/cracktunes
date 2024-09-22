// ------------------------------------------------------------------
// Public types we use to simplify return and parameter types.
// ------------------------------------------------------------------
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
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
pub use rusty_ytdl::VideoInfo;
pub use serenity::all::Attachment;
pub use songbird::input::AuxMetadata;
pub use songbird::input::YoutubeDl;

#[derive(Clone, Debug)]
pub struct SpotifyTrack {
    pub full_track: FullTrack,
}

#[derive(Clone, Debug)]
/// Enum for type of possible queries we have to handle
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

pub fn video_info_to_aux_metadata(video: &VideoInfo) -> AuxMetadata {
    let mut metadata = AuxMetadata::default();
    tracing::info!(
        "video_info_to_aux_metadata: {:?}",
        video.video_details.title
    );
    let details = &video.video_details;
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
