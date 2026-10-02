//! Leaving voice discards the queue, and it does so without the queue lease:
//! `/leave` must work even during a `/gp` game. This is the one way out, so
//! the discard is recorded like any other queue change. See `music::audit`.

use crate::music::audit::{emit, Action, Actor, AuditEvent};
use crate::Data;
use serenity::all::{ChannelId, GuildId};
use songbird::Songbird;

/// Leave, then record what it discarded (only if the leave succeeded). Returns `manager.remove`'s
/// result unchanged, so each caller keeps its own handling.
// (Attributes on expressions are not stable, so the exemption sits on the fn.)
#[expect(
    clippy::disallowed_methods,
    reason = "the one audited caller of `Songbird::get` and `Songbird::remove`; see clippy.toml"
)]
pub async fn disconnect(
    data: &Data,
    manager: &Songbird,
    guild_id: GuildId,
    actor: Actor,
) -> Result<(), songbird::error::JoinError> {
    // Raw `get` on purpose: what is registered is what `remove` will discard,
    // connected or not. Read it first; `remove` drops the Call.
    let seen = match manager.get(guild_id) {
        Some(call) => {
            let h = call.lock().await;
            Some((h.queue().len(), h.current_channel()))
        },
        None => None,
    };
    let result = manager.remove(guild_id).await;
    // Only a leave that happened is recorded: a failed one (gateway down) leaves
    // the Call and its queue in place.
    if result.is_ok() {
        if let Some((discarded, voice)) = seen {
            if discarded > 0 {
                emit(
                    data.audit_tx.as_ref(),
                    AuditEvent {
                        at: chrono::Utc::now(),
                        guild_id,
                        voice_channel: voice.map(|c| ChannelId::new(c.get())),
                        actor,
                        action: Action::Leave { discarded },
                    },
                );
            }
        }
    }
    result
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::audit::{Action, Actor, BotReason};
    use crate::{Data, DataInner};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(5);

    fn data_with_audit() -> (Data, tokio::sync::mpsc::Receiver<AuditEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        (
            Data(Arc::new(DataInner {
                audit_tx: Some(tx),
                ..Default::default()
            })),
            rx,
        )
    }

    fn manager() -> Arc<songbird::Songbird> {
        let m = songbird::Songbird::serenity();
        m.initialise_client_data(1, serenity::all::UserId::new(2));
        m
    }

    /// Register a `Call` on `manager` without connecting, with `n` offline tracks.
    async fn register(data: &Data, manager: &songbird::Songbird, n: usize) {
        let call = manager.get_or_insert(G);
        let guard = data
            .lock_queue(
                G,
                crate::music::PlaybackOwner::Free,
                Actor::bot(BotReason::Autopause),
            )
            .await
            .unwrap();
        for i in 0..n {
            crate::music::queue::enqueue_input_back(
                &guard,
                &call,
                songbird::input::File::new(format!("/nonexistent/{i}.opus")).into(),
                None,
                None,
            )
            .await;
        }
    }

    #[tokio::test]
    async fn disconnect_records_how_many_tracks_it_discarded() {
        let (data, mut rx) = data_with_audit();
        let manager = manager();
        register(&data, &manager, 2).await;
        while rx.try_recv().is_ok() {}
        disconnect(&data, &manager, G, Actor::bot(BotReason::IdleTimeout))
            .await
            .expect("an unconnected Call leaves Ok");
        let e = rx.try_recv().expect("recorded");
        assert_eq!(e.action, Action::Leave { discarded: 2 });
        assert_eq!(e.actor, Actor::bot(BotReason::IdleTimeout));
    }

    #[tokio::test]
    async fn disconnecting_with_nothing_queued_records_nothing() {
        let (data, mut rx) = data_with_audit();
        let manager = manager();
        // A registered but empty Call, so the `discarded > 0` check is what is tested.
        let _ = manager.get_or_insert(G);
        let _ = disconnect(&data, &manager, G, Actor::bot(BotReason::Kicked)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn disconnect_does_not_wait_for_the_lease() {
        let (data, _rx) = data_with_audit();
        let manager = manager();
        let _held = data
            .lock_queue(
                G,
                crate::music::PlaybackOwner::Free,
                Actor::bot(BotReason::Game),
            )
            .await
            .unwrap();
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            disconnect(&data, &manager, G, Actor::bot(BotReason::Kicked)),
        )
        .await
        .expect("disconnect must not take the queue lease");
    }
}
