# Music Command Refactoring Proposal

## Overview

This document outlines a comprehensive refactoring plan for the music command system in the `crack-core` crate, specifically focusing on the `play` command and related functionality in:
- `crack-core/src/commands/music/doplay.rs`
- `crack-core/src/music/queue.rs`
- `crack-core/src/music/query.rs`

## Current Issues

### 1. Code Complexity and Duplication
- The `play_internal` function in `doplay.rs` is overly complex (200+ lines) with many responsibilities
- Multiple similar query handling paths with duplicated logic
- Inconsistent error handling patterns
- Redundant track data structures (`TrackReadyData`, `ResolvedTrack`, etc.)

### 2. Performance Concerns
- Tracing logs indicate performance bottlenecks in queue operations
- Multiple queue traversals that could be optimized
- Inefficient metadata handling

### 3. Architectural Issues
- Unclear separation of concerns between query, queue, and playback logic
- Inconsistent use of different YouTube/media source APIs
- Confusing mode handling logic split across files

## Refactoring Recommendations

### 1. Restructure Command Flow

```
User Command → Command Parsing → Query Resolution → Track Creation → Queue Management → Playback Control → UI Updates
```

### 2. Consolidate Track Data Models

Currently, there are multiple overlapping track data structures:
- `TrackReadyData`
- `ResolvedTrack`
- `TrackData`
- Various metadata wrappers

**Recommendation:** Create a unified track model hierarchy:

```rust
// Proposed structure (simplified)
struct TrackMetadata {
    title: Option<String>,
    source_url: Option<String>,
    thumbnail: Option<String>,
    duration: Option<Duration>,
    // Other metadata fields
}

struct Track {
    metadata: TrackMetadata,
    user_id: Option<UserId>,
    source: Option<TrackSource>,
}

enum TrackSource {
    YouTube(String),
    Spotify(String),
    File(Attachment),
    // Other source types
}

impl Track {
    fn from_query(query: &str) -> Self { /* ... */ }
    fn resolve(&mut self) -> Result<(), Error> { /* ... */ }
    fn to_songbird(&self) -> Result<songbird::Track, Error> { /* ... */ }
}
```

### 3. Simplify Query Resolution

The current query resolution is spread across multiple files with redundant logic. Recommendations:

1. Create a dedicated `TrackResolver` service that handles all query types
2. Implement a clean strategy pattern for different source types
3. Separate the resolution logic from the command handling

### 4. Optimize Queue Operations

1. Reduce queue traversals - currently the code walks the queue multiple times
2. Implement more efficient queue operations for different modes
3. Add proper queue state management to avoid redundant operations

### 5. Improve Error Handling

1. Standardize error handling patterns
2. Add more descriptive error messages
3. Implement proper error recovery paths

### 6. Code Organization

Restructure the code into these modules:
- `commands/` - Just command handling and user interaction
- `playback/` - Core playback functionality
  - `playback/resolver.rs` - Track resolution from queries
  - `playback/queue.rs` - Queue management
  - `playback/track.rs` - Track models and metadata
  - `playback/sources/` - Source-specific implementations

## Detailed Implementation Plan

### Phase 1: Refactor Track Models

#### Create a new `track.rs` module

```rust
// src/music/track.rs
use serenity::all::{Attachment, UserId};
use songbird::input::Input as SongbirdInput;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub source_url: Option<String>,
    pub thumbnail: Option<String>,
    pub duration: Option<Duration>,
    // Other metadata fields as needed
}

#[derive(Debug)]
pub enum TrackSource {
    YouTube(String),
    Spotify(String),
    File(Attachment),
    Search(String),
    Playlist(String),
    // Other source types
}

#[derive(Debug)]
pub struct Track {
    pub metadata: TrackMetadata,
    pub user_id: Option<UserId>,
    pub source: TrackSource,
}

impl Track {
    pub fn new(source: TrackSource) -> Self {
        Self {
            metadata: TrackMetadata::default(),
            user_id: None,
            source,
        }
    }

    pub fn with_user_id(mut self, user_id: UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_metadata(mut self, metadata: TrackMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub async fn resolve(&mut self, client: &reqwest::Client) -> Result<SongbirdInput, Error> {
        // Implementation depends on source type
        match &self.source {
            TrackSource::YouTube(url) => {
                // Resolve YouTube URL to playable track
                // ...
            },
            // Handle other source types
            // ...
        }
    }

    pub fn to_songbird_track(&self, input: SongbirdInput) -> songbird::tracks::Track {
        // Convert to songbird Track with metadata
        // ...
    }
}
```

### Phase 2: Refactor Query Resolution

#### Create a `resolver.rs` module

```rust
// src/music/resolver.rs
use crate::music::track::{Track, TrackMetadata, TrackSource};
use serenity::all::Attachment;
use url::Url;

pub struct TrackResolver {
    client: reqwest::Client,
}

impl TrackResolver {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub async fn resolve_query(&self, query: &str) -> Result<Track, Error> {
        // Determine query type and create appropriate Track
        if let Ok(url) = Url::parse(query) {
            self.resolve_url(url).await
        } else {
            // Treat as search query
            Ok(Track::new(TrackSource::Search(query.to_string())))
        }
    }

    async fn resolve_url(&self, url: Url) -> Result<Track, Error> {
        match url.host_str() {
            Some("www.youtube.com" | "youtube.com" | "youtu.be") => {
                // Handle YouTube URL
                // ...
            },
            Some("open.spotify.com") => {
                // Handle Spotify URL
                // ...
            },
            // Handle other domains
            // ...
        }
    }

    pub async fn resolve_file(&self, file: Attachment) -> Result<Track, Error> {
        // Handle file attachment
        // ...
    }
}
```

### Phase 3: Refactor Queue Management

#### Rewrite `queue.rs`

```rust
// src/music/queue.rs
use crate::music::track::Track;
use songbird::{tracks::TrackHandle, Call};
use std::sync::Arc;
use tokio::sync::Mutex;

pub enum QueuePosition {
    Front,
    Next,
    End,
}

pub struct QueueManager {
    call: Arc<Mutex<Call>>,
}

impl QueueManager {
    pub fn new(call: Arc<Mutex<Call>>) -> Self {
        Self { call }
    }

    pub async fn add_track(&self, track: Track, position: QueuePosition) -> Result<TrackHandle, Error> {
        let mut handler = self.call.lock().await;
        
        // Convert Track to songbird Track
        let songbird_track = track.to_songbird_track();
        
        // Add to queue at specified position
        let track_handle = match position {
            QueuePosition::Front => {
                // Add to front logic
                // ...
            },
            QueuePosition::Next => {
                // Add after current track logic
                // ...
            },
            QueuePosition::End => {
                // Add to end logic
                handler.enqueue(songbird_track).await
            },
        };
        
        Ok(track_handle)
    }

    pub async fn get_queue(&self) -> Vec<TrackHandle> {
        let handler = self.call.lock().await;
        handler.queue().current_queue()
    }

    // Other queue management methods
    // ...
}
```

### Phase 4: Refactor Command Handling

#### Rewrite `doplay.rs`

```rust
// src/commands/music/doplay.rs
use crate::music::{
    resolver::TrackResolver,
    queue::{QueueManager, QueuePosition},
    track::TrackSource,
};

// Command definitions remain similar, but implementation is simplified

pub async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // 1. Parse command and determine mode
    let position = parse_mode(mode, ctx.is_prefix());
    
    // 2. Handle resume case if no query
    if query_or_url.is_none() && file.is_none() {
        if ctx.is_paused().await.unwrap_or_default() {
            return resume_internal(ctx).await;
        }
        return send_no_query_error(ctx).await;
    }
    
    // 3. Join voice channel
    let call = get_call_or_join_author(ctx).await?;
    
    // 4. Send search message
    let search_msg = send_search_message(&ctx).await?;
    
    // 5. Resolve query to track
    let resolver = TrackResolver::new(ctx.get_http_client());
    let track = match (query_or_url, file) {
        (Some(query), None) => resolver.resolve_query(&query).await?,
        (None, Some(file)) => resolver.resolve_file(file).await?,
        _ => unreachable!(),
    };
    
    // 6. Add to queue
    let queue_manager = QueueManager::new(call.clone());
    let _track_handle = queue_manager.add_track(track, position).await?;
    
    // 7. Get updated queue and build response
    let queue = queue_manager.get_queue().await;
    let embed = build_play_embed(&queue, position).await?;
    
    // 8. Update UI
    search_msg.edit(ctx, CreateReply::default().embed(embed)).await?;
    
    Ok(())
}
```

## Migration Strategy

1. Implement new modules without removing existing code
2. Gradually migrate functionality to new modules
3. Update command handlers to use new modules
4. Remove deprecated code once migration is complete

## Testing Strategy

1. Write unit tests for new modules
2. Ensure all existing functionality is covered
3. Test edge cases and error handling
4. Perform integration testing with the full bot

## Performance Considerations

1. Minimize queue traversals
2. Optimize metadata handling
3. Reduce redundant API calls
4. Implement caching where appropriate
