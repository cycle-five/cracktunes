pub mod audit;
pub mod audit_view;
pub mod autoplay;
#[cfg(test)]
mod autoplay_probe;
pub mod connected;
pub mod disconnect;
pub mod lease;
pub mod ops;
pub mod perms;
pub mod query;
pub(crate) use query::NewQueryType;
pub mod queue;
pub mod remote;
pub mod search;
pub mod surface;
pub mod track_data;

pub use lease::{PlaybackOwner, QueueGuard};

/// The music player's state, without `Data`: the pool, songbird, the lease
/// maps and the rest the player reads. `Data::music_data` builds one; the maps
/// are shared with `Data`, not copied.
#[derive(Clone)]
pub struct MusicData {
    pub database_pool: Option<sqlx::PgPool>,
    pub songbird: std::sync::Arc<songbird::Songbird>,
    pub ct_client: crack_testing::CrackTrackClient<'static>,
    pub http_client: reqwest::Client,
    pub audit_tx: Option<tokio::sync::mpsc::Sender<audit::AuditEvent>>,
    pub db_channel: Option<tokio::sync::mpsc::Sender<crate::db::MetadataMsg>>,
    pub autoplay_buffer: std::sync::Arc<autoplay::AutoplayBuffer>,
    pub guild_cache_map: std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<serenity::all::GuildId, crack_guild::cache::GuildCache>,
        >,
    >,
    pub playback_owners:
        std::sync::Arc<dashmap::DashMap<serenity::all::GuildId, lease::PlaybackOwner>>,
    pub queue_locks: std::sync::Arc<
        dashmap::DashMap<serenity::all::GuildId, std::sync::Arc<tokio::sync::Mutex<()>>>,
    >,
    pub guild: crack_guild::operations::GuildData,
    pub surface: std::sync::Arc<dyn surface::Surface>,
}

impl std::fmt::Debug for MusicData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicData")
            .field("database_pool", &self.database_pool)
            .field("guild", &self.guild)
            .finish_non_exhaustive()
    }
}
