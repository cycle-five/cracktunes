use self::serenity::model::{
    channel::Message,
    id::{GuildId, MessageId, UserId},
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use poise::serenity_prelude as serenity;
use std::collections::BTreeMap;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;
use typemap_rev::TypeMapKey;

type QueueMessage = (Message, Arc<RwLock<usize>>);

#[derive(Debug, Clone, Default)]
pub struct GuildCache {
    /// Off by default, and session-only (Ruling 48): this cache is the only
    /// place autoplay lives, so a restart turns it back off.
    pub autoplay: bool,
    /// The bot's messages `/clean` can delete, keyed by when each was last
    /// sent or edited. Written only by [`remember_bot_message`],
    /// [`touch_bot_message`] and [`forget_bot_messages`].
    pub time_ordered_messages: BTreeMap<DateTime<Utc>, Message>,
    pub queue_messages: Vec<QueueMessage>,
    pub current_skip_votes: HashSet<UserId>,
}

#[derive(Default, Debug)]
pub struct GuildCacheMap;

impl TypeMapKey for GuildCacheMap {
    type Value = HashMap<GuildId, GuildCache>;
}

/// Remember a message the bot sent in `guild_id`, for `/clean`.
///
/// 🔑 Called from the gateway's message-create event for every message the bot
/// authors: command replies, the track-end handler's "Now playing", anything.
/// Until v0.12.1 each send path had to remember its own message, and the ones
/// that did not -- every message the bot posted unprompted -- were never
/// cleaned. A message already remembered keeps its place.
pub fn remember_bot_message(
    cache: &DashMap<u64, GuildCache>,
    guild_id: GuildId,
    msg: Message,
    now: DateTime<Utc>,
) {
    let mut guild = cache.entry(guild_id.get()).or_default();
    if guild
        .time_ordered_messages
        .values()
        .any(|remembered| remembered.id == msg.id)
    {
        return;
    }
    guild.time_ordered_messages.insert(now, msg);
}

/// Restart a remembered message's clock, because it was edited.
///
/// 🪤 `/play` posts its reply as the command starts and edits the result in
/// seconds later. `/clean`'s undo window counted from the first post, so on
/// TuneTitan a reply finished 12s before `/clean` measured 17s old, and
/// `/clean` deleted all 15 messages instead of that one.
pub fn touch_bot_message(
    cache: &DashMap<u64, GuildCache>,
    guild_id: GuildId,
    message_id: MessageId,
    now: DateTime<Utc>,
) {
    let Some(mut guild) = cache.get_mut(&guild_id.get()) else {
        return;
    };
    let Some(sent_at) = guild
        .time_ordered_messages
        .iter()
        .find(|(_, msg)| msg.id == message_id)
        .map(|(at, _)| *at)
    else {
        return;
    };
    if let Some(msg) = guild.time_ordered_messages.remove(&sent_at) {
        // Never backwards: gateway events can arrive out of order.
        guild.time_ordered_messages.insert(now.max(sent_at), msg);
    }
}

/// Forget messages deleted from `guild_id`, by hand or by the bot.
///
/// Left behind, a deleted message could be the one `/clean` chose to undo --
/// and it would undo nothing.
pub fn forget_bot_messages(
    cache: &DashMap<u64, GuildCache>,
    guild_id: GuildId,
    deleted: &[MessageId],
) {
    if let Some(mut guild) = cache.get_mut(&guild_id.get()) {
        guild
            .time_ordered_messages
            .retain(|_, msg| !deleted.contains(&msg.id));
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use chrono::TimeDelta;

    const GUILD: GuildId = GuildId::new(7);

    fn message(id: u64) -> Message {
        let mut msg = Message::default();
        msg.id = MessageId::new(id);
        msg
    }

    fn ago(now: DateTime<Utc>, seconds: i64) -> DateTime<Utc> {
        now - TimeDelta::try_seconds(seconds).unwrap()
    }

    /// `(key, message id)` for each remembered message, oldest first.
    fn remembered(cache: &DashMap<u64, GuildCache>) -> Vec<(DateTime<Utc>, u64)> {
        cache
            .get(&GUILD.get())
            .map(|guild| {
                guild
                    .time_ordered_messages
                    .iter()
                    .map(|(at, msg)| (*at, msg.id.get()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn test_guild_cache() {
        let guild_cache = GuildCache::default();
        assert!(!guild_cache.autoplay, "autoplay is off by default");
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

    #[test]
    fn every_message_the_bot_sends_is_remembered_once() {
        let now = Utc::now();
        let cache = DashMap::new();

        remember_bot_message(&cache, GUILD, message(1), ago(now, 30));
        remember_bot_message(&cache, GUILD, message(2), ago(now, 20));
        // The same message again -- a replayed gateway event.
        remember_bot_message(&cache, GUILD, message(1), ago(now, 10));

        assert_eq!(
            remembered(&cache),
            vec![(ago(now, 30), 1), (ago(now, 20), 2)]
        );
    }

    #[test]
    fn an_edit_restarts_a_messages_clock() {
        let now = Utc::now();
        let cache = DashMap::new();
        remember_bot_message(&cache, GUILD, message(1), ago(now, 20));
        remember_bot_message(&cache, GUILD, message(2), ago(now, 10));

        touch_bot_message(&cache, GUILD, MessageId::new(1), ago(now, 5));

        assert_eq!(
            remembered(&cache),
            vec![(ago(now, 10), 2), (ago(now, 5), 1)]
        );
    }

    #[test]
    fn an_edit_to_a_message_never_remembered_changes_nothing() {
        let now = Utc::now();
        let cache = DashMap::new();
        remember_bot_message(&cache, GUILD, message(1), ago(now, 20));

        touch_bot_message(&cache, GUILD, MessageId::new(99), ago(now, 5));
        touch_bot_message(&cache, GuildId::new(8), MessageId::new(1), ago(now, 5));

        assert_eq!(remembered(&cache), vec![(ago(now, 20), 1)]);
        assert_eq!(cache.len(), 1, "an edit in another guild made it a cache");
    }

    #[test]
    fn a_deleted_message_is_forgotten() {
        let now = Utc::now();
        let cache = DashMap::new();
        for id in 1..=3 {
            remember_bot_message(&cache, GUILD, message(id), ago(now, 40 - id as i64));
        }

        forget_bot_messages(&cache, GUILD, &[MessageId::new(1), MessageId::new(3)]);

        assert_eq!(remembered(&cache), vec![(ago(now, 38), 2)]);
    }
}
