//! Queue operations for callers with no poise `Context` -- the web dashboard.
//!
//! Everything that touches songbird for the dashboard lives here, inside
//! crack-core, so it stays behind this crate's `clippy.toml` bans (no
//! `Songbird::get`, no `TrackHandle::data`). crack-web only sees the plain
//! types below.

use crate::messaging::messages::{
    ECHO_FROM_DASHBOARD, ECHO_PAUSED, ECHO_REMOVED, ECHO_REPEAT_OFF, ECHO_REPEAT_ON, ECHO_RESUMED,
    ECHO_SHUFFLED, ECHO_SKIPPED,
};
use crate::messaging::status::{self, DiscordTransport};
use crate::music::audit_view::{cap, escape, TITLE_MAX};
use crate::music::{audit::Actor, ops, PlaybackOwner, QueueGuard};
use crate::{
    commands::music_utils::connected_call,
    utils::{get_requesting_user, get_track_handle_metadata},
    Data,
};
use serenity::all::{Cache, ChannelId, CreateEmbed, GuildId, Http, UserId};
use songbird::{
    input::AuxMetadata,
    tracks::{LoopState, PlayMode, TrackHandle, TrackState},
    Call,
};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Who queued a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requester {
    /// Autoplay picked it (stored as user id 1, see `requesting_user_to_string`).
    Auto,
    User(UserId),
}

/// One queued track, as the dashboard shows it.
#[derive(Debug, Clone)]
pub struct TrackSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
    pub requester: Option<Requester>,
}

/// A guild's queue, as the dashboard may show it.
#[derive(Debug, Clone)]
pub enum QueueState {
    /// Not connected, or nothing queued.
    Idle,
    /// A `/gp` game owns playback: the queue would give the answers away.
    Hidden,
    /// `tracks[0]` is playing; `bot_channel` is where.
    Playing {
        bot_channel: ChannelId,
        tracks: Vec<TrackSummary>,
        /// `tracks[0]` is paused.
        paused: bool,
        /// `tracks[0]` repeats forever.
        looping: bool,
    },
}

/// The queue in `guild_id`. A game hides it before the call is even looked up.
pub async fn queue_state(data: &Data, guild_id: GuildId) -> QueueState {
    unless_a_game(data, guild_id, async {
        match connected_call(&data.songbird, guild_id, None).await {
            Some(call) => state_of_call(&call).await,
            None => QueueState::Idle,
        }
    })
    .await
}

/// `read`, unless a game owns playback before it starts or by the time it
/// ends: a game that claims and enqueues mid-read must not have its first
/// title published.
async fn unless_a_game(
    data: &Data,
    guild_id: GuildId,
    read: impl Future<Output = QueueState>,
) -> QueueState {
    if data.playback_owner(guild_id) != PlaybackOwner::Free {
        return QueueState::Hidden;
    }
    let state = read.await;
    if data.playback_owner(guild_id) != PlaybackOwner::Free {
        return QueueState::Hidden;
    }
    state
}

/// The queue on one call. The call lock is held only to clone the handles;
/// metadata is read after it is released.
pub(crate) async fn state_of_call(call: &Arc<Mutex<Call>>) -> QueueState {
    let (channel, handles) = {
        let handler = call.lock().await;
        (handler.current_channel(), handler.queue().current_queue())
    };
    match channel {
        Some(channel) if !handles.is_empty() => {
            let (paused, looping) = read_flags(&handles).await;
            QueueState::Playing {
                bot_channel: ChannelId::new(channel.get()),
                tracks: summarize(&handles).await,
                paused,
                looping,
            }
        },
        _ => QueueState::Idle,
    }
}

/// `(paused, looping)` for a track's state; `None` (unknown) is `(false, false)`.
pub(crate) fn playback_flags(info: Option<&TrackState>) -> (bool, bool) {
    match info {
        Some(i) => (i.playing == PlayMode::Pause, i.loops == LoopState::Infinite),
        None => (false, false),
    }
}

/// The flags of the first handle. A stalled driver never answers `get_info`,
/// so the read is bounded and reads as `(false, false)`.
pub(crate) async fn read_flags(handles: &[TrackHandle]) -> (bool, bool) {
    let info = match handles.first() {
        Some(h) => tokio::time::timeout(ops::TRACK_INFO_TIMEOUT, h.get_info())
            .await
            .ok()
            .and_then(Result::ok),
        None => None,
    };
    playback_flags(info.as_ref())
}

/// Read what the dashboard shows from each handle.
pub(crate) async fn summarize(handles: &[TrackHandle]) -> Vec<TrackSummary> {
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        // No metadata is a blank row, not an error: `/queue` does the same.
        let meta = get_track_handle_metadata(handle).await.unwrap_or_default();
        out.push(summary_of(handle, meta).await);
    }
    out
}

/// One handle's summary from metadata already read. Infallible: a missing
/// requester is `None`, as missing metadata is a blank row.
pub(crate) async fn summary_of(handle: &TrackHandle, meta: AuxMetadata) -> TrackSummary {
    let requester = get_requesting_user(handle).await.ok().map(|u| {
        if u.get() == 1 {
            Requester::Auto
        } else {
            Requester::User(u)
        }
    });
    TrackSummary {
        id: handle.uuid(),
        title: meta.title,
        url: meta.source_url,
        duration: meta.duration,
        requester,
    }
}

/// The voice channel the bot is connected to in `guild_id`, if any.
pub async fn bot_channel(data: &Data, guild_id: GuildId) -> Option<ChannelId> {
    let call = connected_call(&data.songbird, guild_id, None).await?;
    let channel = call.lock().await.current_channel()?;
    Some(ChannelId::new(channel.get()))
}

/// Every guild with a connected call, and the channel it is in.
pub async fn active_guilds(data: &Data) -> Vec<(GuildId, ChannelId)> {
    let calls: Vec<_> = data.songbird.iter().collect();
    let mut out = Vec::new();
    for (guild_id, call) in calls {
        let handler = call.lock().await;
        if handler.current_connection().is_none() {
            continue;
        }
        if let Some(channel) = handler.current_channel() {
            out.push((GuildId::new(guild_id.get()), ChannelId::new(channel.get())));
        }
    }
    out
}

/// Why a move was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefused {
    /// The bot is not connected in this guild.
    NotPlaying,
    /// A `/gp` game owns playback.
    GameInProgress,
    /// No track with that id is queued (it finished, or was removed).
    Absent,
    /// That track is the one playing; only upcoming tracks move.
    NowPlaying,
}

/// Move a track by id. The queue messages in Discord are refreshed in the
/// background: they are edited one by one under Discord's per-channel rate
/// limit, and the move is done whether or not they have caught up. Posts no
/// reply: a drag is silent in the channel (owner's decision).
pub async fn move_by_id(
    data: Arc<Data>,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild_id: GuildId,
    mover: UserId,
    id: Uuid,
    to_upcoming: usize,
) -> Result<usize, MoveRefused> {
    let cx = ops::OpCx {
        data,
        http,
        cache,
        guild_id,
        actor: Actor::web(mover, "move"),
    };
    match ops::move_track(&cx, ops::Target::Id(id), to_upcoming).await {
        Ok(done) => {
            let (moved, settle, call) = done.into_parts();
            // Settled in the background, as before: queue messages are edited
            // one by one under Discord's rate limit; the move is done regardless.
            tokio::spawn(async move {
                settle.now(&cx, call.as_ref()).await;
            });
            Ok(moved.to)
        },
        Err(ops::OpRefused::GameInProgress) => Err(MoveRefused::GameInProgress),
        Err(ops::OpRefused::NotConnected) => Err(MoveRefused::NotPlaying),
        Err(ops::OpRefused::Absent) => Err(MoveRefused::Absent),
        Err(ops::OpRefused::NowPlaying) => Err(MoveRefused::NowPlaying),
        Err(other) => {
            tracing::warn!("dashboard move refused: {other:?}");
            Err(MoveRefused::NotPlaying)
        },
    }
}

/// A control the dashboard may run on the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Only if `expect` is still the playing track.
    Skip {
        expect: Uuid,
    },
    Pause,
    Resume,
    Repeat {
        on: bool,
    },
    Remove {
        id: Uuid,
    },
    Shuffle,
}

/// Why a control was not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRefused {
    NotPlaying,
    GameInProgress,
    /// The queue changed under the member: the track is gone, is playing, or
    /// is no longer the one they saw.
    Conflict,
    Failed,
}

impl From<&ops::OpRefused> for ControlRefused {
    fn from(r: &ops::OpRefused) -> Self {
        use ops::OpRefused as R;
        match r {
            R::GameInProgress => Self::GameInProgress,
            R::NotConnected | R::NothingPlaying | R::QueueEmpty => Self::NotPlaying,
            R::Absent | R::NowPlaying | R::Stale => Self::Conflict,
            other => {
                tracing::warn!("dashboard control refused: {other:?}");
                Self::Failed
            },
        }
    }
}

/// What a control did, for the echo line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Echo {
    Skipped { title: Option<String> },
    Paused,
    Resumed,
    Repeat { on: bool },
    Removed { title: Option<String> },
    Shuffled,
}

impl Echo {
    /// The one line posted in Discord. Titles are third-party text, so they
    /// are cut to `TITLE_MAX` characters, then escaped.
    #[must_use]
    pub fn line(&self, user: UserId) -> String {
        let (what, title) = match self {
            Self::Skipped { title } => (ECHO_SKIPPED, title.as_deref()),
            Self::Paused => (ECHO_PAUSED, None),
            Self::Resumed => (ECHO_RESUMED, None),
            Self::Repeat { on: true } => (ECHO_REPEAT_ON, None),
            Self::Repeat { on: false } => (ECHO_REPEAT_OFF, None),
            Self::Removed { title } => (ECHO_REMOVED, title.as_deref()),
            Self::Shuffled => (ECHO_SHUFFLED, None),
        };
        match title {
            Some(t) => format!(
                "{what} **{}** {ECHO_FROM_DASHBOARD} — <@{user}>",
                escape(&cap(t, TITLE_MAX))
            ),
            None => format!("{what} {ECHO_FROM_DASHBOARD} — <@{user}>"),
        }
    }
}

fn echo_embed(echo: &Echo, user: UserId) -> CreateEmbed<'static> {
    // An embed mention never pings.
    CreateEmbed::new().description(echo.line(user))
}

/// Run a control for `user`. On success the echo is returned at once; posting
/// it and settling happen in the background. The settle is anchored after the
/// echo, so after a skip the new now-playing message lands below it.
pub async fn control(
    data: Arc<Data>,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild_id: GuildId,
    user: UserId,
    c: Control,
) -> Result<Echo, ControlRefused> {
    let op = match c {
        Control::Skip { .. } => "skip",
        Control::Pause => "pause",
        Control::Resume => "resume",
        Control::Repeat { .. } => "repeat",
        Control::Remove { .. } => "remove",
        Control::Shuffle => "shuffle",
    };
    let cx = ops::OpCx {
        data,
        http,
        cache,
        guild_id,
        actor: Actor::web(user, op),
    };
    let (guard, call) = ops::begin(&cx)
        .await
        .map_err(|r| ControlRefused::from(&r))?;
    let (echo, settle) = run_control(&guard, &call, c)
        .await
        .map_err(|r| ControlRefused::from(&r))?;
    // The lease covers the op only; announcing and settling run without it.
    drop(guard);
    let posted = echo.clone();
    tokio::spawn(async move {
        let transport = DiscordTransport {
            http: cx.http.clone(),
            cache: cx.cache.clone(),
        };
        let anchor =
            status::announce(&cx.data, &transport, guild_id, echo_embed(&posted, user)).await;
        settle.after(&cx, Some(&call), anchor).await;
    });
    Ok(echo)
}

/// Run `c` under `guard` on `call`: the op, and what to echo. No Discord.
pub(crate) async fn run_control(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    c: Control,
) -> Result<(Echo, ops::Settle), ops::OpRefused> {
    Ok(match c {
        Control::Skip { expect } => {
            let (s, settle, _) = ops::skip_on(guard, call, 1, Some(expect))
                .await?
                .into_parts();
            (
                Echo::Skipped {
                    title: s.skipped.and_then(|t| t.title),
                },
                settle,
            )
        },
        Control::Pause => {
            let (_, settle, _) = ops::pause_on(guard, call).await?.into_parts();
            (Echo::Paused, settle)
        },
        Control::Resume => {
            let (_, settle, _) = ops::resume_on(guard, call).await?.into_parts();
            (Echo::Resumed, settle)
        },
        Control::Repeat { on } => {
            let (_, settle, _) = ops::repeat_on(guard, call, Some(on)).await?.into_parts();
            (Echo::Repeat { on }, settle)
        },
        Control::Remove { id } => {
            let (r, settle, _) = ops::remove_on(guard, call, ops::Target::Id(id))
                .await?
                .into_parts();
            (
                Echo::Removed {
                    title: r.first.title,
                },
                settle,
            )
        },
        Control::Shuffle => {
            let (_, settle, _) = ops::shuffle_on(guard, call).await?.into_parts();
            (Echo::Shuffled, settle)
        },
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Data, DataInner};
    use serenity::all::{Cache, GuildId, Http};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    fn data() -> Data {
        Data(Arc::new(DataInner::default()))
    }

    #[tokio::test]
    async fn a_game_hides_the_queue_before_anything_else_is_asked() {
        let d = data();
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        assert!(matches!(queue_state(&d, G).await, QueueState::Hidden));
    }

    #[tokio::test]
    async fn a_game_is_checked_before_the_read_starts() {
        let d = data();
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        let got = unless_a_game(&d, G, async { panic!("the call was read") }).await;
        assert!(matches!(got, QueueState::Hidden));
    }

    #[tokio::test]
    async fn a_game_that_starts_during_the_read_hides_what_it_read() {
        let d = data();
        let got = unless_a_game(&d, G, async {
            // A game claims playback and enqueues while the call is summarized.
            d.claim_playback(G, crate::music::PlaybackOwner::Game)
                .unwrap();
            QueueState::Playing {
                bot_channel: ChannelId::new(2),
                tracks: vec![],
                paused: false,
                looping: false,
            }
        })
        .await;
        assert!(matches!(got, QueueState::Hidden));
    }

    #[test]
    fn flags_read_pause_and_infinite_loop_and_default_to_false() {
        use songbird::tracks::{LoopState, PlayMode, TrackState};
        let mut s = TrackState::default();
        assert_eq!(playback_flags(None), (false, false));
        s.playing = PlayMode::Pause;
        s.loops = LoopState::Infinite;
        assert_eq!(playback_flags(Some(&s)), (true, true));
        s.playing = PlayMode::Play;
        s.loops = LoopState::Finite(Default::default());
        assert_eq!(playback_flags(Some(&s)), (false, false));
    }

    #[tokio::test]
    async fn a_stalled_driver_reads_as_neither_paused_nor_looping() {
        // An offline Call::standalone never answers get_info. The outer guard
        // makes a missing timeout fail the test instead of hanging it.
        let (_data, call, _ids, _rx) = crate::music::ops::test_support::queue_of(1).await;
        let handles = call.lock().await.queue().current_queue();
        let got = tokio::time::timeout(Duration::from_secs(5), read_flags(&handles))
            .await
            .expect("read_flags outlived TRACK_INFO_TIMEOUT");
        assert_eq!(got, (false, false));
        assert_eq!(read_flags(&[]).await, (false, false));
    }

    #[tokio::test]
    async fn no_call_is_idle() {
        assert!(matches!(queue_state(&data(), G).await, QueueState::Idle));
        assert_eq!(bot_channel(&data(), G).await, None);
        assert!(active_guilds(&data()).await.is_empty());
    }

    #[tokio::test]
    async fn a_game_refuses_a_move_before_the_call_is_looked_up() {
        let d = Arc::new(data());
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = move_by_id(
            d,
            http,
            Arc::new(Cache::default()),
            G,
            UserId::new(9),
            uuid::Uuid::from_u128(1),
            0,
        )
        .await;
        assert_eq!(got, Err(MoveRefused::GameInProgress));
    }

    #[tokio::test]
    async fn a_move_with_no_call_is_not_playing() {
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = move_by_id(
            Arc::new(data()),
            http,
            Arc::new(Cache::default()),
            G,
            UserId::new(9),
            uuid::Uuid::from_u128(1),
            0,
        )
        .await;
        assert_eq!(got, Err(MoveRefused::NotPlaying));
    }

    #[test]
    fn refusals_map_to_control_answers() {
        use crate::music::ops::OpRefused as R;
        for (r, want) in [
            (R::GameInProgress, ControlRefused::GameInProgress),
            (R::NotConnected, ControlRefused::NotPlaying),
            (R::NothingPlaying, ControlRefused::NotPlaying),
            (R::QueueEmpty, ControlRefused::NotPlaying),
            (R::Absent, ControlRefused::Conflict),
            (R::NowPlaying, ControlRefused::Conflict),
            (R::Stale, ControlRefused::Conflict),
            (
                R::Failed(crate::music::ops::Failure::Pause),
                ControlRefused::Failed,
            ),
        ] {
            assert_eq!(ControlRefused::from(&r), want, "{r:?}");
        }
    }

    #[test]
    fn echo_lines_name_the_track_and_the_member() {
        let u = UserId::new(42);
        assert_eq!(
            Echo::Skipped {
                title: Some("Song".into())
            }
            .line(u),
            "⏭ Skipped **Song** from the dashboard — <@42>"
        );
        assert_eq!(Echo::Paused.line(u), "⏸ Paused from the dashboard — <@42>");
        assert_eq!(
            Echo::Resumed.line(u),
            "▶ Resumed from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Repeat { on: true }.line(u),
            "🔁 Repeat on from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Repeat { on: false }.line(u),
            "🔁 Repeat off from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Removed { title: None }.line(u),
            "🗑 Removed from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Shuffled.line(u),
            "🔀 Shuffled the queue from the dashboard — <@42>"
        );
        assert_eq!(
            Echo::Skipped { title: None }.line(u),
            "⏭ Skipped from the dashboard — <@42>"
        );
    }

    /// 🔑 `escape` backslashes markdown and `<` (so `<@id>` cannot form); a bare
    /// `@everyone` is left alone, and cannot ping from an embed.
    #[test]
    fn a_title_cannot_inject_markdown_or_a_mention() {
        let u = UserId::new(42);
        let l = Echo::Skipped {
            title: Some("**x** <@7> [a](b)\nz".into()),
        }
        .line(u);
        assert_eq!(
            l,
            r"⏭ Skipped **\*\*x\*\* \<\@7\> \[a\](b) z** from the dashboard — <@42>"
        );
    }

    #[tokio::test]
    async fn a_game_refuses_a_control_before_the_call_is_looked_up() {
        let d = Arc::new(data());
        d.claim_playback(G, crate::music::PlaybackOwner::Game)
            .unwrap();
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = control(
            d,
            http,
            Arc::new(Cache::default()),
            G,
            UserId::new(9),
            Control::Pause,
        )
        .await;
        assert_eq!(got, Err(ControlRefused::GameInProgress));
    }

    #[tokio::test]
    async fn a_control_with_no_call_is_not_playing() {
        let http = Arc::new(Http::new(crack_types::get_valid_token()));
        let got = control(
            Arc::new(data()),
            http,
            Arc::new(Cache::default()),
            G,
            UserId::new(9),
            Control::Shuffle,
        )
        .await;
        assert_eq!(got, Err(ControlRefused::NotPlaying));
    }

    /// 🔑 A title of any length still fits a Discord embed description.
    #[test]
    fn a_long_title_is_cut_before_it_is_escaped() {
        let u = UserId::new(42);
        let l = Echo::Removed {
            title: Some("a".repeat(5000)),
        }
        .line(u);
        assert!(l.chars().count() <= 4096, "{} chars", l.chars().count());
        assert!(
            l.contains(&format!("**{}…**", "a".repeat(TITLE_MAX))),
            "{l}"
        );
        assert!(l.ends_with("<@42>"), "{l}");
    }

    mod dispatch {
        use super::super::{run_control, Control, Echo};
        use crate::music::{
            audit::Action,
            ops::{test_support::*, OpRefused, Settle},
        };

        #[tokio::test]
        async fn pause_pauses_and_echoes_paused() {
            let (data, call, _, mut rx) = queue_of(2).await;
            let g = guard(&data).await;
            let (echo, settle) = run_control(&g, &call, Control::Pause).await.unwrap();
            assert_eq!(echo, Echo::Paused);
            assert_eq!(settle, Settle::Nothing);
            assert_eq!(recorded(&mut rx), vec![Action::Pause]);
        }

        #[tokio::test]
        async fn resume_resumes_and_echoes_resumed() {
            let (data, call, _, mut rx) = queue_of(2).await;
            let g = guard(&data).await;
            let (echo, settle) = run_control(&g, &call, Control::Resume).await.unwrap();
            assert_eq!(echo, Echo::Resumed);
            assert_eq!(settle, Settle::Nothing);
            assert_eq!(recorded(&mut rx), vec![Action::Resume]);
        }

        /// Sets, never toggles: a toggle reads `get_info`, which an offline
        /// call never answers, so it would fail (the outer timeout guards a hang).
        #[tokio::test]
        async fn repeat_sets_what_was_asked_and_echoes_it() {
            let (data, call, _, mut rx) = queue_of(1).await;
            let g = guard(&data).await;
            let (echo, settle) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                run_control(&g, &call, Control::Repeat { on: true }),
            )
            .await
            .expect("repeat hung")
            .unwrap();
            assert_eq!(echo, Echo::Repeat { on: true });
            assert_eq!(settle, Settle::Nothing);
            assert_eq!(recorded(&mut rx), vec![Action::Repeat { on: true }]);
            let (echo, _) = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                run_control(&g, &call, Control::Repeat { on: false }),
            )
            .await
            .expect("repeat hung")
            .unwrap();
            assert_eq!(echo, Echo::Repeat { on: false });
            assert_eq!(recorded(&mut rx), vec![Action::Repeat { on: false }]);
        }

        #[tokio::test]
        async fn remove_takes_out_that_track_and_names_it() {
            let (data, call, ids, _) = queue_of(4).await;
            let g = guard(&data).await;
            let (echo, settle) = run_control(&g, &call, Control::Remove { id: ids[2] })
                .await
                .unwrap();
            assert_eq!(
                echo,
                Echo::Removed {
                    title: Some("t2".into())
                }
            );
            assert_eq!(settle, Settle::QueueMessages);
            assert_eq!(ids_of(&call).await, vec![ids[0], ids[1], ids[3]]);
        }

        #[tokio::test]
        async fn skip_advances_and_names_what_it_skipped() {
            let (data, call, ids, _) = queue_of(3).await;
            let g = guard(&data).await;
            let (echo, settle) = run_control(&g, &call, Control::Skip { expect: ids[0] })
                .await
                .unwrap();
            assert_eq!(
                echo,
                Echo::Skipped {
                    title: Some("t0".into())
                }
            );
            assert_eq!(settle, Settle::NowPlaying);
            assert_eq!(ids_of(&call).await, ids[1..].to_vec());
        }

        #[tokio::test]
        async fn a_stale_skip_is_refused_and_changes_nothing() {
            let (data, call, ids, mut rx) = queue_of(3).await;
            let g = guard(&data).await;
            let got = run_control(&g, &call, Control::Skip { expect: ids[1] }).await;
            assert!(matches!(got, Err(OpRefused::Stale)), "{got:?}");
            assert_eq!(ids_of(&call).await, ids);
            assert!(recorded(&mut rx).is_empty());
        }

        #[tokio::test]
        async fn shuffle_keeps_the_playing_track_first() {
            let (data, call, ids, _) = queue_of(6).await;
            let g = guard(&data).await;
            let (echo, settle) = run_control(&g, &call, Control::Shuffle).await.unwrap();
            assert_eq!(echo, Echo::Shuffled);
            assert_eq!(settle, Settle::QueueMessages);
            let after = ids_of(&call).await;
            assert_eq!(after[0], ids[0]);
            let mut sorted = after.clone();
            sorted.sort();
            let mut want = ids.clone();
            want.sort();
            assert_eq!(sorted, want);
        }
    }
}
