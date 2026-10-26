//! The music player, its sources and the tables it reads and writes.
//!
//! Never depends on poise or crack-core: crack-core re-exports `music`,
//! `sources`, `http_utils` and the `db` tables at their old paths.

pub mod db;
pub mod http_utils;
pub mod music;
pub mod sources;

#[cfg(test)]
pub(crate) use crack_test_db::test_db;
