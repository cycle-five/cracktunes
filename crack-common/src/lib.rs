//! Errors, user-facing strings and platform-neutral messages.
//!
//! Never depends on poise or crack-core: crack-core re-exports each module at
//! its old path.
pub mod cards;
pub mod courier;
pub mod discord;
pub mod errors;
pub mod failure;
pub mod format;
pub mod message;
pub mod messages;
pub mod perms;
pub mod render;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod throttle;
pub mod transport;

pub type CrackedResult<T> = std::result::Result<T, errors::CrackedError>;
pub type CrackedHowResult<T> = anyhow::Result<T, errors::CrackedError>;
