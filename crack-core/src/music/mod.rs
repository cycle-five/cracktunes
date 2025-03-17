// Old stuff
pub mod query;
pub mod queue;
// Re-export existing modules
pub use super::music::query::*;
pub use super::music::queue::*;

// Export new modules
pub mod queue_manager;
pub mod resolver;
pub mod track;

// Re-export key types
pub use queue_manager::{QueueManager, QueuePosition};
pub use resolver::TrackResolver;
pub use track::{Track, TrackCollection, TrackMetadata, TrackSource};
