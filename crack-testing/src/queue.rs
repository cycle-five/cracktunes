use crate::ResolvedTrack;
use crack_types::Error;

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A queue of tracks to be played.
#[derive(Clone, Debug)]
pub struct CrackTrackQueue {
    inner: Arc<Mutex<VecDeque<ResolvedTrack>>>,
    display: Option<String>,
}

/// Implement [Default] for [CrackTrackQueue].
impl Default for CrackTrackQueue {
    fn default() -> Self {
        CrackTrackQueue {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            display: None,
        }
    }
}

/// Implement [CrackTrackQueue].
impl CrackTrackQueue {
    /// Create a new [CrackTrackQueue].
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a new [CrackTrackQueue] with a given queue.
    pub fn with_queue(queue: VecDeque<ResolvedTrack>) -> Self {
        CrackTrackQueue {
            inner: Arc::new(Mutex::new(queue)),
            display: None,
        }
    }

    /// Get the queue.
    pub async fn get_queue(&self) -> VecDeque<ResolvedTrack> {
        self.inner.lock().await.clone()
    }

    /// Enqueue a track.
    pub async fn enqueue(&self, track: ResolvedTrack) {
        self.inner.lock().await.push_back(track);
    }

    /// Dequeue a track.
    pub async fn dequeue(&self) -> Option<ResolvedTrack> {
        self.inner.lock().await.pop_front()
    }

    /// Return the display string for the queue.
    pub fn get_display(&self) -> String {
        self.display.clone().unwrap_or_default()
    }

    /// Build the display string for the queue.
    /// This *must* be called before displaying the queue.
    pub async fn build_display(&mut self) -> Result<(), Error> {
        let queue = self.inner.lock().await.clone();
        let mut res = String::new();
        for track in queue {
            res.push_str(&format!("{}\n", track));
        }
        self.display = Some(res);
        Ok(())
    }

    pub async fn clear(&self) {
        self.inner.lock().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }

    pub async fn get(&self, index: usize) -> Option<ResolvedTrack> {
        self.inner.lock().await.get(index).cloned()
    }

    pub async fn remove(&self, index: usize) -> Option<ResolvedTrack> {
        self.inner.lock().await.remove(index)
    }

    pub async fn push_back(&self, track: ResolvedTrack) {
        self.inner.lock().await.push_back(track);
    }

    pub async fn push_front(&self, track: ResolvedTrack) {
        self.inner.lock().await.push_front(track);
    }

    pub async fn pop_back(&self) -> Option<ResolvedTrack> {
        self.inner.lock().await.pop_back()
    }

    pub async fn pop_front(&self) -> Option<ResolvedTrack> {
        self.inner.lock().await.pop_front()
    }
}

impl Display for CrackTrackQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.display.as_ref().unwrap_or(&"No queue".to_string())
        )
    }
}
