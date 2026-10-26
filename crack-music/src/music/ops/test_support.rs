//! Offline queues for op tests: tracks queue on a `Call::standalone`, nothing plays.
use super::{OpCx, Settle};
use crate::music::{
    audit::{Action, Actor, AuditEvent, BotReason},
    queue::enqueue_input_back,
    PlaybackOwner,
};
use crate::music::{surface::Surface, MusicData};
use crack_common::{message::CrackedMessage, render::RenderCx, transport::Transport};
use serenity::all::{Cache, GenericChannelId, GuildId, Http, MessageId, UserId};
use songbird::{input::AuxMetadata, Call, Songbird};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub const GUILD: GuildId = GuildId::new(1);

pub fn offline_call() -> Arc<Mutex<Call>> {
    Arc::new(Mutex::new(standalone_call(GUILD, UserId::new(2))))
}

/// Create songbird's process-wide mixer scheduler on a runtime that outlives
/// every test. Call it before building any `Call`.
///
/// 🪤 songbird keeps one process-wide mixer scheduler, and the task behind it
/// is `tokio::spawn`ed onto whichever runtime first builds a call. Every
/// `#[tokio::test]` has its own runtime, so when the test that happened to
/// create the scheduler finished, the scheduler died with it, and the next
/// test to build a call panicked in `Scheduler::new_mixer` with `SendError`.
/// Which test that was depended on ordering: it failed CI once on ct#586 and
/// every time under `--test-threads=1`. The scheduler is now created first,
/// on a runtime that lives as long as the test process.
pub fn lasting_scheduler() {
    static LASTING: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let runtime = LASTING.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("songbird-test-scheduler")
            .enable_all()
            .build()
            .expect("a runtime for songbird's test scheduler")
    });
    let _inside = runtime.enter();
    songbird::driver::get_default_scheduler();
}

/// A `Call::standalone` that cannot be orphaned by another test: it creates
/// the scheduler first, see [`lasting_scheduler`] for the trap.
#[expect(
    clippy::disallowed_methods,
    reason = "the one sanctioned test constructor; see lasting_scheduler"
)]
pub fn standalone_call(guild: GuildId, user: UserId) -> Call {
    lasting_scheduler();
    Call::standalone(guild, user)
}

pub fn titled(title: &str) -> AuxMetadata {
    AuxMetadata {
        title: Some(title.to_owned()),
        ..Default::default()
    }
}

/// A `MusicData` built without `Data`, each field the way `DataInner::default()`
/// builds it, over a [`RecordingSurface`]. Adjust it with struct-update syntax.
pub fn test_music_data() -> MusicData {
    MusicData {
        database_pool: None,
        songbird: Songbird::serenity(),
        ct_client: crack_testing::CrackTrackClient::default(),
        http_client: crate::http_utils::get_client().clone(),
        audit_tx: None,
        db_channel: None,
        autoplay_buffer: Arc::default(),
        guild_cache_map: Arc::new(Mutex::new(std::collections::HashMap::new())),
        playback_owners: Arc::default(),
        queue_locks: Arc::default(),
        guild: crack_guild::operations::GuildData::default(),
        surface: Arc::new(RecordingSurface::default()),
    }
}

/// `n` tracks titled t0..t(n-1) queued on `data`'s guild 1 and a new offline
/// call, with their ids.
pub async fn queue_on(data: &MusicData, n: usize) -> (Arc<Mutex<Call>>, Vec<uuid::Uuid>) {
    let call = offline_call();
    let mut ids = Vec::new();
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
    drop(guard);
    (call, ids)
}

/// `n` tracks titled t0..t(n-1) on an offline call, with the audit channel
/// drained of the adds.
pub async fn queue_of(
    n: usize,
) -> (
    MusicData,
    Arc<Mutex<Call>>,
    Vec<uuid::Uuid>,
    mpsc::Receiver<AuditEvent>,
) {
    let (tx, mut rx) = mpsc::channel(256);
    let data = MusicData {
        audit_tx: Some(tx),
        ..test_music_data()
    };
    let (call, ids) = queue_on(&data, n).await;
    while rx.try_recv().is_ok() {}
    (data, call, ids, rx)
}

/// A guard for `data`, acting as a member through the dashboard.
pub async fn guard(data: &MusicData) -> crate::music::QueueGuard {
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

/// An `OpCx` over a default `MusicData` with no call registered.
pub fn cx_without_call() -> OpCx {
    OpCx {
        data: test_music_data(),
        http: Arc::new(Http::new(crack_types::get_valid_token())),
        cache: Arc::new(Cache::default()),
        guild_id: GUILD,
        actor: Actor::web(UserId::new(9), "test"),
    }
}

/// One settle the recording surface was asked for.
pub type RecordedSettle = (Settle, GuildId, Option<(GenericChannelId, MessageId)>);

/// A [`Surface`] that records what it is asked and renders and sends nothing.
/// `echo` returns `None`.
#[derive(Default)]
pub struct RecordingSurface {
    pub settles: std::sync::Mutex<Vec<RecordedSettle>>,
}

#[serenity::async_trait]
impl Surface for RecordingSurface {
    async fn settle(
        &self,
        settle: Settle,
        cx: &OpCx,
        _call: Option<&Arc<Mutex<Call>>>,
        anchor: Option<(GenericChannelId, MessageId)>,
    ) {
        self.settles
            .lock()
            .unwrap()
            .push((settle, cx.guild_id, anchor));
    }

    async fn echo(
        &self,
        _guild: GuildId,
        _transport: &dyn Transport,
        _msg: &CrackedMessage,
        _cx: &RenderCx,
    ) -> Option<(GenericChannelId, MessageId)> {
        None
    }
}

/// A default `MusicData` whose surface is `surface`.
pub fn music_data_recording(surface: Arc<RecordingSurface>) -> MusicData {
    MusicData {
        surface,
        ..test_music_data()
    }
}
