//! "What's your song?" party game (`/gp`).
//!
//! Each round the bot posts a prompt from [`gp_prompts`](super::gp_prompts) and opens a timed
//! window for everyone in the voice channel to secretly submit a song. The
//! round's songs then play back-to-back: guess the submitter from a dropdown,
//! 👍 the ones you like, and the submitter is revealed when the song ends.
//! Scores live in memory for the duration of the game, and are written to
//! Postgres at each submission and each song's end so a restart does not end
//! the game -- see [`gp_persist`](super::gp_persist).

mod commands;
mod data_impl;
mod playback;

pub use commands::gp;
pub use crack_gp::gp::{
    gp_post, now, GpCard, GpClip, GpData, GpGame, GpPhase, GpPlayed, GpReveal, GpSubmitOutcome,
    GpTrackStart, GpWindowOpened, GP_BLOCKED_COMMANDS, GP_CUSTOM_ID_PREFIX, GP_DEAD_LINK,
    GP_MIN_PLAYED, GP_PARK_GRACE_SECS, GP_RESUME_WINDOW_SECS,
};
pub use playback::{
    gp_advance_track, gp_after_close, gp_open_round, gp_play_track, gp_spawn_window_timer_secs,
    handle_gp_component, GpPlayback,
};

#[cfg(test)]
mod test_delivery;
#[cfg(test)]
mod test_glue;
