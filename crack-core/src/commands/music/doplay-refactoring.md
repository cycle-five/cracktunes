# `doplay.rs` Refactoring Plan

## Current Issues

1. **Complexity**: The `play_internal` function is over 200 lines long with many responsibilities
2. **Duplication**: Similar code paths for different query types
3. **Performance**: Multiple queue traversals and inefficient metadata handling
4. **Error Handling**: Inconsistent error handling patterns
5. **Maintainability**: Difficult to understand and modify

## Step-by-Step Refactoring Plan

### Step 1: Extract Helper Functions

Break down the large `play_internal` function into smaller, focused helper functions:

```rust
// Before refactoring
pub async fn play_internal(
    ctx: Context<'_>,
    mode: Option<String>,
    file: Option<serenity::Attachment>,
    query_or_url: Option<String>,
) -> Result<(), Error> {
    // 200+ lines of code with multiple responsibilities
}

// After refactoring
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
        return handle_empty_query(ctx).await;
    }
    
    // 3. Join voice channel
    let call = get_call_or_join_author(ctx).await?;
    
    // 4. Send search message
    let search_msg = send_search_message(&ctx).await?;
    
    // 5. Resolve query to track
    let track = resolve_query(ctx, query_or_url, file).await?;
    
    // 6. Add to queue
    let track_handle = add_to_queue(call.clone(), track, position).await?;
    
    // 7. Update UI
    update_ui(ctx, call, track_handle, position, search_msg).await?;
    
    Ok(())
}
```

### Step 2: Implement Mode Parsing

Create a clean mode parsing function that converts string options to a proper enum:

```rust
// Define a proper enum for queue positions
pub enum QueuePosition {
    Front,
    Next,
    End,
    Jump,
    Search,
    Download(bool), // bool indicates mp3 vs mkv
}

// Parse mode from command options
fn parse_mode(mode_str: Option<String>, is_prefix: bool) -> QueuePosition {
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

### Step 3: Implement Query Resolution

Create a dedicated resolver that handles all query types:

```rust
async fn resolve_query(
    ctx: Context<'_>,
    query_or_url: Option<String>,
    file: Option<serenity::Attachment>,
) -> Result<Track, Error> {
    let resolver = TrackResolver::new(ctx.get_http_client());
    
    match (query_or_url, file) {
        (Some(query), None) => {
            let mut track = resolver.resolve_query(&query).await?;
            track.user_id = Some(ctx.author().id);
            track
        },
        (None, Some(file)) => {
            let mut track = resolver.resolve_file(file).await?;
            track.user_id = Some(ctx.author().id);
            track
        },
        _ => unreachable!(),
    }
}
```

### Step 4: Implement Queue Management

Create a dedicated queue manager that handles all queue operations:

```rust
async fn add_to_queue(
    call: Arc<Mutex<Call>>,
    track: Track,
    position: QueuePosition,
) -> Result<TrackHandle, Error> {
    let queue_manager = QueueManager::new(call);
    let resolver = TrackResolver::new(http_utils::get_client().clone());
    
    queue_manager.add_track(track, position, &resolver).await
}
```

### Step 5: Implement UI Updates

Create a dedicated function for building and updating the UI:

```rust
async fn update_ui(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    _track_handle: TrackHandle,
    position: QueuePosition,
    search_msg: ReplyHandle<'_>,
) -> Result<(), Error> {
    let queue = {
        let handler = call.lock().await;
        handler.queue().current_queue()
    };
    
    let embed = build_play_embed(&queue, &position).await?;
    
    search_msg
        .edit(ctx, CreateReply::default().embed(embed))
        .await?;
    
    Ok(())
}
```

### Step 6: Implement Embed Building

Create a dedicated function for building the play embed:

```rust
async fn build_play_embed(
    queue: &[TrackHandle],
    position: &QueuePosition,
) -> Result<CreateEmbed<'static>, Error> {
    // Implementation similar to current build_play_embed but simplified
    // ...
}
```

## Refactoring the Command Handlers

The command handlers (`play`, `playnext`, `search`, etc.) should be simplified to just call the `play_internal` function with the appropriate parameters:

```rust
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("p", "P"),
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn play(
    ctx: Context<'_>,
    #[rest]
    #[description = "song link or search query."]
    #[autocomplete = "autocomplete"]
    query: String,
) -> Result<(), Error> {
    // Split off the first part of the query
    let query = query.split('~').next().unwrap_or_default().to_string();
    play_internal(ctx, None, None, Some(query)).await
}
```

## Refactoring the Mode Handling

Replace the current `match_mode` function with a cleaner approach that uses the `QueuePosition` enum:

```rust
async fn handle_queue_position(
    ctx: Context<'_>,
    call: Arc<Mutex<Call>>,
    track: Track,
    position: QueuePosition,
    search_msg: ReplyHandle<'_>,
) -> Result<(), Error> {
    let queue_manager = QueueManager::new(call.clone());
    let resolver = TrackResolver::new(ctx.get_http_client());
    
    match position {
        QueuePosition::Front => {
            queue_manager.add_track(track, QueuePosition::Front, &resolver).await?;
        },
        QueuePosition::Next => {
            queue_manager.add_track(track, QueuePosition::Next, &resolver).await?;
        },
        QueuePosition::End => {
            queue_manager.add_track(track, QueuePosition::End, &resolver).await?;
        },
        QueuePosition::Search => {
            // Handle search mode
            // ...
        },
        QueuePosition::Download(mp3) => {
            // Handle download mode
            // ...
        },
        QueuePosition::Jump => {
            // Handle jump mode
            // ...
        },
    }
    
    Ok(())
}
```

## Performance Optimizations

1. **Reduce Queue Traversals**:
   - Only fetch the queue once after all modifications
   - Use more efficient queue operations

2. **Optimize Metadata Handling**:
   - Cache metadata where possible
   - Only fetch metadata when needed

3. **Reduce Redundant API Calls**:
   - Consolidate API calls for track resolution
   - Implement proper caching

## Error Handling Improvements

1. **Standardize Error Handling**:
   - Use consistent error types
   - Provide meaningful error messages

2. **Implement Recovery Paths**:
   - Handle common failure scenarios gracefully
   - Provide fallback options where appropriate

## Testing Strategy

1. **Unit Tests**:
   - Test each helper function in isolation
   - Test edge cases and error handling

2. **Integration Tests**:
   - Test the full command flow
   - Test with different query types and modes

## Implementation Order

1. Create the new data structures (`Track`, `TrackMetadata`, `QueuePosition`)
2. Implement the helper functions
3. Refactor `play_internal` to use the helper functions
4. Update the command handlers
5. Implement performance optimizations
6. Improve error handling
7. Add tests

This approach allows for incremental refactoring, where each step can be tested before moving on to the next.
