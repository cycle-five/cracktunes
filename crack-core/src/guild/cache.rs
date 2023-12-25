use self::serenity::model::{
    channel::Message,
    id::{GuildId, UserId},
};
use chrono::{DateTime, Utc};
use poise::serenity_prelude as serenity;
use std::{collections::BTreeMap, sync::RwLock};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use typemap_rev::TypeMapKey;

type QueueMessage = (Message, Arc<RwLock<usize>>);
//type TimeOrderedMessage = (DateTime<Utc>, QueueMessage);

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
