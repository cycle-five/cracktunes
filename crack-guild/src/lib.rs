//! Guild settings, the per-guild cache, the plan and their queries.
//!
//! Never depends on poise or crack-core: crack-core re-exports `cache`,
//! `operations`, `permissions`, `plan`, `settings` and `db::guild` at their
//! old paths.

pub mod cache;
pub mod db;
pub mod operations;
pub mod permissions;
pub mod plan;
pub mod settings;

#[cfg(test)]
pub(crate) use crack_test_db::test_db;
