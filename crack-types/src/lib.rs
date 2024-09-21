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
pub use serenity::all::Attachment;
pub use songbird::input::AuxMetadata;
pub use songbird::input::YoutubeDl;

pub use crate::crack_core::QueryType;

// #[derive(Clone, Debug)]
// pub struct SpotifyTrack {
//     pub fulltrack: FullTrack,
// }

// #[derive(Clone, Debug)]
// /// Enum for type of possible queries we have to handle
// pub enum QueryType {
//     Keywords(String),
//     KeywordList(Vec<String>),
//     VideoLink(String),
//     SpotifyTracks(Vec<SpotifyTrack>),
//     PlaylistLink(String),
//     File(Attachment),
//     NewYoutubeDl((YoutubeDl, AuxMetadata)),
//     YoutubeSearch(String),
//     None,
// }
