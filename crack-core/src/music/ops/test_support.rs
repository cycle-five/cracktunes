//! Offline queues for op tests: tracks queue on a `Call::standalone`, nothing plays.
use super::OpCx;
use crate::music::{
    PlaybackOwner,
    audit::{Action, Actor, AuditEvent, BotReason},
    queue::enqueue_input_back,
};
use crate::{Data, DataInner};
use serenity::all::{Cache, GuildId, Http, UserId};
use songbird::{Call, input::AuxMetadata};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub const GUILD: GuildId = GuildId::new(1);

pub fn offline_call() -> Arc<Mutex<Call>> {
    Arc::new(Mutex::new(Call::standalone(GUILD, UserId::new(2))))
}

pub fn titled(title: &str) -> AuxMetadata {
    AuxMetadata {
        title: Some(title.to_owned()),
        ..Default::default()
    }
}

/// `n` tracks titled t0..t(n-1) on an offline call, with the audit channel
/// drained of the adds.
pub async fn queue_of(
    n: usize,
) -> (
    Data,
    Arc<Mutex<Call>>,
    Vec<uuid::Uuid>,
    mpsc::Receiver<AuditEvent>,
) {
    let (tx, mut rx) = mpsc::channel(256);
    let data = Data(Arc::new(DataInner {
        audit_tx: Some(tx),
        ..Default::default()
    }));
    let call = offline_call();
    let mut ids = Vec::new();
    {
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, Actor::bot(BotReason::Autopause))
            .await
            .unwrap();
        for i in 0..n {
            let h = enqueue_input_back(
                &guard,
                &call,
                songbird::input::File::new(format!("/nonexistent/{i}.opus")).into(),
                Some(titled(&format!("t{i}"))),
                None,
            )
            .await;
            ids.push(h.uuid());
        }
    }
    while rx.try_recv().is_ok() {}
    (data, call, ids, rx)
}

/// A guard for `data`, acting as a member through the dashboard.
pub async fn guard(data: &Data) -> crate::music::QueueGuard {
    data.lock_queue(
        GUILD,
        PlaybackOwner::Free,
        Actor::web(UserId::new(9), "test"),
    )
    .await
    .unwrap()
}

/// Every action recorded since the last call.
pub fn recorded(rx: &mut mpsc::Receiver<AuditEvent>) -> Vec<Action> {
    let mut out = Vec::new();
    while let Ok(e) = rx.try_recv() {
        out.push(e.action);
    }
    out
}

/// The queued ids, in order.
pub async fn ids_of(call: &Arc<Mutex<Call>>) -> Vec<uuid::Uuid> {
    call.lock()
        .await
        .queue()
        .current_queue()
        .iter()
        .map(|h| h.uuid())
        .collect()
}

/// An `OpCx` over a default `Data` with no call registered.
pub fn cx_without_call() -> OpCx {
    OpCx {
        data: Arc::new(Data(Arc::new(DataInner::default()))),
        http: Arc::new(Http::new(crack_types::get_valid_token())),
        cache: Arc::new(Cache::default()),
        guild_id: GUILD,
        actor: Actor::web(UserId::new(9), "test"),
    }
}
