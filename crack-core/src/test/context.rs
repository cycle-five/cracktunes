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

    /// Get the current builder's options.
    #[must_use]
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
    use super::*;
    use futures::stream::FusedStream;

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
        let (shard_manager, monitor) = ShardManagerBuilder::new().build();
        assert!(!monitor.is_terminated());
        assert_eq!(shard_manager.runners.lock().await.len(), 0);
    }
}
