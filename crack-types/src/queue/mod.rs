pub mod queue;
pub mod resolve;
pub use queue::*;
pub use resolve::*;
//------------------------------------
// Constants
//------------------------------------
pub const CREATING: &str = "Creating";
pub const DEFAULT_PLAYLIST_LIMIT: u64 = 50;
pub const EMPTY_QUEUE: &str = "Queue is empty or display not built.";
pub const NEW_FAILED: &str = "New failed";
pub const REQ_CLIENT_STR: &str = "Reqwest client";
pub const UNKNOWN_TITLE: &str = "Unknown title";
pub const UNKNOWN_URL: &str = "";
pub const UNKNOWN_DURATION: &str = "Unknown duration";
pub const YOUTUBE_CLIENT_STR: &str = "YouTube client";
