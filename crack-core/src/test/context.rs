use futures::StreamExt;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};

use serenity::all::{Cache, GatewayIntents, Http, ShardManager, ShardManagerOptions};
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

use futures::channel::mpsc::UnboundedReceiver;
use serenity::all::GatewayError;
pub struct ShardManagerBuilder(
    Arc<ShardManager>,
    UnboundedReceiver<Result<(), GatewayError>>,
);

impl ShardManagerBuilder {
    pub fn new() -> Self {
        let (manager, res) = ShardManager::new(ShardManagerOptionsBuilder::new().build());
        Self(manager, res)
    }

    pub fn with_opts(opts: ShardManagerOptions) -> Self {
        let (manager, res) = ShardManager::new(opts);
        Self(manager, res)
    }

    pub fn build(
        self,
    ) -> (
        Arc<ShardManager>,
        UnboundedReceiver<Result<(), GatewayError>>,
    ) {
        (self.0, self.1)
    }
}

#[cfg(test)]
mod tests {
    use futures::stream::FusedStream;
    use futures::StreamExt;

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

    #[tokio::test]
    async fn test_create_shard_manager() {
        let (shard_manager, monitor) = ShardManagerBuilder::new().build();
        assert!(!monitor.is_terminated());
        assert!(shard_manager.runners.lock().await.len() > 0);
    }
}
