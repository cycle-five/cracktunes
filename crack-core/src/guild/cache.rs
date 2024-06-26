use self::serenity::model::{
    channel::Message,
    id::{GuildId, UserId},
};
use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;
use std::collections::BTreeMap;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;
use typemap_rev::TypeMapKey;

type QueueMessage = (Message, Arc<RwLock<usize>>);

#[derive(Debug, Clone)]
pub struct GuildCache {
    pub autoplay: bool,
    pub time_ordered_messages: BTreeMap<DateTime<Utc>, Message>,
    pub queue_messages: Vec<QueueMessage>,
    pub current_skip_votes: HashSet<UserId>,
}

impl Default for GuildCache {
    fn default() -> Self {
        Self {
            autoplay: true,
            time_ordered_messages: BTreeMap::new(),
            queue_messages: Vec::new(),
            current_skip_votes: HashSet::new(),
        }
    }
}

#[derive(Default, Debug)]
pub struct GuildCacheMap;

impl TypeMapKey for GuildCacheMap {
    type Value = HashMap<GuildId, GuildCache>;
}

#[cfg(test)]
mod test {

    use super::*;

    #[tokio::test]
    async fn test_guild_cache() {
        let guild_cache = GuildCache::default();
        assert_eq!(guild_cache.autoplay, true);
        assert_eq!(guild_cache.time_ordered_messages.len(), 0);
        assert_eq!(guild_cache.queue_messages.len(), 0);
        assert_eq!(guild_cache.current_skip_votes.len(), 0);
    }

    // Test inserting queue messages and getting them out
    #[tokio::test]
    async fn test_queue_messages() {
        let guild_cache = GuildCache::default();
        let message = Message::default();
        let queue_message = (message, Arc::new(RwLock::new(0)));
        let mut guild_cache = guild_cache.clone();
        guild_cache.queue_messages.push(queue_message.clone());
        assert_eq!(guild_cache.queue_messages.len(), 1);
        //assert_eq!(guild_cache.queue_messages[0], queue_message);
    }
}
