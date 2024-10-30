use crate::ResolvedTrack;
use crack_types::Error;

use rand::seq::SliceRandom;
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A [`CrackTrackQueue`] queue of tracks to be played.
#[derive(Clone, Debug)]
pub struct CrackTrackQueue {
    inner: Arc<Mutex<VecDeque<ResolvedTrack>>>,
    display: Option<String>,
}

/// Implement [`Default`] for [`CrackTrackQueue`].
impl Default for CrackTrackQueue {
    fn default() -> Self {
        CrackTrackQueue {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            display: None,
        }
    }
}

/// Implement [`CrackTrackQueue`].
impl CrackTrackQueue {
    /// Create a new [`CrackTrackQueue`].
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a new [`CrackTrackQueue`] with a given [`VecDeque`] of [`ResolvedTrack`].
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
        self.display = {
            let queue = self.inner.lock().await.clone();
            Some(
                queue
                    .iter()
                    .map(|track| track.to_string())
                    .collect::<Vec<String>>()
                    .join("\n"),
            )
        };
        Ok(())
    }

    /// Clear the queue in place.
    pub async fn clear(&self) {
        self.inner.lock().await.clear();
    }

    /// Get the length of the queue.
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Check if the queue is empty.
    pub async fn is_empty(&self) -> bool {
        self.inner.lock().await.is_empty()
    }

    /// Get the element at the given index in the queue.
    pub async fn get(&self, index: usize) -> Option<ResolvedTrack> {
        self.inner.lock().await.get(index).cloned()
    }

    /// Remove the element at the given index in the queue.
    pub async fn remove(&self, index: usize) -> Option<ResolvedTrack> {
        self.inner.lock().await.remove(index)
    }

    /// Add a track to the back of the queue.
    pub async fn push_back(&self, track: ResolvedTrack) {
        self.inner.lock().await.push_back(track);
    }

    /// Add a track to the front of the queue.
    pub async fn push_front(&self, track: ResolvedTrack) {
        self.inner.lock().await.push_front(track);
    }

    /// Remove the last track from the queue.
    pub async fn pop_back(&self) -> Option<ResolvedTrack> {
        self.inner.lock().await.pop_back()
    }

    /// Remove the first track from the queue.
    pub async fn pop_front(&self) -> Option<ResolvedTrack> {
        self.inner.lock().await.pop_front()
    }

    /// Insert a track at the given index in the queue.
    pub async fn insert(&self, index: usize, track: ResolvedTrack) {
        self.inner.lock().await.insert(index, track);
    }

    /// Append a vector of tracks to the end of the queue.
    pub async fn append_vec(&self, vec: Vec<ResolvedTrack>) {
        self.inner.lock().await.append(&mut VecDeque::from(vec));
    }

    /// Append another queue to the end of this queue.
    pub async fn append(&self, other: &mut VecDeque<ResolvedTrack>) {
        self.inner.lock().await.append(other);
    }

    /// Append another queue to the front of this queue.
    pub async fn append_front(&self, other: &mut VecDeque<ResolvedTrack>) {
        self.inner.lock().await.append(&mut other.clone());
    }

    /// Shuffle the queue.
    pub async fn shuffle(&self) {
        let mut queue = self.inner.lock().await.clone();
        queue.make_contiguous().shuffle(&mut rand::thread_rng());
        *self.inner.lock().await = queue;
    }

    pub async fn append_self_to_other(&self, other: &mut VecDeque<ResolvedTrack>) {
        other.append(&mut self.inner.lock().await.clone());
    }
}

/// Implement [`Display`] for [`CrackTrackQueue`].
impl Display for CrackTrackQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.display.as_ref().unwrap_or(&"No queue".to_string())
        )
    }
}
