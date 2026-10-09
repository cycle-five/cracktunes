//! "What's your song?" party game (`/gp`).
//!
//! Each round the bot posts a prompt from [`gp_prompts`] and opens a timed
//! window for everyone in the voice channel to secretly submit a song. The
//! round's songs then play back-to-back: guess the submitter from a dropdown,
//! 👍 the ones you like, and the submitter is revealed when the song ends.
//! Scores live in memory for the duration of the game, and are written to
//! Postgres at each submission and each song's end so a restart does not end
//! the game -- see [`gp_persist`](super::gp_persist).

mod commands;
mod playback;
mod state;
mod ui;

pub use commands::*;
pub use playback::*;
pub use state::*;
pub use ui::*;

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_delivery;
