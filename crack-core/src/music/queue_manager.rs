use crate::music::resolver::TrackResolver;
use crate::music::track::{Track, TrackCollection};
use crack_types::CrackedError;
use songbird::tracks::PlayMode;
use songbird::{tracks::TrackHandle, Call};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Position in the queue
#[derive(Clone, Debug, PartialEq)]
pub enum QueuePosition {
    /// At the front of the queue (skip current track)
    Front,
    /// After the current track
    Next,
    /// At the end of the queue
    End,
    /// At a specific position
    At(usize),
}

/// Manager for the track queue
pub struct QueueManager {
    /// Call handle for the voice connection
    call: Arc<Mutex<Call>>,
}

impl QueueManager {
    /// Create a new queue manager with the given call handle
    pub fn new(call: Arc<Mutex<Call>>) -> Self {
        Self { call }
    }

    /// Get the current queue
    pub async fn get_queue(&self) -> Vec<TrackHandle> {
        let handler = self.call.lock().await;
        handler.queue().current_queue()
    }

    /// Add a track to the queue at the specified position
    pub async fn add_track(
        &self,
        track: Track,
        position: QueuePosition,
        resolver: &TrackResolver,
    ) -> Result<TrackHandle, CrackedError> {
        // Resolve track to songbird track
        let mut track_clone = track.clone();
        let songbird_track = resolver.resolve_to_songbird_track(&mut track_clone).await?;

        // Add to queue at specified position
        let mut handler = self.call.lock().await;

        match position {
            QueuePosition::Front => {
                // Add to front (skip current track)
                let track_handle = handler.enqueue(songbird_track).await;

                // Move to front of queue
                handler.queue().modify_queue(|queue| {
                    if queue.len() > 1 {
                        let last = queue.pop_back().unwrap();
                        queue.insert(0, last);
                    }
                });

                Ok(track_handle)
            },
            QueuePosition::Next => {
                // Add after current track
                let track_handle = handler.enqueue(songbird_track).await;

                // Move to position after current track
                handler.queue().modify_queue(|queue| {
                    if queue.len() > 2 {
                        let last = queue.pop_back().unwrap();
                        queue.insert(1, last);
                    }
                });

                Ok(track_handle)
            },
            QueuePosition::End => {
                // Add to end (default behavior)
                Ok(handler.enqueue(songbird_track).await)
            },
            QueuePosition::At(index) => {
                // Add at specific position
                let track_handle = handler.enqueue(songbird_track).await;

                // Move to specified position
                handler.queue().modify_queue(|queue| {
                    if queue.len() > index + 1 {
                        let last = queue.pop_back().unwrap();
                        queue.insert(index, last);
                    }
                });

                Ok(track_handle)
            },
        }
    }

    /// Add multiple tracks to the queue
    pub async fn add_tracks(
        &self,
        tracks: TrackCollection,
        position: QueuePosition,
        resolver: &TrackResolver,
    ) -> Result<Vec<TrackHandle>, CrackedError> {
        let mut track_handles = Vec::new();

        match position {
            QueuePosition::End => {
                // For End position, we can add all tracks at once
                for mut track in tracks.tracks {
                    let songbird_track = resolver.resolve_to_songbird_track(&mut track).await?;
                    let mut handler = self.call.lock().await;
                    let track_handle = handler.enqueue(songbird_track).await;
                    track_handles.push(track_handle);
                }
            },
            QueuePosition::Front => {
                // For Front position, we need to add tracks in reverse order
                for mut track in tracks.tracks.into_iter().rev() {
                    let songbird_track = resolver.resolve_to_songbird_track(&mut track).await?;
                    let mut handler = self.call.lock().await;
                    let track_handle = handler.enqueue(songbird_track).await;

                    // Move to front of queue
                    handler.queue().modify_queue(|queue| {
                        if queue.len() > 1 {
                            let last = queue.pop_back().unwrap();
                            queue.insert(0, last);
                        }
                    });

                    track_handles.push(track_handle);
                }

                // Reverse the track handles to match the original order
                track_handles.reverse();
            },
            QueuePosition::Next => {
                // For Next position, we need to add tracks in reverse order
                for mut track in tracks.tracks.into_iter().rev() {
                    let songbird_track = resolver.resolve_to_songbird_track(&mut track).await?;
                    let mut handler = self.call.lock().await;
                    let track_handle = handler.enqueue(songbird_track).await;

                    // Move to position after current track
                    handler.queue().modify_queue(|queue| {
                        if queue.len() > 2 {
                            let last = queue.pop_back().unwrap();
                            queue.insert(1, last);
                        }
                    });

                    track_handles.push(track_handle);
                }

                // Reverse the track handles to match the original order
                track_handles.reverse();
            },
            QueuePosition::At(index) => {
                // For At position, we need to add tracks in reverse order
                for (i, mut track) in tracks.tracks.into_iter().rev().enumerate() {
                    let songbird_track = resolver.resolve_to_songbird_track(&mut track).await?;
                    let mut handler = self.call.lock().await;
                    let track_handle = handler.enqueue(songbird_track).await;

                    // Move to specified position
                    handler.queue().modify_queue(|queue| {
                        if queue.len() > index + 1 {
                            let last = queue.pop_back().unwrap();
                            queue.insert(index + i, last);
                        }
                    });

                    track_handles.push(track_handle);
                }

                // Reverse the track handles to match the original order
                track_handles.reverse();
            },
        }

        Ok(track_handles)
    }

    /// Skip to the next track
    pub async fn skip(&self) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        if handler.queue().is_empty() {
            return Err(CrackedError::Other("Queue is empty"));
        }

        handler.queue().skip()?;
        Ok(())
    }

    /// Skip to a specific track
    pub async fn skip_to(&self, index: usize) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        if handler.queue().is_empty() {
            return Err(CrackedError::Other("Queue is empty"));
        }

        if index >= handler.queue().len() {
            return Err(CrackedError::Other("Index out of bounds"));
        }

        // Skip to the specified track
        for _ in 0..index {
            handler.queue().skip()?;
        }

        Ok(())
    }

    /// Remove a track from the queue
    pub async fn remove(&self, index: usize) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        if handler.queue().is_empty() {
            return Err(CrackedError::Other("Queue is empty"));
        }

        if index >= handler.queue().len() {
            return Err(CrackedError::Other("Index out of bounds"));
        }

        handler.queue().modify_queue(|queue| {
            queue.remove(index);
        });

        Ok(())
    }

    /// Clear the queue
    pub async fn clear(&self) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        handler.queue().stop();

        Ok(())
    }

    /// Pause playback
    pub async fn pause(&self) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        handler.queue().pause()?;

        Ok(())
    }

    /// Resume playback
    pub async fn resume(&self) -> Result<(), CrackedError> {
        let handler = self.call.lock().await;

        handler.queue().resume()?;

        Ok(())
    }

    /// Check if playback is paused
    pub async fn is_paused(&self) -> Result<bool, CrackedError> {
        let handler = self.call.lock().await;

        if let Some(current) = handler.queue().current() {
            Ok(current.get_info().await?.playing == PlayMode::Pause)
        } else {
            Ok(false)
        }
    }
}
