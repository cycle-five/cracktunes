//! Offline queues for op tests: tracks queue on a `Call::standalone`, nothing plays.
use super::OpCx;
use crate::music::{
    audit::{Action, Actor, AuditEvent, BotReason},
    queue::enqueue_input_back,
    PlaybackOwner,
};
use crate::{Data, DataInner};
use serenity::all::{Cache, GuildId, Http, UserId};
use songbird::{input::AuxMetadata, Call};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub const GUILD: GuildId = GuildId::new(1);

pub fn offline_call() -> Arc<Mutex<Call>> {
    Arc::new(Mutex::new(standalone_call(GUILD, UserId::new(2))))
}

/// A `Call::standalone` that cannot be orphaned by another test.
///
/// 🪤 songbird keeps one process-wide mixer scheduler, and the task behind it
/// is `tokio::spawn`ed onto whichever runtime first builds a call. Every
/// `#[tokio::test]` has its own runtime, so when the test that happened to
/// create the scheduler finished, the scheduler died with it, and the next
/// test to build a call panicked in `Scheduler::new_mixer` with `SendError`.
/// Which test that was depended on ordering: it failed CI once on ct#586 and
/// every time under `--test-threads=1`. The scheduler is now created first,
/// on a runtime that lives as long as the test process.
#[expect(
    clippy::disallowed_methods,
    reason = "the one sanctioned test constructor; see above"
)]
pub fn standalone_call(guild: GuildId, user: UserId) -> Call {
    static LASTING: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let runtime = LASTING.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("songbird-test-scheduler")
            .enable_all()
            .build()
            .expect("a runtime for songbird's test scheduler")
    });
    {
        let _inside = runtime.enter();
        songbird::driver::get_default_scheduler();
    }
    Call::standalone(guild, user)
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
