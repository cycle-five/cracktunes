//! The `/gp` game, its saved form, its prompts and the table it is saved in.
//!
//! Never depends on poise or crack-core: crack-core re-exports `gp`,
//! `gp_persist`, `gp_prompts` and the `db::gp` table at their old paths.

pub mod db;
pub mod gp;
pub mod gp_persist;
pub mod gp_prompts;

#[cfg(test)]
pub(crate) use crack_test_db::test_db;
