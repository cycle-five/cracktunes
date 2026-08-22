use crack_types::{get_valid_token, Duration};
use serenity::all::{
    Cache, GatewayIntents, Http, ShardManager, ShardManagerOptions, TransportCompression,
};
use std::{
    num::NonZeroU16,
    sync::{Arc, OnceLock},
};

pub struct ShardManagerOptionsBuilder(pub ShardManagerOptions);

impl ShardManagerOptionsBuilder {
    pub fn new() -> Self {
        let ws_url = "ws://localhost:3030".to_string();
        let ws_url: Arc<str> = Arc::from(ws_url);
        let token = get_valid_token();
        Self(ShardManagerOptions {
            compression: TransportCompression::None,
            data: Arc::new(crate::Data::default()),
            event_handler: None,
            raw_event_handler: None,
            framework: Arc::new(OnceLock::new()),
            max_concurrency: NonZeroU16::new(1).unwrap(),
            shard_total: NonZeroU16::new(1).unwrap(),
            wait_time_between_shard_start: Duration::from_secs(1),
            cache: Arc::new(Cache::new()),
            http: Arc::new(Http::new(token.clone())),
            token,
            intents: GatewayIntents::all(),
            presence: None,
            voice_manager: None,
            ws_url,
        })
    }

    pub fn build(self) -> ShardManagerOptions {
        self.0
    }
}

/// serenity's `ShardManager::new` now returns the manager itself; the shard
/// monitor channel it used to hand back is internal.
pub struct ShardManagerBuilder(ShardManager);

impl ShardManagerBuilder {
    pub fn new() -> Self {
        Self(ShardManager::new(ShardManagerOptionsBuilder::new().build()))
    }

    pub fn with_opts(opts: ShardManagerOptions) -> Self {
        Self(ShardManager::new(opts))
    }

    pub fn build(self) -> ShardManager {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_shard_manager_opts() {
        let opts = ShardManagerOptionsBuilder::new().build();
        //assert_eq!(opts.shard_index, 0);
        //assert_eq!(opts.shard_init, 0);
        assert_eq!(opts.shard_total, NonZeroU16::new(1).unwrap());
        let ws_url = opts.ws_url.clone();
        assert_eq!(ws_url, "ws://localhost:3030".into());
    }

    #[tokio::test]
    async fn test_create_shard_manager() {
        let shard_manager = ShardManagerBuilder::new().build();
        // `runners` is a DashMap now, not a mutex-guarded map.
        assert_eq!(shard_manager.runners.len(), 0);
    }
}
