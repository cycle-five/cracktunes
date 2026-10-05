//! The one place a user-initiated playback or queue change is orchestrated.
//! Slash commands, the dashboard and (next) the embed buttons are surfaces
//! over it: they build an [`OpCx`], call an op, render its outcome and settle.
//! Spec: docs/superpowers/specs/2026-10-04-ops-layer-and-dashboard-controls-design.md

mod edit;
mod end;
mod playback;
mod skip;
pub use playback::*;
pub use skip::*;
#[cfg(test)]
pub(crate) mod test_support;

use crate::{
    commands::music_utils::connected_call,
    handlers::track_end::update_queue_messages,
    messaging::{
        messages::{
            FAIL_LOOP, FAIL_PAUSE, FAIL_SEEK_OP, OP_TRACK_ABSENT, OP_TRACK_PLAYING, OP_TRACK_STALE,
        },
        status,
    },
    music::{audit::Actor, PlaybackOwner, QueueGuard},
    CrackedError, Data,
};
use serenity::all::{Cache, GenericChannelId, GuildId, Http, MessageId};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Who is acting, and where.
#[derive(Clone)]
pub struct OpCx {
    pub data: Arc<Data>,
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub guild_id: GuildId,
    pub actor: Actor,
}

impl OpCx {
    /// The member running this command.
    pub fn from_ctx(ctx: &crate::Context<'_>) -> Result<OpCx, CrackedError> {
        let sc = ctx.serenity_context();
        Ok(OpCx {
            data: ctx.data(),
            http: sc.http.clone(),
            cache: sc.cache.clone(),
            guild_id: ctx.guild_id().ok_or(CrackedError::NoGuildId)?,
            actor: Actor::from_ctx(ctx),
        })
    }
}

/// What songbird refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Pause,
    Resume,
    Loop,
}

/// Why an op did nothing.
#[derive(Debug)]
pub enum OpRefused {
    NotConnected,
    NothingPlaying,
    QueueEmpty,
    GameInProgress,
    /// No track with that id is queued (it finished, or was removed).
    Absent,
    /// That id is the playing track; only upcoming tracks move or are removed.
    NowPlaying,
    /// Skip's `expect` is no longer the current track.
    Stale,
    OutOfRange {
        what: &'static str,
        got: usize,
        min: usize,
        max: usize,
    },
    /// A bad argument, with the words the command always used.
    Invalid(&'static str),
    Failed(Failure),
    SeekFailed(songbird::tracks::ControlError),
}

impl From<OpRefused> for CrackedError {
    fn from(r: OpRefused) -> Self {
        match r {
            OpRefused::NotConnected => CrackedError::NotConnected,
            OpRefused::NothingPlaying => CrackedError::NothingPlaying,
            OpRefused::QueueEmpty => CrackedError::QueueEmpty,
            OpRefused::GameInProgress => CrackedError::GameInProgress,
            OpRefused::Absent => CrackedError::Other(OP_TRACK_ABSENT),
            OpRefused::NowPlaying => CrackedError::Other(OP_TRACK_PLAYING),
            OpRefused::Stale => CrackedError::Other(OP_TRACK_STALE),
            OpRefused::OutOfRange {
                what,
                got,
                min,
                max,
            } => CrackedError::NotInRange(what, got as isize, min as isize, max as isize),
            OpRefused::Invalid(s) => CrackedError::Other(s),
            OpRefused::Failed(Failure::Pause) => CrackedError::Other(FAIL_PAUSE),
            OpRefused::Failed(Failure::Resume) => CrackedError::FailedResume,
            OpRefused::Failed(Failure::Loop) => CrackedError::Other(FAIL_LOOP),
            OpRefused::SeekFailed(_) => CrackedError::Other(FAIL_SEEK_OP),
        }
    }
}

/// Which tracks an op means. Not every op takes every form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Index(usize),
    Range(usize, usize),
    Id(uuid::Uuid),
}

/// What Discord needs refreshed after an op. Decided by the op's effect,
/// never by the surface (spec §1 "Settling").
#[must_use = "settle it: the surface owes Discord a refresh"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    QueueMessages,
    NowPlaying,
    Finished,
    Nothing,
}

impl Settle {
    /// 🔑 Hold no Call lock: this locks it.
    pub async fn after(
        self,
        cx: &OpCx,
        call: Option<&Arc<Mutex<Call>>>,
        anchor: Option<(GenericChannelId, MessageId)>,
    ) {
        match (self, call) {
            (Settle::QueueMessages, Some(call)) => {
                let queue = call.lock().await.queue().current_queue();
                update_queue_messages(&cx.http, cx.data.clone(), &queue, cx.guild_id).await;
            },
            (Settle::NowPlaying, Some(call)) => {
                let playing = call.lock().await.queue().current().is_some();
                if playing {
                    status::show_now_playing_after(
                        &cx.data,
                        cx.http.clone(),
                        cx.cache.clone(),
                        cx.guild_id,
                        call,
                        anchor,
                    )
                    .await;
                }
            },
            (Settle::Finished, _) => {
                status::show_finished(&cx.data, cx.http.clone(), cx.cache.clone(), cx.guild_id)
                    .await;
            },
            _ => {},
        }
    }

    pub async fn now(self, cx: &OpCx, call: Option<&Arc<Mutex<Call>>>) {
        self.after(cx, call, None).await;
    }
}

/// An op's outcome, how to settle it, and the call it ran on.
#[must_use = "settle it: call settle_now or settle_after"]
pub struct Done<T> {
    pub outcome: T,
    pub settle: Settle,
    pub call: Option<Arc<Mutex<Call>>>,
}

impl<T> Done<T> {
    pub async fn settle_now(self, cx: &OpCx) -> T {
        self.settle.now(cx, self.call.as_ref()).await;
        self.outcome
    }
    pub async fn settle_after(self, cx: &OpCx, anchor: Option<(GenericChannelId, MessageId)>) -> T {
        self.settle.after(cx, self.call.as_ref(), anchor).await;
        self.outcome
    }
}

/// The lease, then the call. In that order: a game refuses at once, before
/// the call is looked up (as `remote::move_by_id` always did).
pub(crate) async fn begin(cx: &OpCx) -> Result<(QueueGuard, Arc<Mutex<Call>>), OpRefused> {
    let guard = cx
        .data
        .lock_queue(cx.guild_id, PlaybackOwner::Free, cx.actor.clone())
        .await
        .map_err(|e| match e {
            CrackedError::GameInProgress => OpRefused::GameInProgress,
            other => {
                tracing::warn!("lock_queue refused an op: {other}");
                OpRefused::GameInProgress
            },
        })?;
    let call = connected_call(&cx.data.songbird, cx.guild_id, None)
        .await
        .ok_or(OpRefused::NotConnected)?;
    Ok((guard, call))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{music::PlaybackOwner, Data, DataInner};
    use serenity::all::{Cache, GuildId, Http, UserId};

    const G: GuildId = GuildId::new(1);

    fn cx(data: Data) -> OpCx {
        OpCx {
            data: Arc::new(data),
            http: Arc::new(Http::new(crack_types::get_valid_token())),
            cache: Arc::new(Cache::default()),
            guild_id: G,
            actor: Actor::web(UserId::new(9), "test"),
        }
    }

    #[tokio::test]
    async fn a_game_refuses_before_the_call_is_looked_up() {
        let d = Data(Arc::new(DataInner::default()));
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        assert!(matches!(
            begin(&cx(d)).await,
            Err(OpRefused::GameInProgress)
        ));
    }

    #[tokio::test]
    async fn no_call_is_not_connected() {
        let d = Data(Arc::new(DataInner::default()));
        assert!(matches!(begin(&cx(d)).await, Err(OpRefused::NotConnected)));
    }

    #[test]
    fn refusals_map_to_the_errors_the_commands_replied_with() {
        use crate::messaging::messages::FAIL_LOOP;
        let cases: Vec<(OpRefused, String)> = vec![
            (
                OpRefused::NotConnected,
                CrackedError::NotConnected.to_string(),
            ),
            (
                OpRefused::NothingPlaying,
                CrackedError::NothingPlaying.to_string(),
            ),
            (OpRefused::QueueEmpty, CrackedError::QueueEmpty.to_string()),
            (
                OpRefused::GameInProgress,
                CrackedError::GameInProgress.to_string(),
            ),
            (
                OpRefused::Failed(Failure::Resume),
                CrackedError::FailedResume.to_string(),
            ),
            (
                OpRefused::Failed(Failure::Pause),
                CrackedError::Other("Failed to pause").to_string(),
            ),
            (
                OpRefused::Failed(Failure::Loop),
                CrackedError::Other(FAIL_LOOP).to_string(),
            ),
            (
                OpRefused::Invalid("Index for `at` out of bounds"),
                CrackedError::Other("Index for `at` out of bounds").to_string(),
            ),
            (
                OpRefused::OutOfRange {
                    what: "index",
                    got: 7,
                    min: 1,
                    max: 3,
                },
                CrackedError::NotInRange("index", 7, 1, 3).to_string(),
            ),
        ];
        for (refused, want) in cases {
            let label = format!("{refused:?}");
            assert_eq!(CrackedError::from(refused).to_string(), want, "{label}");
        }
    }

    #[test]
    fn web_actors_name_the_op() {
        let a = Actor::web(UserId::new(9), "skip");
        assert_eq!(a.command(), "dashboard skip");
        assert_eq!(a.source(), crate::music::audit::Source::Web);
    }
}
