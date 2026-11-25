//! Every metric name in one place. No other crate writes a metric name string.

// Process and gateway state.
pub const BUILD_INFO: &str = "cracktunes_build_info";
pub const GATEWAY_CONNECTED: &str = "cracktunes_gateway_connected";
pub const GATEWAY_LATENCY: &str = "cracktunes_gateway_latency_seconds";
pub const GATEWAY_EVENTS: &str = "cracktunes_gateway_events_total";
pub const GUILDS: &str = "cracktunes_guilds";
pub const VOICE_CONNECTIONS: &str = "cracktunes_voice_connections";
pub const TRACKS_PLAYING: &str = "cracktunes_tracks_playing";
pub const QUEUED_TRACKS: &str = "cracktunes_queued_tracks";

// Commands.
pub const COMMANDS: &str = "cracktunes_commands_total";
pub const COMMAND_ERRORS: &str = "cracktunes_command_errors_total";
pub const COMMAND_DURATION: &str = "cracktunes_command_duration_seconds";
pub const AUTOCOMPLETE_DURATION: &str = "cracktunes_autocomplete_duration_seconds";

// Tracks.
pub const TRACK_RESOLVES: &str = "cracktunes_track_resolves_total";
pub const TRACK_RESOLVE_DURATION: &str = "cracktunes_track_resolve_duration_seconds";
pub const TRACK_FAILURES: &str = "cracktunes_track_failures_total";
pub const TRACKS_STARTED: &str = "cracktunes_tracks_started_total";
pub const TRACKS_ENDED: &str = "cracktunes_tracks_ended_total";

// Per guild (the only metrics that carry `guild_id`).
pub const GUILD_INFO: &str = "cracktunes_guild_info";
pub const GUILD_COMMANDS: &str = "cracktunes_guild_commands_total";
pub const GUILD_TRACKS_STARTED: &str = "cracktunes_guild_tracks_started_total";
pub const GUILD_PLAYBACK_SECONDS: &str = "cracktunes_guild_playback_seconds_total";
pub const GUILD_VOICE_CONNECTED_SECONDS: &str = "cracktunes_guild_voice_connected_seconds_total";

// Fallbacks and upstream services.
pub const PROXY_FALLBACK: &str = "cracktunes_proxy_fallback_total";
pub const PROXY_FALLBACK_ATTEMPTS: &str = "cracktunes_proxy_fallback_attempts_total";
pub const SEARCH_FALLBACK: &str = "cracktunes_search_fallback_total";
pub const SLEEVENOTE_REQUESTS: &str = "cracktunes_sleevenote_requests_total";
pub const SLEEVENOTE_RETRIES: &str = "cracktunes_sleevenote_retries_total";
pub const SLEEVENOTE_REQUEST_DURATION: &str = "cracktunes_sleevenote_request_duration_seconds";
pub const SUGGESTION_LABEL: &str = "cracktunes_suggestion_label_total";

/// Histogram buckets, in seconds.
pub const COMMAND_BUCKETS: [f64; 9] = [0.1, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 30.0];
pub const AUTOCOMPLETE_BUCKETS: [f64; 7] = [0.1, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0];
pub const SLEEVENOTE_BUCKETS: [f64; 7] = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0];
