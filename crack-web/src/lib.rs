//! The Crack Tunes web dashboard (arc 1): view a guild's queue live, and
//! reorder it from the bot's voice channel. Runs inside the bot process; see
//! docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md.

pub mod access;
pub mod backend;
pub mod config;
pub mod page;
pub mod routes;
#[cfg(test)]
mod test_support;
pub mod view;
pub mod watch;
