//! The voice rooms, the mod log and their queries.
//!
//! Never depends on poise or crack-core: crack-core re-exports `rooms`,
//! `modlog` and `db::{rooms,modlog}` at their old paths.

pub mod db;
pub mod modlog;
pub mod rooms;

#[cfg(test)]
pub(crate) use crack_test_db::test_db;
