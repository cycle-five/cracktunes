pub use crack_music::music::{
    audit, audit_view, autoplay, connected, disconnect, lease, ops, perms, remote, search, surface,
    track_data, MusicData, PlaybackOwner, QueueGuard,
};
pub mod context;
pub mod core_surface;
mod data_impl;
pub mod from_ctx;
pub mod query_modes;
pub(crate) mod queue_logged;
pub mod resume;
pub use context::QueryContext;
pub mod queue {
    pub use super::queue_logged::{
        queue_keyword_list_back, queue_query_list_offset, queue_resolved_list_back,
        queue_track_back, queue_track_front,
    };
    pub use crack_music::music::queue::{
        _queue_track_ready_back, build_track, enqueue, enqueue_input_back,
        enqueue_resolved_tracks_back, enqueue_track_back, force_skip_top_track, get_mode, get_msg,
        new_track, pause_queue, play_beside_queue, preload_from_metadata, preload_time,
        queue_resolved_track_back, queue_track_ready_front, record_add, remove_at, stop_queue,
        QueuedTrack, TrackReadyData,
    };
}
pub mod query {
    pub use super::query_modes::query_type_from_url;
    pub use crack_music::music::query::{
        play_all_plan, spotify_query, unresolved_spotify_link, ListingShortfall, NewQueryType,
        PlayAll, Queries, ResolvedQuery, DEFAULT_PLAYLIST_LIMIT,
    };
}
pub(crate) use query::NewQueryType;
pub(crate) use queue::TrackReadyData;
