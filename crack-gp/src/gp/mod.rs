//! "What's your song?" party game (`/gp`).
//!
//! Each round the bot posts a prompt from [`gp_prompts`](crate::gp_prompts) and opens a timed
//! window for everyone in the voice channel to secretly submit a song. The
//! round's songs then play back-to-back: guess the submitter from a dropdown,
//! 👍 the ones you like, and the submitter is revealed when the song ends.
//! Scores live in memory for the duration of the game, and are written to
//! Postgres at each submission and each song's end so a restart does not end
//! the game -- see [`gp_persist`](crate::gp_persist).

mod state;
mod ui;

#[cfg(any(test, feature = "test-support"))]
pub use state::GP_STALLING_COMMANDS;
pub use state::{
    gp_min_played, now, GpClip, GpData, GpGame, GpGuessOutcome, GpLikeOutcome, GpNext, GpPhase,
    GpPlayed, GpReveal, GpRound, GpRoundResult, GpSongResult, GpStatus, GpSubmitOutcome, GpTrack,
    GpTrackResult, GpTrackStart, GpVoteFullOutcome, GpVoteSkipOutcome, GpWindowClosed,
    GpWindowOpened, GpWindowWarning, GP_BLOCKED_COMMANDS, GP_CUSTOM_ID_PREFIX, GP_DEAD_LINK,
    GP_DEFAULT_CLIPS, GP_DEFAULT_CLIP_LENGTH_SECS, GP_DEFAULT_CLIP_START_SECS, GP_DEFAULT_ROUNDS,
    GP_DEFAULT_TIMER_SECS, GP_MAX_CLIP_LENGTH_SECS, GP_MAX_CLIP_START_SECS, GP_MAX_ROUNDS,
    GP_MAX_TIMER_SECS, GP_MIN_CLIP_LENGTH_SECS, GP_MIN_PLAYED, GP_MIN_TIMER_SECS,
    GP_PARK_GRACE_SECS, GP_PICK_CANCEL_ID, GP_PICK_START_ID, GP_PICK_TIMEOUT_SECS,
    GP_RESUME_WINDOW_SECS, GP_REVEAL_PAUSE_SECS, GP_WARNING_SECS,
};
pub use ui::{
    gp_picked, gp_post, gp_rendered, gp_warning_text, parse_custom_id, user_refs, GpCard,
    GpClosedView, GpComponent, GpPromptView, GpRevealView, GpRoundView, GpSongRow, GpSongView,
    GpStatusView,
};

// Names only the tests under this module reach, through `use super::*`.
#[cfg(test)]
use state::{
    gp_votes_required, shuffle_against, GpTrackScore, GP_MAX_PLAYERS, GP_PICK_MENU_ID,
    GP_POINTS_CORRECT, GP_POINTS_FOOLED_ALL, GP_POINTS_FULL_SONG, GP_POINTS_PER_LIKE,
};
#[cfg(test)]
use ui::{
    gp_components, gp_custom_id, gp_pick_components, gp_pick_embed, gp_prompt_closed_embed,
    gp_prompt_embed, gp_reveal_embed, gp_round_results_embed, gp_rules_embed, gp_scoreboard_embed,
    gp_status_embed, gp_track_embed, render_card, GP_TITLE_MAX,
};

#[cfg(test)]
mod test;
#[cfg(test)]
mod test_delivery;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
