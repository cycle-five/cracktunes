# Music Command System Refactoring Roadmap

This document provides a comprehensive roadmap for refactoring the music command system in the `crack-core` crate. It outlines the implementation steps, dependencies, and timeline for the refactoring process.

## Overview

The music command system consists of three main components:

1. **Command Handling** (`doplay.rs`): Handles user commands and orchestrates the overall flow
2. **Query Resolution** (`query.rs`): Resolves user queries to playable tracks
3. **Queue Management** (`queue.rs`): Manages the playback queue

The refactoring aims to address the following issues:

- Code complexity and duplication
- Performance bottlenecks
- Inconsistent error handling
- Redundant data structures
- Unclear separation of concerns

## Implementation Phases

### Phase 1: Core Data Structures (Week 1)

1. Create a new `track.rs` module
   - Implement `TrackMetadata` struct
   - Implement `TrackSource` enum
   - Implement `Track` struct

2. Create a new `resolver.rs` module
   - Implement `TrackResolver` service
   - Implement URL parsing
   - Implement source-specific resolvers

3. Create a new `queue_manager.rs` module
   - Implement `QueuePosition` enum
   - Implement `QueueManager` struct

**Dependencies**: None

**Deliverables**:

- `src/music/track.rs`
- `src/music/resolver.rs`
- `src/music/queue_manager.rs`
- Unit tests for each module

### Phase 2: Command Handling Refactoring (Week 2)

1. Refactor `doplay.rs`
   - Extract helper functions
   - Implement mode parsing
   - Implement UI updates
   - Update command handlers

**Dependencies**: Phase 1

**Deliverables**:

- Refactored `src/commands/music/doplay.rs`
- Unit tests for helper functions

### Phase 3: Query Resolution Refactoring (Week 2-3)

1. Refactor `query.rs`
   - Implement strategy pattern for source types
   - Standardize API usage
   - Implement caching
   - Improve error handling

**Dependencies**: Phase 1

**Deliverables**:

- Refactored `src/music/query.rs`
- Unit tests for query resolution

### Phase 4: Queue Management Refactoring (Week 3)

1. Refactor `queue.rs`
   - Simplify queue position logic
   - Optimize queue traversals
   - Improve error handling

**Dependencies**: Phase 1

**Deliverables**:

- Refactored `src/music/queue.rs`
- Unit tests for queue operations

### Phase 5: Performance Optimizations (Week 4)

1. Implement parallel resolution for playlists
2. Implement lazy loading for playlists
3. Optimize metadata fetching
4. Reduce lock contention
5. Batch process tracks
6. Optimize queue traversals

**Dependencies**: Phases 2-4

**Deliverables**:

- Performance improvements in all modules
- Benchmarks comparing before and after

### Phase 6: Integration and Testing (Week 4-5)

1. Integrate all refactored components
2. Write integration tests
3. Fix any issues that arise
4. Document the new architecture

**Dependencies**: Phases 1-5

**Deliverables**:

- Fully refactored music command system
- Integration tests
- Documentation

## Migration Strategy

To ensure a smooth transition to the refactored code, we'll follow these steps:

1. Implement new modules without removing existing code
2. Create adapter functions that bridge old and new APIs
3. Gradually migrate functionality to new modules
4. Update command handlers to use new modules
5. Remove deprecated code once migration is complete

This approach allows for incremental refactoring, where each step can be tested before moving on to the next.

## Testing Strategy

### Unit Tests

- Test each module in isolation
- Test edge cases and error handling
- Test performance with large inputs

### Integration Tests

- Test the full command flow
- Test with different query types and modes
- Test with different source types (YouTube, Spotify, files)

### Performance Tests

- Benchmark query resolution
- Benchmark queue operations
- Compare before and after refactoring

## Risk Assessment

### Potential Risks

1. **Breaking Changes**: The refactoring may introduce breaking changes to the API
   - Mitigation: Use adapter functions to maintain backward compatibility

2. **Performance Regression**: The refactoring may inadvertently reduce performance
   - Mitigation: Implement benchmarks to compare before and after

3. **Incomplete Migration**: Some parts of the codebase may still use the old API
   - Mitigation: Use compiler warnings to identify usage of deprecated functions

## Conclusion

This refactoring roadmap provides a structured approach to improving the music command system. By following this plan, we can address the current issues while maintaining backward compatibility and ensuring a smooth transition to the new architecture.

The end result will be a more maintainable, performant, and robust music command system that is easier to extend and modify in the future.
