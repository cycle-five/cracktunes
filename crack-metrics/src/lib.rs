//! Prometheus metrics for Crack Tunes.
//!
//! This is the only crate that writes a metric name or calls a `metrics` macro.
//! The rest of the bot calls the event functions in [`events`] with typed labels
//! from [`labels`]; [`spawn_if_configured`] starts the `/metrics` listener when
//! `METRICS_BIND` is set.

pub mod events;
pub mod labels;
pub mod names;
mod serve;

pub use events::{
    autocomplete_finished, command_failed, command_finished, command_started, gateway_event,
    guild_playback_seconds, guild_voice_connected_seconds, proxy_fallback, search_fallback,
    set_gateway_connected, set_gateway_latency, set_guild_info, set_guilds, set_queued_tracks,
    set_tracks_playing, set_voice_connections, sleevenote_request, suggestion_label, track_ended,
    track_failed, track_resolved, track_started,
};
pub use labels::{
    CommandKind, EndOutcome, FailureReason, GatewayEvent, ProxyOutcome, RequestOutcome,
    ResolveOutcome, SearchFallbackOutcome, SearchFallbackReason, Source, SuggestionLabelOutcome,
};
pub use serve::{serve, spawn_if_configured, ServeError, BIND_ENV};
