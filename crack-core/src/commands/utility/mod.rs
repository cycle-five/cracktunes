pub mod invite;
pub mod ping;
pub mod version;

pub use invite::*;
pub use ping::*;
pub use version::*;

/// Get all the utility commands.
pub fn utility_commands() -> [crate::Command; 3] {
    [invite(), ping(), version()]
}
