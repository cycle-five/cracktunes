//! The data a queued track carries, and the reads of it.

use crack_common::errors::CrackedError;
use serenity::all::UserId;
#[allow(deprecated)]
use songbird::{input::AuxMetadata, tracks::TrackHandle};
use std::sync::Arc;
use tokio::sync::RwLock;

// This term gets appended to search queries in the default mode to try to find the album version of a song.
// pub const MUSIC_SEARCH_SUFFIX: &str = "album version";
// FIXME: Whether we use this or not it doesnt' go here.
pub const MUSIC_SEARCH_SUFFIX: &str = r#"\"topic\""#;

//use tokio::sync::RwLock;
/// Modifiable data struct for the track information.
#[derive(Clone, Debug, Default)]
pub struct TrackData {
    pub user_id: Arc<RwLock<Option<UserId>>>,
    pub aux_metadata: Arc<RwLock<Option<AuxMetadata>>>,
}

unsafe impl Send for TrackData {}
unsafe impl Sync for TrackData {}

impl TrackData {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::new(RwLock::new(Some(UserId::new(1)))),
            aux_metadata: Arc::new(RwLock::new(None)),
        })
    }

    pub fn with_user_id(self: Arc<Self>, user_id: UserId) -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::new(RwLock::new(Some(user_id))),
            aux_metadata: Arc::clone(&self.aux_metadata),
        })
    }

    pub fn with_metadata(self: Arc<Self>, md: AuxMetadata) -> Arc<Self> {
        Arc::new(Self {
            user_id: Arc::clone(&self.user_id),
            aux_metadata: Arc::new(RwLock::new(Some(md))),
        })
    }
}

// impl Default for TrackData {
//     fn default() -> Self {
//         *Self::new().clone()
//     }
// }
// pub type ArcTrackData = Arc<TrackDataInner>;

// impl std::ops::DerefMut for TrackData {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         Arc::get_mut(self).unwrap()
//     }
// }

/// The [`TrackData`] on a queued track.
///
/// The crate's only read of `TrackHandle::data`, which `clippy.toml` bans
/// everywhere else: it panics on a track built without `TrackData`, and only
/// `queue::new_track` guarantees there is some.
#[allow(clippy::disallowed_methods)]
pub fn track_data(track: &TrackHandle) -> Arc<TrackData> {
    track.data::<TrackData>()
}

/// Gets the requesting user from the typemap of the track handle.
pub async fn get_requesting_user(track: &TrackHandle) -> Result<UserId, CrackedError> {
    let data = track_data(track);
    let lock = data.user_id.read().await;
    lock.ok_or(CrackedError::NoUserAutoplay)
}

/// Gets the metadata from a track.
pub async fn get_track_handle_metadata(track: &TrackHandle) -> Result<AuxMetadata, CrackedError> {
    let data = track_data(track);
    let lock = data.aux_metadata.read().await;
    lock.clone().ok_or(CrackedError::NoMetadata)
}
