//! The floating status message: one per guild, edited in place while it is
//! still the newest message in its channel and moved to the bottom otherwise.
//! Spec: docs/superpowers/specs/2026-09-15-floating-status-message-design.md

use crate::http_utils::is_unknown_message;
use serenity::all::{CreateEmbed, GenericChannelId, GuildId, MessageId};
use serenity::async_trait;
use std::sync::Arc;

/// Whether the status says something is playing or that playback finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Finished,
}

/// The status message on screen for a guild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMessage {
    pub channel: GenericChannelId,
    pub id: MessageId,
    pub phase: Phase,
}

/// Everything the status message needs to remember per guild.
#[derive(Debug, Default)]
pub struct StatusSlot {
    /// What is on screen now, if anything.
    pub message: Option<StatusMessage>,
    /// Where the guild's most recent music command was run.
    pub last_command_channel: Option<GenericChannelId>,
}

/// How to bring the status up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Still the newest message in its channel: change it in place.
    Edit,
    /// Something was posted since, or it is in another channel: delete it and
    /// send a new one.
    Replace,
    /// Nothing is tracked: send one.
    Send,
}

/// 🔑 Discord ids grow with time, so the status is still at the bottom exactly
/// when the channel's last message is not newer than it. A cached id *older*
/// than ours is our own send not yet echoed back through the gateway, which is
/// still "nothing posted since". An unknown last message (channel not cached)
/// moves rather than guesses.
pub fn placement(
    current: Option<&StatusMessage>,
    target: GenericChannelId,
    channel_last: Option<MessageId>,
) -> Placement {
    let Some(current) = current else {
        return Placement::Send;
    };
    if current.channel != target {
        return Placement::Replace;
    }
    match channel_last {
        Some(last) if last <= current.id => Placement::Edit,
        _ => Placement::Replace,
    }
}

/// The guild's music channel, else the channel of its last music command,
/// else wherever the status already is. None means post nothing.
pub fn target_channel(
    music: Option<GenericChannelId>,
    last_command: Option<GenericChannelId>,
    tracked: Option<GenericChannelId>,
) -> Option<GenericChannelId> {
    music.or(last_command).or(tracked)
}

/// Why Discord refused a status request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The message is gone -- deleted by hand or by `/clean`.
    UnknownMessage,
    /// Anything else, as text for the log.
    Other(String),
}

impl From<serenity::Error> for TransportError {
    fn from(err: serenity::Error) -> Self {
        if is_unknown_message(&err) {
            Self::UnknownMessage
        } else {
            Self::Other(err.to_string())
        }
    }
}

/// The Discord calls the status makes, behind a seam so every branch of
/// [`apply`] is testable without Discord.
#[async_trait]
pub trait StatusTransport: Send + Sync {
    async fn send(
        &self,
        channel: GenericChannelId,
        embed: CreateEmbed<'static>,
    ) -> Result<MessageId, TransportError>;
    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        embed: CreateEmbed<'static>,
    ) -> Result<(), TransportError>;
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>;
    /// The newest message id the gateway has reported for `channel`, if the
    /// channel is cached.
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId>;
}

/// Bring the status in `slot` up to date in `target`, and return what is on
/// screen afterwards.
pub async fn apply(
    transport: &dyn StatusTransport,
    slot: &mut StatusSlot,
    guild: GuildId,
    target: GenericChannelId,
    embed: CreateEmbed<'static>,
    phase: Phase,
) -> Option<StatusMessage> {
    let current = slot.message;
    let last = transport.last_message_id(guild, target);
    match (placement(current.as_ref(), target, last), current) {
        (Placement::Edit, Some(current)) => {
            match transport
                .edit(current.channel, current.id, embed.clone())
                .await
            {
                Ok(()) => {
                    let shown = StatusMessage { phase, ..current };
                    slot.message = Some(shown);
                    return Some(shown);
                },
                // Deleted by hand or by `/clean`: a fresh one is sent below.
                Err(TransportError::UnknownMessage) => {},
                Err(TransportError::Other(err)) => {
                    tracing::warn!(
                        "status: could not edit {} in {}: {err}",
                        current.id,
                        current.channel
                    );
                    slot.message = None;
                    return None;
                },
            }
        },
        (Placement::Replace, Some(current)) => {
            match transport.delete(current.channel, current.id).await {
                Ok(()) | Err(TransportError::UnknownMessage) => {},
                Err(TransportError::Other(err)) => tracing::warn!(
                    "status: could not delete {} in {}: {err}",
                    current.id,
                    current.channel
                ),
            }
        },
        _ => {},
    }
    match transport.send(target, embed).await {
        Ok(id) => {
            let shown = StatusMessage {
                channel: target,
                id,
                phase,
            };
            slot.message = Some(shown);
            Some(shown)
        },
        Err(err) => {
            tracing::warn!("status: could not send to {target}: {err:?}");
            slot.message = None;
            None
        },
    }
}

impl crate::Data {
    /// The guild's status slot, created on first use.
    ///
    /// 🪤 The `.clone()` matters: a dashmap reference held across the caller's
    /// `.lock().await` deadlocks the shard (see `lease.rs::lock_queue`).
    pub fn status_slot(&self, guild: GuildId) -> Arc<tokio::sync::Mutex<StatusSlot>> {
        self.status_slots
            .entry(guild)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(StatusSlot::default())))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const GUILD: GuildId = GuildId::new(1);

    fn ch(id: u64) -> GenericChannelId {
        GenericChannelId::new(id)
    }

    fn tracked(channel: u64, id: u64, phase: Phase) -> StatusMessage {
        StatusMessage {
            channel: ch(channel),
            id: MessageId::new(id),
            phase,
        }
    }

    fn embed() -> CreateEmbed<'static> {
        CreateEmbed::new().title("status")
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Send(u64),
        Edit(u64, u64),
        Delete(u64, u64),
    }

    /// A stand-in Discord: records every call, answers from what the test set.
    #[derive(Default)]
    struct Fake {
        last: std::sync::Mutex<Option<MessageId>>,
        edit_error: std::sync::Mutex<Option<TransportError>>,
        delete_error: std::sync::Mutex<Option<TransportError>>,
        send_error: std::sync::Mutex<Option<TransportError>>,
        ops: std::sync::Mutex<Vec<Op>>,
        sent: AtomicU64,
    }

    impl Fake {
        fn with_last(self, last: u64) -> Self {
            *self.last.lock().unwrap() = Some(MessageId::new(last));
            self
        }
        fn ops(&self) -> Vec<Op> {
            self.ops.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl StatusTransport for Fake {
        async fn send(
            &self,
            channel: GenericChannelId,
            _embed: CreateEmbed<'static>,
        ) -> Result<MessageId, TransportError> {
            self.ops.lock().unwrap().push(Op::Send(channel.get()));
            if let Some(err) = self.send_error.lock().unwrap().clone() {
                return Err(err);
            }
            Ok(MessageId::new(
                1000 + self.sent.fetch_add(1, Ordering::SeqCst),
            ))
        }

        async fn edit(
            &self,
            channel: GenericChannelId,
            id: MessageId,
            _embed: CreateEmbed<'static>,
        ) -> Result<(), TransportError> {
            self.ops
                .lock()
                .unwrap()
                .push(Op::Edit(channel.get(), id.get()));
            match self.edit_error.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }

        async fn delete(
            &self,
            channel: GenericChannelId,
            id: MessageId,
        ) -> Result<(), TransportError> {
            self.ops
                .lock()
                .unwrap()
                .push(Op::Delete(channel.get(), id.get()));
            match self.delete_error.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }

        fn last_message_id(
            &self,
            _guild: GuildId,
            _channel: GenericChannelId,
        ) -> Option<MessageId> {
            *self.last.lock().unwrap()
        }
    }

    // ---- placement ----

    #[test]
    fn nothing_tracked_is_sent() {
        assert_eq!(
            placement(None, ch(5), Some(MessageId::new(9))),
            Placement::Send
        );
    }

    #[test]
    fn a_status_in_another_channel_is_moved() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(
            placement(Some(&current), ch(6), Some(MessageId::new(100))),
            Placement::Replace
        );
    }

    #[test]
    fn a_message_posted_since_moves_the_status() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(
            placement(Some(&current), ch(5), Some(MessageId::new(101))),
            Placement::Replace
        );
    }

    #[test]
    fn a_quiet_channel_edits_in_place() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(
            placement(Some(&current), ch(5), Some(MessageId::new(100))),
            Placement::Edit
        );
    }

    /// Our own send has not echoed back through the gateway yet, so the cache
    /// still holds an older id. That is not "someone posted".
    #[test]
    fn our_own_send_not_yet_echoed_still_edits() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(
            placement(Some(&current), ch(5), Some(MessageId::new(99))),
            Placement::Edit
        );
    }

    #[test]
    fn an_uncached_channel_moves_rather_than_guesses() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(5), None), Placement::Replace);
    }

    // ---- target_channel ----

    #[test]
    fn the_music_channel_wins() {
        assert_eq!(
            target_channel(Some(ch(7)), Some(ch(5)), Some(ch(6))),
            Some(ch(7))
        );
    }

    #[test]
    fn then_the_last_command_channel() {
        assert_eq!(target_channel(None, Some(ch(5)), Some(ch(6))), Some(ch(5)));
    }

    #[test]
    fn then_wherever_the_status_already_is() {
        assert_eq!(target_channel(None, None, Some(ch(6))), Some(ch(6)));
    }

    #[test]
    fn with_nowhere_known_there_is_no_channel() {
        assert_eq!(target_channel(None, None, None), None);
    }

    // ---- apply ----

    #[tokio::test]
    async fn the_first_update_sends_and_tracks_the_message() {
        let fake = Fake::default();
        let mut slot = StatusSlot::default();

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
        assert_eq!(slot.message, shown);
    }

    #[tokio::test]
    async fn a_quiet_channel_is_edited_in_place() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100)]);
        assert_eq!(shown, Some(tracked(5, 100, Phase::Playing)));
    }

    #[tokio::test]
    async fn chat_since_the_status_moves_it_to_the_bottom() {
        let fake = Fake::default().with_last(101);
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_status_deleted_by_hand_is_sent_again() {
        let fake = Fake::default().with_last(100);
        *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_failed_delete_still_sends_the_new_status() {
        let fake = Fake::default().with_last(101);
        *fake.delete_error.lock().unwrap() =
            Some(TransportError::Other("Missing Permissions".into()));
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_failed_send_forgets_the_message() {
        let fake = Fake::default().with_last(101);
        *fake.send_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(5)]);
        assert_eq!(shown, None);
        assert_eq!(slot.message, None);
    }

    #[tokio::test]
    async fn a_failed_edit_forgets_the_message() {
        let fake = Fake::default().with_last(100);
        *fake.edit_error.lock().unwrap() =
            Some(TransportError::Other("Missing Permissions".into()));
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100)]);
        assert_eq!(shown, None);
        assert_eq!(slot.message, None);
    }

    #[tokio::test]
    async fn finished_stays_tracked_and_playing_continues_it() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let finished = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Finished).await;
        let playing = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(finished, Some(tracked(5, 100, Phase::Finished)));
        assert_eq!(playing, Some(tracked(5, 100, Phase::Playing)));
        assert_eq!(fake.ops(), vec![Op::Edit(5, 100), Op::Edit(5, 100)]);
    }

    #[tokio::test]
    async fn moving_to_another_channel_deletes_the_old_status() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot {
            message: Some(tracked(5, 100, Phase::Playing)),
            ..Default::default()
        };

        let shown = apply(&fake, &mut slot, GUILD, ch(6), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(6)]);
        assert_eq!(shown, Some(tracked(6, 1000, Phase::Playing)));
    }

    // ---- per-guild slot ----

    #[tokio::test]
    async fn every_update_for_a_guild_shares_one_slot() {
        let data = crate::Data::default();

        data.status_slot(GUILD).lock().await.last_command_channel = Some(ch(5));

        assert_eq!(
            data.status_slot(GUILD).lock().await.last_command_channel,
            Some(ch(5))
        );
        assert_eq!(
            data.status_slot(GuildId::new(2))
                .lock()
                .await
                .last_command_channel,
            None
        );
    }
}
