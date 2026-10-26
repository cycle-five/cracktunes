//! Per-server credits: earned in voice and by chatting, spent in the shop.
//! Off by default; see `economy::settings`.
//!
//! The economy itself lives in the `crack-economy` crate, re-exported here so
//! every old `crate::economy::…` path keeps working. What stays in this crate
//! is the glue that needs poise, songbird or the rooms: the shop's buttons,
//! theme playback (`walk_on`) and the target facts (`facts`).

pub use crack_economy::*;

pub mod buttons;
pub mod facts;
pub mod walk_on;
