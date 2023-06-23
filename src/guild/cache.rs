use self::serenity::{
    model::{
        channel::Message,
        id::{GuildId, UserId},
    },
    RwLock, TypeMapKey,
};
use poise::serenity_prelude as serenity;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

type QueueMessage = (Message, Arc<RwLock<usize>>);

#[derive(Default, Debug)]
pub struct GuildCache {
    pub queue_messages: Vec<QueueMessage>,
    pub current_skip_votes: HashSet<UserId>,
}

#[derive(Default, Debug)]
pub struct GuildCacheMap;

impl TypeMapKey for GuildCacheMap {
    type Value = HashMap<GuildId, GuildCache>;
}
