# `query.rs` Refactoring Plan

## Current Issues

1. **Complex Query Type Handling**: The `NewQueryType` struct and its methods are overly complex
2. **Inconsistent API Usage**: Inconsistent use of different YouTube/media source APIs
3. **Redundant Code**: Multiple similar code paths for different query types
4. **Error Handling**: Inconsistent error handling patterns
5. **Performance Concerns**: Inefficient query resolution process

## Step-by-Step Refactoring Plan

### Step 1: Simplify Query Type Representation

Replace the current `NewQueryType` wrapper with a cleaner enum-based approach:

```rust
// Current approach
#[derive(Clone, Debug)]
pub struct NewQueryType(pub crack_types::QueryType);

// New approach
#[derive(Clone, Debug)]
pub enum QuerySource {
    YouTube {
        url: String,
        is_playlist: bool,
    },
    Spotify(String),
    Search(String),
    File(serenity::all::Attachment),
    // Other source types
}

#[derive(Clone, Debug)]
pub struct Query {
    pub source: QuerySource,
    pub user_id: Option<serenity::all::UserId>,
}

impl Query {
    pub fn new(source: QuerySource) -> Self {
        Self {
            source,
            user_id: None,
        }
    }

    pub fn with_user_id(mut self, user_id: serenity::all::UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }
}
```

### Step 2: Create a TrackResolver Service

Create a dedicated `TrackResolver` service to handle query resolution:

```rust
pub struct TrackResolver {
    client: reqwest::Client,
}

impl TrackResolver {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    // Parse a query string into a Query object
    pub async fn parse_query(&self, query_str: &str) -> Result<Query, Error> {
        // Implementation
    }

    // Parse a file attachment into a Query object
    pub async fn parse_file(&self, file: serenity::all::Attachment) -> Result<Query, Error> {
        Ok(Query::new(QuerySource::File(file)))
    }

    // Resolve a Query to a Track
    pub async fn resolve(&self, query: Query) -> Result<Track, Error> {
        match query.source {
            QuerySource::YouTube { url, is_playlist } => {
                if is_playlist {
                    self.resolve_youtube_playlist(&url).await
                } else {
                    self.resolve_youtube_video(&url).await
                }
            },
            QuerySource::Spotify(url) => self.resolve_spotify(&url).await,
            QuerySource::Search(query) => self.resolve_search(&query).await,
            QuerySource::File(file) => self.resolve_file(file).await,
            // Handle other source types
        }
    }

    // Resolve a YouTube video URL to a Track
    async fn resolve_youtube_video(&self, url: &str) -> Result<Track, Error> {
        // Implementation
    }

    // Resolve a YouTube playlist URL to multiple Tracks
    async fn resolve_youtube_playlist(&self, url: &str) -> Result<Vec<Track>, Error> {
        // Implementation
    }

    // Resolve a Spotify URL to a Track or Tracks
    async fn resolve_spotify(&self, url: &str) -> Result<Vec<Track>, Error> {
        // Implementation
    }

    // Resolve a search query to a Track
    async fn resolve_search(&self, query: &str) -> Result<Track, Error> {
        // Implementation
    }

    // Resolve a file attachment to a Track
    async fn resolve_file(&self, file: serenity::all::Attachment) -> Result<Track, Error> {
        // Implementation
    }
}
```

### Step 3: Implement Strategy Pattern for Source Types

Use the strategy pattern to handle different source types:

```rust
// Source resolver trait
trait SourceResolver {
    async fn resolve(&self, query: &str, client: &reqwest::Client) -> Result<Track, Error>;
}

// YouTube resolver
struct YouTubeResolver;
impl SourceResolver for YouTubeResolver {
    async fn resolve(&self, url: &str, client: &reqwest::Client) -> Result<Track, Error> {
        // Implementation
    }
}

// Spotify resolver
struct SpotifyResolver;
impl SourceResolver for SpotifyResolver {
    async fn resolve(&self, url: &str, client: &reqwest::Client) -> Result<Track, Error> {
        // Implementation
    }
}

// Search resolver
struct SearchResolver;
impl SourceResolver for SearchResolver {
    async fn resolve(&self, query: &str, client: &reqwest::Client) -> Result<Track, Error> {
        // Implementation
    }
}

// File resolver
struct FileResolver;
impl SourceResolver for FileResolver {
    async fn resolve(&self, url: &str, client: &reqwest::Client) -> Result<Track, Error> {
        // Implementation
    }
}

// Factory for creating resolvers
struct ResolverFactory;
impl ResolverFactory {
    fn get_resolver(source: &QuerySource) -> Box<dyn SourceResolver> {
        match source {
            QuerySource::YouTube { .. } => Box::new(YouTubeResolver),
            QuerySource::Spotify(_) => Box::new(SpotifyResolver),
            QuerySource::Search(_) => Box::new(SearchResolver),
            QuerySource::File(_) => Box::new(FileResolver),
            // Other source types
        }
    }
}
```

### Step 4: Standardize API Usage

Standardize on a single API for each source type:

```rust
// YouTube API wrapper
struct YouTubeAPI {
    client: reqwest::Client,
}

impl YouTubeAPI {
    fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    async fn get_video(&self, url: &str) -> Result<Track, Error> {
        // Implementation using rusty_ytdl
    }

    async fn get_playlist(&self, url: &str) -> Result<Vec<Track>, Error> {
        // Implementation using rusty_ytdl
    }

    async fn search(&self, query: &str) -> Result<Track, Error> {
        // Implementation using rusty_ytdl
    }
}

// Spotify API wrapper
struct SpotifyAPI {
    client: reqwest::Client,
}

impl SpotifyAPI {
    fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    async fn get_track(&self, url: &str) -> Result<Track, Error> {
        // Implementation
    }

    async fn get_playlist(&self, url: &str) -> Result<Vec<Track>, Error> {
        // Implementation
    }
}
```

### Step 5: Improve URL Parsing

Create a dedicated URL parser to handle different URL formats:

```rust
struct URLParser;

impl URLParser {
    fn parse(url_str: &str) -> Result<QuerySource, Error> {
        use url::Url;
        
        if let Ok(url) = Url::parse(url_str) {
            match url.host_str() {
                Some("www.youtube.com" | "youtube.com" | "youtu.be") => {
                    let is_playlist = url.query_pairs().any(|(k, _)| k == "list");
                    Ok(QuerySource::YouTube {
                        url: url_str.to_string(),
                        is_playlist,
                    })
                },
                Some("open.spotify.com") => {
                    Ok(QuerySource::Spotify(url_str.to_string()))
                },
                // Handle other domains
                _ => {
                    // Default to YouTube for unknown URLs
                    Ok(QuerySource::YouTube {
                        url: url_str.to_string(),
                        is_playlist: false,
                    })
                },
            }
        } else {
            // Not a URL, treat as search query
            Ok(QuerySource::Search(url_str.to_string()))
        }
    }
}
```

### Step 6: Implement Caching

Add caching to improve performance:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::{Duration, Instant};

struct QueryCache {
    cache: Arc<Mutex<HashMap<String, (Instant, Track)>>>,
    ttl: Duration,
}

impl QueryCache {
    fn new(ttl: Duration) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            ttl,
        }
    }

    async fn get(&self, key: &str) -> Option<Track> {
        let mut cache = self.cache.lock().await;
        
        if let Some((timestamp, track)) = cache.get(key) {
            if timestamp.elapsed() < self.ttl {
                return Some(track.clone());
            }
            // Expired, remove from cache
            cache.remove(key);
        }
        
        None
    }

    async fn set(&self, key: String, track: Track) {
        let mut cache = self.cache.lock().await;
        cache.insert(key, (Instant::now(), track));
    }
}
```

### Step 7: Improve Error Handling

Standardize error handling across all query operations:

```rust
#[derive(Debug, Error)]
pub enum QueryError {
    #[error("Failed to parse URL: {0}")]
    URLParseError(String),
    
    #[error("Failed to resolve YouTube video: {0}")]
    YouTubeError(String),
    
    #[error("Failed to resolve Spotify track: {0}")]
    SpotifyError(String),
    
    #[error("Failed to resolve search query: {0}")]
    SearchError(String),
    
    #[error("Failed to resolve file: {0}")]
    FileError(String),
    
    // Other error variants
}

impl From<QueryError> for Error {
    fn from(err: QueryError) -> Self {
        // Convert to general error type
    }
}
```

## Performance Optimizations

### 1. Implement Parallel Resolution for Playlists

Process playlist tracks in parallel:

```rust
async fn resolve_playlist_parallel(&self, url: &str) -> Result<Vec<Track>, Error> {
    let playlist = rusty_ytdl::search::Playlist::get(url.to_string(), None).await?;
    
    let futures = playlist.videos.into_iter()
        .map(|video| self.resolve_youtube_video(&video.url));
    
    let results = futures::future::join_all(futures).await;
    
    let tracks = results.into_iter()
        .filter_map(Result::ok)
        .collect();
    
    Ok(tracks)
}
```

### 2. Implement Lazy Loading for Playlists

Only load metadata for the first few tracks in a playlist initially:

```rust
async fn resolve_playlist_lazy(&self, url: &str, limit: usize) -> Result<LazyPlaylist, Error> {
    let playlist = rusty_ytdl::search::Playlist::get(url.to_string(), None).await?;
    
    let initial_tracks = playlist.videos.iter()
        .take(limit)
        .map(|video| self.resolve_youtube_video(&video.url))
        .collect::<Vec<_>>();
    
    let initial_tracks = futures::future::join_all(initial_tracks).await
        .into_iter()
        .filter_map(Result::ok)
        .collect();
    
    let remaining_urls = playlist.videos.iter()
        .skip(limit)
        .map(|video| video.url.clone())
        .collect();
    
    Ok(LazyPlaylist {
        initial_tracks,
        remaining_urls,
        resolver: self.clone(),
    })
}

struct LazyPlaylist {
    initial_tracks: Vec<Track>,
    remaining_urls: Vec<String>,
    resolver: TrackResolver,
}

impl LazyPlaylist {
    async fn load_more(&mut self, count: usize) -> Result<Vec<Track>, Error> {
        let urls_to_load = self.remaining_urls.drain(..count.min(self.remaining_urls.len())).collect::<Vec<_>>();
        
        let futures = urls_to_load.into_iter()
            .map(|url| self.resolver.resolve_youtube_video(&url));
        
        let results = futures::future::join_all(futures).await;
        
        let tracks = results.into_iter()
            .filter_map(Result::ok)
            .collect();
        
        Ok(tracks)
    }
}
```

### 3. Optimize Metadata Fetching

Only fetch metadata when needed:

```rust
async fn resolve_with_minimal_metadata(&self, url: &str) -> Result<Track, Error> {
    // Implementation that only fetches essential metadata
}

async fn fetch_full_metadata(&self, track: &mut Track) -> Result<(), Error> {
    // Implementation that fetches full metadata
}
```

## Testing Strategy

1. **Unit Tests**:
   - Test URL parsing
   - Test query resolution for different source types
   - Test error handling

2. **Integration Tests**:
   - Test with different URL formats
   - Test with different query types
   - Test performance with large playlists

## Implementation Order

1. Create the new data structures (`Query`, `QuerySource`, `TrackResolver`)
2. Implement URL parsing
3. Implement source-specific resolvers
4. Implement caching
5. Improve error handling
6. Implement performance optimizations
7. Add tests

This approach allows for incremental refactoring, where each step can be tested before moving on to the next.
