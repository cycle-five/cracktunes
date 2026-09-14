use crate::{
    //commands::sub_help as help,
    errors::CrackedError,
    guild::cache::GuildCache,
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context,
    Error,
};
use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use serenity::http::{HttpError, JsonErrorCode};
use serenity::model::channel::Message;
use std::collections::BTreeMap;

/// How new the bot's latest message must be for `/clean` to undo only that one.
const UNDO_WINDOW_SECONDS: i64 = 15;

/// Delete the bot's messages, or only its latest one if sent in the last 15 seconds.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Utility",
    prefix_command,
    slash_command,
    guild_only,
    required_permissions = "MANAGE_MESSAGES",
    required_bot_permissions = "MANAGE_MESSAGES",
    //subcommands("help")
)]
pub async fn clean(ctx: Context<'_>) -> Result<(), Error> {
    clean_internal(ctx).await
}

/// Clean up old messages from the bot, internal fucntion.
pub async fn clean_internal(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let cache_id: u64 = guild_id.into();
    let data = ctx.data();
    let cached = data
        .id_cache_map
        .get(&cache_id)
        .map(|cache| cache.time_ordered_messages.clone())
        .ok_or(CrackedError::Other("No messages in cache"))?;
    let undo_window = TimeDelta::try_seconds(UNDO_WINDOW_SECONDS)
        .ok_or("Chat cleanup seconds not a number??!?")?;
    let to_delete = messages_to_clean(&cached, Utc::now(), undo_window);
    tracing::info!(
        "/clean: {} cached, deleting {}",
        cached.len(),
        to_delete.len()
    );

    let reply_handle = ctx.say("Cleaning up old messages...").await?;
    let mut status_msg = reply_handle.into_message().await?;
    let mut deleted = 0;
    let mut settled = Vec::with_capacity(to_delete.len());
    for (sent_at, msg) in to_delete {
        status_msg
            .edit(
                &ctx.serenity_context(),
                serenity::builder::EditMessage::default().content(format!(
                    "Deleting message {}\nDeleted so far: {}",
                    msg.id, deleted
                )),
            )
            .await?;
        tracing::warn!("Deleting message {}", msg.id);
        match msg.delete(ctx.http(), None).await {
            Ok(()) => {
                deleted += 1;
                settled.push(sent_at);
            },
            Err(e) if is_unknown_message(&e) => settled.push(sent_at),
            Err(e) => tracing::warn!("/clean could not delete message {}: {e:?}", msg.id),
        }
    }
    forget_messages(&data.id_cache_map, cache_id, &settled);

    status_msg.delete(ctx.http(), None).await?;
    send_reply(&ctx, CrackedMessage::Clean(deleted), true).await?;
    Ok(())
}

/// The cached messages `/clean` deletes, newest first, with their cache keys.
///
/// 🔑 The latest one alone when it is at most `undo_window` old -- an undo of
/// what the bot just said -- and every one otherwise. Until v0.12.0 a latest
/// message that new made `/clean` delete nothing at all: it walked newest
/// first and stopped at the first message inside the window.
fn messages_to_clean(
    cached: &BTreeMap<DateTime<Utc>, Message>,
    now: DateTime<Utc>,
    undo_window: TimeDelta,
) -> Vec<(DateTime<Utc>, Message)> {
    let Some((latest_at, latest)) = cached.last_key_value() else {
        return Vec::new();
    };
    if now - *latest_at <= undo_window {
        return vec![(*latest_at, latest.clone())];
    }
    cached
        .iter()
        .rev()
        .map(|(at, msg)| (*at, msg.clone()))
        .collect()
}

/// Drops the messages `/clean` settled from the live guild cache.
///
/// 🪤 By key, never by writing the snapshot back: a message the bot sent while
/// `/clean` was deleting is not in the snapshot, and would be lost with it.
/// Left in the cache, a deleted message made the next `/clean` fail on it.
fn forget_messages(cache: &DashMap<u64, GuildCache>, cache_id: u64, sent_at: &[DateTime<Utc>]) {
    if let Some(mut guild) = cache.get_mut(&cache_id) {
        for at in sent_at {
            guild.time_ordered_messages.remove(at);
        }
    }
}

/// Whether Discord refused a delete because the message is already gone --
/// deleted by hand, say. Its cache entry can go like a deleted one's.
fn is_unknown_message(err: &serenity::Error) -> bool {
    matches!(
        err,
        serenity::Error::Http(HttpError::UnsuccessfulRequest(response))
            if response.error.code == JsonErrorCode::UnknownMessage
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::http::HttpBuilder;
    use serenity::model::id::{GenericChannelId, MessageId};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const GUILD: u64 = 1134971338021408858;

    fn window() -> TimeDelta {
        TimeDelta::try_seconds(UNDO_WINDOW_SECONDS).unwrap()
    }

    fn message(id: u64) -> Message {
        let mut msg = Message::default();
        msg.id = MessageId::new(id);
        msg
    }

    /// A cache holding `(seconds before now, message id)` entries.
    fn cache_of(now: DateTime<Utc>, entries: &[(i64, u64)]) -> BTreeMap<DateTime<Utc>, Message> {
        entries
            .iter()
            .map(|&(ago, id)| (now - TimeDelta::try_seconds(ago).unwrap(), message(id)))
            .collect()
    }

    fn ids(picked: &[(DateTime<Utc>, Message)]) -> Vec<u64> {
        picked.iter().map(|(_, msg)| msg.id.get()).collect()
    }

    #[test]
    fn a_message_under_15_seconds_old_is_undone_on_its_own() {
        // The production run: /queue 47s before /clean, /nowplaying 7s before.
        let now = Utc::now();
        let cached = cache_of(now, &[(120, 1), (47, 2), (7, 3)]);

        assert_eq!(ids(&messages_to_clean(&cached, now, window())), vec![3]);
    }

    #[test]
    fn a_message_exactly_15_seconds_old_is_still_undone_on_its_own() {
        let now = Utc::now();
        let cached = cache_of(now, &[(60, 1), (UNDO_WINDOW_SECONDS, 2)]);

        assert_eq!(ids(&messages_to_clean(&cached, now, window())), vec![2]);
    }

    #[test]
    fn with_nothing_that_new_every_message_is_cleaned_newest_first() {
        let now = Utc::now();
        let cached = cache_of(now, &[(600, 1), (120, 2), (16, 3)]);

        assert_eq!(
            ids(&messages_to_clean(&cached, now, window())),
            vec![3, 2, 1]
        );
    }

    #[test]
    fn an_empty_cache_cleans_nothing() {
        assert!(messages_to_clean(&BTreeMap::new(), Utc::now(), window()).is_empty());
    }

    #[test]
    fn the_cache_key_travels_with_each_message_to_clean() {
        let now = Utc::now();
        let cached = cache_of(now, &[(600, 1), (120, 2)]);

        let keys: Vec<_> = messages_to_clean(&cached, now, window())
            .into_iter()
            .map(|(at, _)| at)
            .collect();
        assert_eq!(keys, cached.keys().rev().copied().collect::<Vec<_>>());
    }

    #[test]
    fn settled_messages_leave_the_cache_but_one_sent_meanwhile_stays() {
        let now = Utc::now();
        let map: DashMap<u64, GuildCache> = DashMap::new();
        let before = cache_of(now, &[(600, 1), (120, 2)]);
        map.entry(GUILD).or_default().time_ordered_messages = before.clone();
        // Sent while /clean was deleting: not in the snapshot it worked from.
        map.get_mut(&GUILD)
            .unwrap()
            .time_ordered_messages
            .insert(now, message(3));

        let settled: Vec<_> = before.keys().copied().collect();
        forget_messages(&map, GUILD, &settled);

        let left = map.get(&GUILD).unwrap().time_ordered_messages.clone();
        assert_eq!(
            left.values().map(|m| m.id.get()).collect::<Vec<_>>(),
            vec![3]
        );
    }

    #[test]
    fn forgetting_leaves_the_rest_of_the_guild_cache_alone() {
        let now = Utc::now();
        let map: DashMap<u64, GuildCache> = DashMap::new();
        {
            let mut guild = map.entry(GUILD).or_default();
            guild.autoplay = true;
            guild.time_ordered_messages = cache_of(now, &[(600, 1)]);
        }

        forget_messages(&map, GUILD, &[now - TimeDelta::try_seconds(600).unwrap()]);

        let guild = map.get(&GUILD).unwrap();
        assert!(guild.autoplay);
        assert!(guild.time_ordered_messages.is_empty());
    }

    #[test]
    fn forgetting_in_a_guild_with_no_cache_creates_none() {
        let map: DashMap<u64, GuildCache> = DashMap::new();

        forget_messages(&map, GUILD, &[Utc::now()]);

        assert!(map.is_empty());
    }

    /// Asks a stand-in Discord to delete a message, answering with `status`
    /// and `body`, and returns serenity's real error for it.
    async fn delete_answered(status: u16, body: &'static str) -> serenity::Error {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf).await;
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
        let http = HttpBuilder::without_token()
            .proxy(format!("http://{addr}"))
            .ratelimiter_disabled(true)
            .build();
        GenericChannelId::new(1)
            .delete_message(&http, MessageId::new(2), None)
            .await
            .expect_err("the stand-in refuses every delete")
    }

    #[tokio::test]
    async fn a_message_discord_no_longer_knows_counts_as_already_gone() {
        let err = delete_answered(404, r#"{"code":10008,"message":"Unknown Message"}"#).await;

        assert!(is_unknown_message(&err), "{err:?}");
    }

    #[tokio::test]
    async fn a_refused_delete_is_not_taken_for_an_already_gone_message() {
        let err = delete_answered(403, r#"{"code":50013,"message":"Missing Permissions"}"#).await;

        assert!(!is_unknown_message(&err), "{err:?}");
    }
}
