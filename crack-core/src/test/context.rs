use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};

use serenity::all::{Cache, GatewayIntents, Http, ShardManagerOptions};
use serenity::prelude::TypeMap;

pub struct ShardManagerOptionsBuilder(ShardManagerOptions);

impl ShardManagerOptionsBuilder {
    pub fn new() -> Self {
        Self(ShardManagerOptions {
            data: Arc::new(RwLock::new(TypeMap::new())),
            event_handlers: vec![],
            raw_event_handlers: vec![],
            framework: Arc::new(OnceLock::new()),
            shard_index: 0,
            shard_init: 0,
            shard_total: 1,
            ws_url: Arc::new(Mutex::new("ws://localhost:3030".to_string())),
            cache: Arc::new(Cache::new()),
            http: Arc::new(Http::new("")),
            intents: GatewayIntents::all(),
            presence: None,
            voice_manager: None,
        })
    }

    pub fn shard_index(mut self, shard_index: u32) -> Self {
        self.0.shard_index = shard_index;
        self
    }

    pub fn shard_init(mut self, shard_init: u32) -> Self {
        self.0.shard_init = shard_init;
        self
    }

    pub fn shard_total(mut self, shard_total: u32) -> Self {
        self.0.shard_total = shard_total;
        self
    }

    pub fn ws_url(mut self, ws_url: String) -> Self {
        self.0.ws_url = Arc::new(Mutex::new(ws_url));
        self
    }

    pub fn build(self) -> ShardManagerOptions {
        self.0
    }
}

mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_shard_manager_opts() {
        let opts = ShardManagerOptionsBuilder::new().build();
        assert_eq!(opts.shard_index, 0);
        assert_eq!(opts.shard_init, 0);
        assert_eq!(opts.shard_total, 1);
        let ws_url = opts.ws_url.lock().await.clone();
        assert_eq!(ws_url, "ws://localhost:3030".to_string());
    }
}
