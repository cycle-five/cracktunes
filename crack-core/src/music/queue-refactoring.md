# `queue.rs` Refactoring Plan

## Current Issues

1. **Redundant Data Structures**: Multiple overlapping track data structures (`TrackReadyData`, `ResolvedTrack`, etc.)
2. **Inefficient Queue Operations**: Multiple queue traversals and inefficient queue modifications
3. **Inconsistent Error Handling**: Mix of different error types and handling patterns
4. **Complex Mode Handling**: Mode handling logic is spread across multiple functions
5. **Performance Concerns**: Tracing logs indicate performance bottlenecks in queue operations

## Step-by-Step Refactoring Plan

### Step 1: Consolidate Track Data Structures

Replace the multiple track data structures with a single unified `Track` structure:

```rust
// Current structures
pub struct TrackReadyData {
    pub source: SongbirdInput,
    pub metadata: NewAuxMetadata,
    pub user_id: Option<UserId>,
    pub username: Option<String>,
}

// New unified structure
pub struct Track {
    pub metadata: TrackMetadata,
    pub user_id: Option<UserId>,
    pub source: Option<TrackSource>,
}

pub enum TrackSource {
    YouTube(String),
    Spotify(String),
    File(Attachment),
    // Other source types
}

impl Track {
    pub fn new(source: TrackSource) -> Self { /* ... */ }
    pub fn with_user_id(mut self, user_id: UserId) -> Self { /* ... */ }
    pub fn with_metadata(mut self, metadata: TrackMetadata) -> Self { /* ... */ }
    pub async fn resolve(&mut self, client: &reqwest::Client) -> Result<SongbirdInput, Error> { /* ... */ }
    pub fn to_songbird_track(&self, input: SongbirdInput) -> songbird::tracks::Track { /* ... */ }
}
```

### Step 2: Create a QueueManager Struct

Create a dedicated `QueueManager` struct to encapsulate queue operations:

```rust
pub struct QueueManager {
    call: Arc<Mutex<Call>>,
}

impl QueueManager {
    pub fn new(call: Arc<Mutex<Call>>) -> Self {
        Self { call }
    }

    pub async fn add_track(
        &self,
        track: Track,
        position: QueuePosition,
        resolver: &TrackResolver,
    ) -> Result<TrackHandle, Error> {
        // Implementation
    }

    pub async fn get_queue(&self) -> Vec<TrackHandle> {
        let handler = self.call.lock().await;
        handler.queue().current_queue()
    }

    pub async fn add_tracks(
        &self,
        tracks: Vec<Track>,
        position: QueuePosition,
        resolver: &TrackResolver,
    ) -> Result<Vec<TrackHandle>, Error> {
        // Implementation
    }

    // Other queue management methods
}
```

### Step 3: Simplify Queue Position Logic

Replace the current mode handling with a cleaner enum-based approach:

```rust
pub enum QueuePosition {
    Front,
    Next,
    End,
    Jump,
    Search,
    Download(bool), // bool indicates mp3 vs mkv
}

impl QueueManager {
    // Add to queue at specified position
    async fn add_at_position(
        &self,
        track: songbird::tracks::Track,
        position: QueuePosition,
    ) -> Result<TrackHandle, Error> {
        let mut handler = self.call.lock().await;
        
        match position {
            QueuePosition::Front => {
                // Add to front (skip current track)
                let track_handle = handler.enqueue(track).await;
                
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
                let track_handle = handler.enqueue(track).await;
                
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
                Ok(handler.enqueue(track).await)
            },
            // Handle other positions
            _ => Err(Error::from(CrackedError::Other("Position not supported"))),
        }
    }
}
```

### Step 4: Optimize Queue Traversals

Reduce the number of queue traversals by caching the queue when needed:

```rust
impl QueueManager {
    // Get the current queue with caching
    async fn get_queue_with_cache(&self) -> Vec<TrackHandle> {
        static QUEUE_CACHE: Lazy<Mutex<HashMap<GuildId, (Instant, Vec<TrackHandle>)>>> = 
            Lazy::new(|| Mutex::new(HashMap::new()));
        
        // Implementation with caching logic
    }
    
    // Add multiple tracks efficiently
    async fn add_tracks_efficiently(
        &self,
        tracks: Vec<Track>,
        position: QueuePosition,
    ) -> Result<Vec<TrackHandle>, Error> {
        let mut handler = self.call.lock().await;
        
        // Batch process tracks
        // ...
        
        Ok(handler.queue().current_queue())
    }
}
```

### Step 5: Improve Error Handling

Standardize error handling across all queue operations:

```rust
// Define custom error type for queue operations
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("Failed to add track to queue: {0}")]
    AddTrackFailed(String),
    
    #[error("Failed to modify queue: {0}")]
    ModifyQueueFailed(String),
    
    #[error("Invalid queue position: {0}")]
    InvalidPosition(String),
    
    #[error("Queue is empty")]
    EmptyQueue,
    
    // Other error variants
}

impl From<QueueError> for Error {
    fn from(err: QueueError) -> Self {
        // Convert to general error type
    }
}

// Use throughout QueueManager
impl QueueManager {
    async fn add_track_with_better_errors(
        &self,
        track: Track,
        position: QueuePosition,
    ) -> Result<TrackHandle, QueueError> {
        // Implementation with better error handling
    }
}
```

### Step 6: Refactor Mode Parsing

Move the mode parsing logic from `doplay.rs` to `queue.rs`:

```rust
// Parse mode from command options
pub fn parse_mode(mode_str: Option<String>, is_prefix: bool) -> QueuePosition {
    if is_prefix {
        // Parse from prefix command
        match mode_str.as_deref() {
            Some("next") => QueuePosition::Next,
            Some("front") => QueuePosition::Front,
            Some("jump") => QueuePosition::Jump,
            Some("search") => QueuePosition::Search,
            Some("downloadmp3") => QueuePosition::Download(true),
            Some("downloadmkv") => QueuePosition::Download(false),
            _ => QueuePosition::End,
        }
    } else {
        // Parse from slash command
        match mode_str.as_deref() {
            Some("next") => QueuePosition::Next,
            Some("front") => QueuePosition::Front,
            Some("jump") => QueuePosition::Jump,
            Some("search") => QueuePosition::Search,
            Some("downloadmp3") => QueuePosition::Download(true),
            Some("downloadmkv") => QueuePosition::Download(false),
            _ => QueuePosition::End,
        }
    }
}
```

## Performance Optimizations

### 1. Reduce Lock Contention

Minimize the time spent holding locks on the Call object:

```rust
// Before
async fn add_track(&self, track: Track) -> Result<TrackHandle, Error> {
    let mut handler = self.call.lock().await;
    // Do lots of work while holding the lock
    // ...
    Ok(handler.enqueue(track).await)
}

// After
async fn add_track(&self, track: Track) -> Result<TrackHandle, Error> {
    // Do preparation work outside the lock
    let songbird_track = track.to_songbird_track();
    
    // Only lock for the minimal time needed
    let mut handler = self.call.lock().await;
    Ok(handler.enqueue(songbird_track).await)
}
```

### 2. Batch Process Tracks

Process multiple tracks in a single operation when possible:

```rust
async fn add_tracks_batch(
    &self,
    tracks: Vec<Track>,
    position: QueuePosition,
) -> Result<Vec<TrackHandle>, Error> {
    let mut handler = self.call.lock().await;
    let mut track_handles = Vec::with_capacity(tracks.len());
    
    // Convert all tracks to songbird tracks first
    let songbird_tracks: Vec<_> = tracks.into_iter()
        .map(|t| t.to_songbird_track())
        .collect();
    
    // Then add them all at once
    for track in songbird_tracks {
        let handle = handler.enqueue(track).await;
        track_handles.push(handle);
    }
    
    // Reorder queue once at the end if needed
    if position != QueuePosition::End {
        handler.queue().modify_queue(|queue| {
            // Reorder logic
        });
    }
    
    Ok(track_handles)
}
```

### 3. Optimize Queue Traversals

Avoid unnecessary queue traversals:

```rust
// Before
async fn get_track_at_position(&self, position: usize) -> Option<TrackHandle> {
    let handler = self.call.lock().await;
    let queue = handler.queue().current_queue();
    queue.get(position).cloned()
}

// After
async fn get_track_at_position(&self, position: usize) -> Option<TrackHandle> {
    let handler = self.call.lock().await;
    handler.queue().get(position).cloned()
}
```

## Testing Strategy

1. **Unit Tests**:
   - Test each QueueManager method in isolation
   - Test edge cases (empty queue, single track, multiple tracks)
   - Test error handling

2. **Integration Tests**:
   - Test with different track types
   - Test with different queue positions
   - Test performance with large queues

## Implementation Order

1. Create the new data structures (`Track`, `TrackMetadata`, `QueuePosition`)
2. Implement the `QueueManager` struct
3. Refactor queue operations to use the new structures
4. Implement performance optimizations
5. Improve error handling
6. Add tests

This approach allows for incremental refactoring, where each step can be tested before moving on to the next.
