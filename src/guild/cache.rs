use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use poise::serenity_prelude as serenity;
use serenity::{
    model::{
        channel::Message,
        id::{GuildId, UserId},
    },
    RwLock, TypeMapKey,
};

type QueueMessage = (Message, Arc<RwLock<usize>>);

#[derive(Default)]
pub struct GuildCache {
    pub queue_messages: Vec<QueueMessage>,
    pub current_skip_votes: HashSet<UserId>,
}

pub struct GuildCacheMap;

impl TypeMapKey for GuildCacheMap {
    type Value = HashMap<GuildId, GuildCache>;
}
