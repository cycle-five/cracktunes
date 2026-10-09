//! Picking the music queue back up after a restart (#595). Spec:
//! docs/superpowers/specs/2026-10-09-queue-resume-design.md
//!
//! Shutdown writes each playing guild's queue down ([`queue_shutdown`]); the
//! guild-create handler claims it and, if it is worth it, rejoins and rebuilds
//! the queue ([`queue_resume_guild`], Task 4).

use crate::commands::music::gp_persist::vc_members;
use crate::commands::music_utils::{join_permitted, set_global_handlers_with};
use crate::db::queue_snapshot::claim;
use crate::db::queue_snapshot::{save_all, QueueSnapshot, SnapshotTrack};
use crate::errors::CrackedError;
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::courier::{post, Destination};
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::RenderCx;
use crate::messaging::status::{note_command_channel, show_now_playing};
use crate::messaging::transport::{DiscordTransport, Transport};
use crate::music::audit::{Actor, BotReason};
use crate::music::ops::TRACK_INFO_TIMEOUT;
use crate::music::ops::{pause_on, repeat_on, SEEK_TIMEOUT};
use crate::music::perms::ensure_can_join;
use crate::music::queue::{self, enqueue_resolved_tracks_back};
use crate::music::PlaybackOwner;
use crate::utils::{get_requesting_user, get_track_handle_metadata};
use crate::Data;
use crack_testing::ResolvedTrack;
use crack_types::SavedTrack;
use poise::serenity_prelude::Context as SerenityContext;
use serenity::all::{ChannelId, GenericChannelId, Guild, GuildId, MessageId, UserId};
use songbird::tracks::TrackHandle;
use songbird::tracks::{LoopState, PlayMode, TrackState};
use songbird::Call;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// (position, paused, looping) of the current track; a driver that did not
/// answer reads as just started, playing, not on repeat.
pub(crate) fn read_state(info: Option<&TrackState>) -> (Duration, bool, bool) {
    match info {
        Some(i) => (
            i.position,
            i.playing == PlayMode::Pause,
            i.loops == LoopState::Infinite,
        ),
        None => (Duration::ZERO, false, false),
    }
}

/// Position and repeat belong to the current track alone; `paused` is the
/// playback's. When the current track did not make it into the snapshot, the
/// next one starts from the top and does not loop.
pub(crate) fn current_state(
    first_kept: bool,
    state: (Duration, bool, bool),
) -> (Duration, bool, bool) {
    let (position, paused, looping) = state;
    if first_kept {
        (position, paused, looping)
    } else {
        (Duration::ZERO, paused, false)
    }
}

/// One handle's row, or `None` when a resume could not play it: no metadata,
/// or no source URL (a local file, an attachment that never resolved).
pub(crate) fn keep(
    meta: Option<songbird::input::AuxMetadata>,
    requester: Option<i64>,
) -> Option<SnapshotTrack> {
    let meta = meta?;
    let url = meta.source_url.filter(|u| !u.is_empty())?;
    Some(SnapshotTrack {
        url,
        title: meta.title,
        artist: meta.artist,
        duration_secs: meta.duration.map(|d| d.as_secs() as i64),
        requester,
    })
}

/// The kept rows in order, and whether the current track (index 0) is among
/// them.
pub(crate) fn gather(per_handle: Vec<Option<SnapshotTrack>>) -> (Vec<SnapshotTrack>, bool) {
    let first_kept = matches!(per_handle.first(), Some(Some(_)));
    (per_handle.into_iter().flatten().collect(), first_kept)
}

/// The guild's queue as a [`QueueSnapshot`]. `None` when there is nothing a
/// resume could play, or when a `/gp` game owns the guild (its own resume
/// brings it back).
pub(crate) async fn snapshot_call(
    data: &Data,
    guild: GuildId,
    voice: ChannelId,
    call: &Arc<Mutex<Call>>,
) -> Option<QueueSnapshot> {
    if data.gp_is_active(guild) {
        return None;
    }
    // 🔑 Clone the handles and let go of the call before any await on them.
    let handles = call.lock().await.queue().current_queue();
    let first = handles.first()?.clone();
    let mut per_handle = Vec::with_capacity(handles.len());
    for h in &handles {
        let meta = get_track_handle_metadata(h).await.ok();
        let requester = get_requesting_user(h).await.ok().map(|u| u.get() as i64);
        per_handle.push(keep(meta, requester));
    }
    let (tracks, first_kept) = gather(per_handle);
    if tracks.is_empty() {
        return None;
    }
    let info = tokio::time::timeout(TRACK_INFO_TIMEOUT, first.get_info())
        .await
        .ok()
        .and_then(Result::ok);
    let (position, paused, looping) = current_state(first_kept, read_state(info.as_ref()));
    let (status, last_command) = {
        let slot = data.status_slot(guild);
        let slot = slot.lock().await;
        (slot.message, slot.last_command_channel)
    };
    let text = data
        .get_music_channel(guild)
        .await
        .or(last_command)
        .or(status.map(|s| s.channel));
    Some(QueueSnapshot {
        guild_id: guild.get() as i64,
        voice_channel_id: voice.get() as i64,
        text_channel_id: text.map(|c| c.get() as i64),
        status_channel_id: status.map(|s| s.channel.get() as i64),
        status_message_id: status.map(|s| s.id.get() as i64),
        position_ms: position.as_millis() as i64,
        paused,
        looping,
        autoplay: data.get_autoplay(guild).await,
        tracks,
    })
}

/// Write down every playing guild's queue, for the resume after this
/// shutdown. Bounded by `budget`: on timeout nothing is written rather than a
/// partial set. A no-op without a database.
pub async fn queue_shutdown(data: &Data, budget: Duration) {
    let Some(pool) = data.database_pool.clone() else {
        return;
    };
    let work = async {
        let calls: Vec<_> = data.songbird.iter().collect();
        let mut pending = Vec::new();
        for (guild, call) in calls {
            let voice = {
                let h = call.lock().await;
                if h.current_connection().is_none() {
                    continue;
                }
                h.current_channel()
            };
            let Some(voice) = voice else { continue };
            let guild = GuildId::new(guild.get());
            let voice = ChannelId::new(voice.get());
            pending.push(async move { snapshot_call(data, guild, voice, &call).await });
        }
        let snapshots: Vec<QueueSnapshot> = futures::future::join_all(pending)
            .await
            .into_iter()
            .flatten()
            .collect();
        if snapshots.is_empty() {
            return;
        }
        match save_all(&pool, &snapshots).await {
            Ok(()) => tracing::info!("queue: {} queue(s) saved for the restart", snapshots.len()),
            Err(e) => tracing::warn!("queue: saving queues at shutdown: {e}"),
        }
    };
    if tokio::time::timeout(budget, work).await.is_err() {
        tracing::warn!("queue: saving queues did not finish within {budget:?}; none saved");
    }
}

pub const QUEUE_RESUME_WINDOW_SECS: i64 = 300;
pub const QUEUE_RESUME_MIN_SEEK: Duration = Duration::from_secs(5);
pub const QUEUE_RESUME_REWIND: Duration = Duration::from_secs(3);

/// What to do with a claimed snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResumePlan {
    Resume,
    /// Down longer than [`QUEUE_RESUME_WINDOW_SECS`]: the room has moved on.
    TooLate,
    /// Nobody (but bots) in the voice channel to hear it.
    Empty,
    /// A `/gp` game owns the guild.
    GameRunning,
}

pub(crate) fn plan(age_secs: i64, listeners: usize, game_running: bool) -> ResumePlan {
    if game_running {
        ResumePlan::GameRunning
    } else if age_secs > QUEUE_RESUME_WINDOW_SECS {
        ResumePlan::TooLate
    } else if listeners == 0 {
        ResumePlan::Empty
    } else {
        ResumePlan::Resume
    }
}

/// How a seek of the current track went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeekOutcome {
    Done,
    /// songbird documents a failed seek as fatal: it removes the track.
    Failed,
    TimedOut,
}

pub(crate) async fn seek_for_real(track: TrackHandle, to: Duration) -> SeekOutcome {
    match tokio::time::timeout(SEEK_TIMEOUT, track.seek(to).result_async()).await {
        Ok(Ok(_)) => SeekOutcome::Done,
        Ok(Err(e)) => {
            tracing::warn!("queue: resuming at {to:?} failed ({e}); playing from the top");
            SeekOutcome::Failed
        },
        Err(_) => SeekOutcome::TimedOut,
    }
}

fn resolved(t: &SnapshotTrack) -> ResolvedTrack<'static> {
    let saved = SavedTrack::from_secs(
        t.url.clone(),
        t.title.clone(),
        t.artist.clone(),
        t.duration_secs,
    );
    let r = ResolvedTrack::from_saved(&saved);
    match t.requester {
        Some(id) if id > 0 => r.with_user_id(UserId::new(id as u64)),
        _ => r,
    }
}

/// Rebuild the queue from `snapshot` on `call`, as the bot. One guard for the
/// whole rebuild, so nothing interleaves with it: the current track, sought to
/// just before where it was (a failed seek has removed it, so it goes back in
/// fresh and plays from the top), then the rest, then repeat and paused.
/// Autoplay last, after the guard, so a refill cannot race the rebuild.
#[expect(
    clippy::disallowed_methods,
    reason = "the restore removes its own dead handle under its guard; ops::remove_on refuses the playing track"
)]
pub(crate) async fn restore<F, Fut>(
    data: &Data,
    guild: GuildId,
    call: &Arc<Mutex<Call>>,
    snapshot: &QueueSnapshot,
    seek: F,
) -> Result<(), CrackedError>
where
    F: FnOnce(TrackHandle, Duration) -> Fut,
    Fut: Future<Output = SeekOutcome>,
{
    let Some((first, rest)) = snapshot.tracks.split_first() else {
        return Ok(());
    };
    let guard = data
        .lock_queue(guild, PlaybackOwner::Free, Actor::bot(BotReason::Resume))
        .await?;
    let http = data.http_client.clone();
    let current =
        enqueue_resolved_tracks_back(&guard, call, vec![resolved(first)], http.clone()).await?;
    let position = Duration::from_millis(snapshot.position_ms.max(0) as u64);
    if let Some(handle) = current.handles.first().cloned() {
        if position >= QUEUE_RESUME_MIN_SEEK {
            let to = position.saturating_sub(QUEUE_RESUME_REWIND);
            if seek(handle.clone(), to).await == SeekOutcome::Failed {
                // songbird may not have removed it yet; either way, it goes.
                {
                    let handler = call.lock().await;
                    let at = handler
                        .queue()
                        .current_queue()
                        .iter()
                        .position(|t| t.uuid() == handle.uuid());
                    if let Some(at) = at {
                        queue::remove_at(&guard, &handler, at);
                    }
                }
                enqueue_resolved_tracks_back(&guard, call, vec![resolved(first)], http.clone())
                    .await?;
            }
        }
    }
    enqueue_resolved_tracks_back(&guard, call, rest.iter().map(resolved).collect(), http).await?;
    if snapshot.looping {
        let _ = repeat_on(&guard, call, Some(true)).await;
    }
    if snapshot.paused {
        let _ = pause_on(&guard, call).await;
    }
    drop(guard);
    data.set_autoplay(guild, snapshot.autoplay).await;
    Ok(())
}

pub(crate) async fn retire_old_status(
    transport: &dyn Transport,
    guild: GuildId,
    s: &QueueSnapshot,
) {
    let (Some(channel), Some(id)) = (s.status_channel_id, s.status_message_id) else {
        return;
    };
    let (channel, id) = (
        GenericChannelId::new(channel as u64),
        MessageId::new(id as u64),
    );
    if let Err(e) = transport.clear_components(channel, id).await {
        tracing::warn!("queue: taking the buttons off the pre-restart status in {guild}: {e:?}");
    }
}

pub(crate) async fn announce_resumed(
    data: &Data,
    transport: &dyn Transport,
    guild: GuildId,
    s: &QueueSnapshot,
) {
    let Some(text) = s.text_channel_id else {
        return;
    };
    let text = GenericChannelId::new(text as u64);
    note_command_channel(data, guild, text).await;
    post(
        data,
        transport,
        Destination::Channel(text),
        &CrackedMessage::QueueResumed,
        &RenderCx::now(),
    )
    .await;
}

/// Bring back the guild's queue if one was written down at the last shutdown.
/// Runs from the guild-create handler after `/gp`'s resume; the claim makes a
/// later reconnect's call a quick no.
pub async fn queue_resume_guild(data: &Data, ctx: &SerenityContext, guild: &Guild) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    let guild_id = guild.id;
    let claimed = match claim(pool, guild_id.get() as i64).await {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("queue: claiming the saved queue in {guild_id}: {e}");
            return;
        },
    };
    let snapshot = claimed.snapshot;
    let transport: Arc<dyn Transport> = Arc::new(DiscordTransport::of(ctx));
    retire_old_status(&*transport, guild_id, &snapshot).await;
    let voice = ChannelId::new(snapshot.voice_channel_id as u64);
    let me = ctx.cache.current_user().id;
    match plan(
        claimed.age_secs,
        vc_members(guild, voice, me),
        data.gp_is_active(guild_id),
    ) {
        ResumePlan::Resume => {},
        other => {
            tracing::info!("queue: not resuming the queue in {guild_id}: {other:?}");
            return;
        },
    }
    let permit = match ensure_can_join(&ctx.cache, guild_id, voice) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("queue: cannot rejoin {voice} in {guild_id} to resume: {e}");
            return;
        },
    };
    let call = match join_permitted(data, &data.songbird, permit).await {
        Ok(call) => call,
        Err(e) => {
            tracing::warn!("queue: rejoining {voice} in {guild_id} to resume: {e}");
            return;
        },
    };
    let text = snapshot
        .text_channel_id
        .map(|c| GenericChannelId::new(c as u64))
        .unwrap_or_else(|| GenericChannelId::new(voice.get()));
    set_global_handlers_with(ctx, Arc::new(data.clone()), call.clone(), guild_id, text).await;
    tracing::info!(
        "queue: resuming {} track(s) in {guild_id}",
        snapshot.tracks.len()
    );
    // The rebuild waits on a seek; the guild-create handler does not.
    let (data, http, cache) = (data.clone(), ctx.http.clone(), ctx.cache.clone());
    tokio::spawn(async move {
        if let Err(e) = restore(&data, guild_id, &call, &snapshot, seek_for_real).await {
            tracing::warn!("queue: rebuilding the queue in {guild_id}: {e}");
            return;
        }
        announce_resumed(&data, &*transport, guild_id, &snapshot).await;
        show_now_playing(&data, http, cache, guild_id, &call).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::ops::test_support::{offline_call, GUILD};
    use crate::music::{
        audit::{Actor, BotReason},
        queue, PlaybackOwner,
    };
    use serenity::all::{GenericChannelId, MessageId, UserId};
    use songbird::tracks::{LoopState, PlayMode, TrackState};

    const VC: ChannelId = ChannelId::new(10);

    use crate::messaging::test_support::{FakeTransport, Op};
    use crate::messaging::transport::TransportError;

    fn with_status(channel: Option<i64>, id: Option<i64>) -> QueueSnapshot {
        let mut s = snap(0, false, false, false);
        s.status_channel_id = channel;
        s.status_message_id = id;
        s
    }

    #[tokio::test]
    async fn the_old_status_message_loses_its_buttons() {
        let t = FakeTransport::default();
        retire_old_status(&t, GUILD, &with_status(Some(30), Some(500))).await;
        assert_eq!(t.ops(), vec![Op::ClearComponents(30, 500)]);
    }

    #[tokio::test]
    async fn no_status_message_nothing_to_retire() {
        let t = FakeTransport::default();
        retire_old_status(&t, GUILD, &with_status(None, None)).await;
        retire_old_status(&t, GUILD, &with_status(Some(30), None)).await;
        assert!(t.ops().is_empty());
    }

    /// Review Focus 5.
    #[tokio::test]
    async fn retiring_a_status_message_that_is_gone_carries_on() {
        let t = FakeTransport::default();
        *t.edit_error.lock().unwrap() = Some(TransportError::Other("Unknown Message".into()));
        retire_old_status(&t, GUILD, &with_status(Some(30), Some(500))).await;
        assert_eq!(t.ops(), vec![Op::ClearComponents(30, 500)]);
    }

    #[tokio::test]
    async fn the_resume_is_announced_in_the_text_channel() {
        let data = Data::default();
        let t = FakeTransport::default();
        announce_resumed(&data, &t, GUILD, &snap(0, false, false, false)).await;
        assert_eq!(t.ops(), vec![Op::Send(20)]);
        assert_eq!(
            t.texts(),
            vec!["♻️ Back after a restart — picking up where we left off.".to_string()]
        );
        assert_eq!(
            data.status_slot(GUILD).lock().await.last_command_channel,
            Some(GenericChannelId::new(20)),
            "the status and later notices land there too"
        );
    }

    #[tokio::test]
    async fn no_text_channel_no_announce() {
        let data = Data::default();
        let t = FakeTransport::default();
        let mut s = snap(0, false, false, false);
        s.text_channel_id = None;
        announce_resumed(&data, &t, GUILD, &s).await;
        assert!(t.ops().is_empty());
    }

    fn meta(n: usize, url: Option<&str>) -> songbird::input::AuxMetadata {
        songbird::input::AuxMetadata {
            title: Some(format!("t{n}")),
            artist: Some(format!("a{n}")),
            duration: Some(Duration::from_secs(100 + n as u64)),
            source_url: url.map(str::to_string),
            ..Default::default()
        }
    }

    /// Queue `tracks` (url, requester) on an offline call.
    async fn queued(tracks: &[(Option<&str>, Option<u64>)]) -> (Data, Arc<Mutex<Call>>) {
        let data = Data::default();
        let call = offline_call();
        let guard = data
            .lock_queue(GUILD, PlaybackOwner::Free, Actor::bot(BotReason::Autopause))
            .await
            .unwrap();
        for (i, (url, who)) in tracks.iter().enumerate() {
            let t = queue::new_track(
                songbird::input::File::new(format!("/nonexistent/{i}.opus")).into(),
                Some(meta(i, *url)),
                who.map(UserId::new),
            );
            queue::enqueue_track_back(&guard, &call, t, None).await;
        }
        (data, call)
    }

    #[test]
    fn state_reads_position_paused_and_repeat() {
        let paused = TrackState {
            playing: PlayMode::Pause,
            position: Duration::from_secs(61),
            loops: LoopState::Infinite,
            ..Default::default()
        };
        assert_eq!(
            read_state(Some(&paused)),
            (Duration::from_secs(61), true, true)
        );
        let playing = TrackState {
            playing: PlayMode::Play,
            position: Duration::from_secs(5),
            ..Default::default()
        };
        assert_eq!(
            read_state(Some(&playing)),
            (Duration::from_secs(5), false, false)
        );
        assert_eq!(read_state(None), (Duration::ZERO, false, false));
        // Each flag alone, so swapping the two cannot pass.
        let only_paused = TrackState {
            playing: PlayMode::Pause,
            ..Default::default()
        };
        assert_eq!(
            read_state(Some(&only_paused)),
            (Duration::ZERO, true, false)
        );
        let only_looping = TrackState {
            loops: LoopState::Infinite,
            ..Default::default()
        };
        assert_eq!(
            read_state(Some(&only_looping)),
            (Duration::ZERO, false, true)
        );
    }

    #[tokio::test]
    async fn a_snapshot_holds_the_queue_in_order_with_requesters() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) = queued(&[
                (Some("https://www.youtube.com/watch?v=a"), Some(100)),
                (Some("https://www.youtube.com/watch?v=b"), None),
            ])
            .await;
            crate::messaging::status::note_command_channel(&data, GUILD, GenericChannelId::new(20))
                .await;
            let s = snapshot_call(&data, GUILD, VC, &call)
                .await
                .expect("a queue");
            assert_eq!(s.guild_id, GUILD.get() as i64);
            assert_eq!(s.voice_channel_id, VC.get() as i64);
            assert_eq!(s.text_channel_id, Some(20));
            assert_eq!(
                s.tracks,
                vec![
                    SnapshotTrack {
                        url: "https://www.youtube.com/watch?v=a".into(),
                        title: Some("t0".into()),
                        artist: Some("a0".into()),
                        duration_secs: Some(100),
                        requester: Some(100),
                    },
                    SnapshotTrack {
                        url: "https://www.youtube.com/watch?v=b".into(),
                        title: Some("t1".into()),
                        artist: Some("a1".into()),
                        duration_secs: Some(101),
                        // `new_track` records a missing requester as user 1.
                        requester: Some(1),
                    },
                ]
            );
        })
        .await
        .expect("get_info is bounded");
    }

    /// Review Focus 2.
    #[tokio::test]
    async fn a_track_without_a_source_url_is_left_out() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) = queued(&[
                (None, Some(100)),
                (Some("https://www.youtube.com/watch?v=b"), Some(200)),
            ])
            .await;
            let s = snapshot_call(&data, GUILD, VC, &call)
                .await
                .expect("one left");
            assert_eq!(s.tracks.len(), 1);
            assert_eq!(s.tracks[0].requester, Some(200));
            let (data, call) = queued(&[(None, Some(100))]).await;
            assert!(
                snapshot_call(&data, GUILD, VC, &call).await.is_none(),
                "nothing playable"
            );
        })
        .await
        .expect("get_info is bounded");
    }

    #[tokio::test]
    async fn an_empty_queue_has_no_snapshot() {
        let data = Data::default();
        assert!(snapshot_call(&data, GUILD, VC, &offline_call())
            .await
            .is_none());
    }

    #[tokio::test]
    async fn the_status_message_on_screen_is_written_down() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) =
                queued(&[(Some("https://www.youtube.com/watch?v=a"), Some(100))]).await;
            data.status_slot(GUILD).lock().await.message =
                Some(crate::messaging::status::StatusMessage {
                    channel: GenericChannelId::new(30),
                    id: MessageId::new(500),
                    phase: crate::messaging::status::Phase::Playing,
                });
            let s = snapshot_call(&data, GUILD, VC, &call).await.unwrap();
            assert_eq!(
                (s.status_channel_id, s.status_message_id),
                (Some(30), Some(500))
            );
            assert_eq!(
                s.text_channel_id,
                Some(30),
                "no music or command channel: the status's"
            );
        })
        .await
        .expect("get_info is bounded");
    }

    /// A `/gp` game owns the guild: its queue is not this module's to write.
    #[tokio::test]
    async fn a_guild_with_a_gp_game_is_not_snapshotted() {
        use crate::commands::music::gp_prompts::{GpCategories, GpCategory, GpPrompt};
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) =
                queued(&[(Some("https://www.youtube.com/watch?v=a"), Some(100))]).await;
            assert!(
                snapshot_call(&data, GUILD, VC, &call).await.is_some(),
                "control: no game"
            );
            data.gp_start(
                GUILD,
                UserId::new(100),
                "alice".into(),
                VC,
                GenericChannelId::new(20),
                GpCategories::from_choice(GpCategory::Nostalgia).unwrap(),
                vec![GpPrompt {
                    category: GpCategory::Nostalgia,
                    text: "x".into(),
                }],
                30,
                None,
                Default::default(),
                true,
                0,
            )
            .unwrap();
            assert!(data.gp_is_active(GUILD));
            assert!(snapshot_call(&data, GUILD, VC, &call).await.is_none());
        })
        .await
        .expect("get_info is bounded");
    }

    #[tokio::test]
    async fn the_command_channel_beats_the_status_channel_for_text() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) =
                queued(&[(Some("https://www.youtube.com/watch?v=a"), Some(100))]).await;
            crate::messaging::status::note_command_channel(&data, GUILD, GenericChannelId::new(20))
                .await;
            data.status_slot(GUILD).lock().await.message =
                Some(crate::messaging::status::StatusMessage {
                    channel: GenericChannelId::new(30),
                    id: MessageId::new(500),
                    phase: crate::messaging::status::Phase::Playing,
                });
            let s = snapshot_call(&data, GUILD, VC, &call).await.unwrap();
            assert_eq!(s.text_channel_id, Some(20));
            assert_eq!(s.status_channel_id, Some(30));
        })
        .await
        .expect("get_info is bounded");
    }

    #[test]
    fn a_dropped_current_track_does_not_lend_its_position_to_the_next() {
        let state = (Duration::from_secs(61), true, true);
        assert_eq!(current_state(true, state), state);
        assert_eq!(current_state(false, state), (Duration::ZERO, true, false));
    }

    #[tokio::test]
    async fn a_snapshot_whose_current_track_was_dropped_keeps_the_rest() {
        tokio::time::timeout(Duration::from_secs(5), async {
            let (data, call) = queued(&[
                (None, Some(100)),
                (Some("https://www.youtube.com/watch?v=b"), Some(200)),
            ])
            .await;
            let s = snapshot_call(&data, GUILD, VC, &call).await.unwrap();
            // Offline `get_info` reads defaults, so position is pinned by the
            // pure `current_state` and `gather` tests, not here.
            assert_eq!(s.tracks.len(), 1);
        })
        .await
        .expect("get_info is bounded");
    }

    fn row(n: usize) -> SnapshotTrack {
        SnapshotTrack {
            url: format!("u{n}"),
            title: None,
            artist: None,
            duration_secs: None,
            requester: None,
        }
    }

    #[test]
    fn gather_says_whether_the_current_track_was_kept() {
        assert_eq!(gather(vec![None, Some(row(1))]), (vec![row(1)], false));
        assert_eq!(
            gather(vec![Some(row(0)), None, Some(row(2))]),
            (vec![row(0), row(2)], true)
        );
        assert_eq!(gather(vec![]), (vec![], false));
    }

    #[test]
    fn keep_needs_a_source_url() {
        assert_eq!(keep(None, Some(1)), None, "no metadata");
        assert_eq!(keep(Some(meta(0, None)), Some(1)), None);
        assert_eq!(keep(Some(meta(0, Some(""))), Some(1)), None);
        assert_eq!(
            keep(Some(meta(3, Some("https://x/y"))), Some(7)),
            Some(SnapshotTrack {
                url: "https://x/y".into(),
                title: Some("t3".into()),
                artist: Some("a3".into()),
                duration_secs: Some(103),
                requester: Some(7),
            })
        );
    }

    #[test]
    fn the_decision() {
        assert_eq!(plan(10, 2, false), ResumePlan::Resume);
        assert_eq!(plan(10, 0, false), ResumePlan::Empty);
        assert_eq!(plan(400, 2, false), ResumePlan::TooLate);
        assert_eq!(plan(10, 2, true), ResumePlan::GameRunning);
        assert_eq!(
            plan(400, 0, true),
            ResumePlan::GameRunning,
            "the game first"
        );
        assert_eq!(
            plan(400, 0, false),
            ResumePlan::TooLate,
            "too late before empty"
        );
    }

    /// Review Focus 4.
    #[test]
    fn the_resume_window_is_inclusive_at_five_minutes() {
        assert_eq!(plan(300, 1, false), ResumePlan::Resume);
        assert_eq!(plan(301, 1, false), ResumePlan::TooLate);
    }

    fn saved(n: usize, requester: Option<i64>) -> SnapshotTrack {
        SnapshotTrack {
            url: format!("https://www.youtube.com/watch?v=s{n}"),
            title: Some(format!("s{n}")),
            artist: None,
            duration_secs: Some(200),
            requester,
        }
    }

    fn snap(position_ms: i64, paused: bool, looping: bool, autoplay: bool) -> QueueSnapshot {
        QueueSnapshot {
            guild_id: GUILD.get() as i64,
            voice_channel_id: VC.get() as i64,
            text_channel_id: Some(20),
            status_channel_id: None,
            status_message_id: None,
            position_ms,
            paused,
            looping,
            autoplay,
            tracks: vec![saved(0, Some(100)), saved(1, Some(200)), saved(2, None)],
        }
    }

    /// What is queued now: (title, requester) in order.
    async fn now_queued(call: &Arc<Mutex<Call>>) -> Vec<(String, u64)> {
        let handles = call.lock().await.queue().current_queue();
        let mut out = Vec::new();
        for h in &handles {
            let title = get_track_handle_metadata(h).await.unwrap().title.unwrap();
            let who = get_requesting_user(h).await.unwrap().get();
            out.push((title, who));
        }
        out
    }

    fn data_with_audit() -> (
        Data,
        tokio::sync::mpsc::Receiver<crate::music::audit::AuditEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        (
            Data(Arc::new(crate::DataInner {
                audit_tx: Some(tx),
                ..Default::default()
            })),
            rx,
        )
    }

    #[tokio::test]
    async fn a_restore_rebuilds_the_queue_in_order_with_requesters() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        restore(
            &data,
            GUILD,
            &call,
            &snap(61_000, false, false, false),
            |_, _| async { SeekOutcome::Done },
        )
        .await
        .unwrap();
        assert_eq!(
            now_queued(&call).await,
            vec![("s0".into(), 100), ("s1".into(), 200), ("s2".into(), 1)]
        );
    }

    #[tokio::test]
    async fn the_current_track_is_sought_to_just_before_where_it_was() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        let asked = std::sync::Mutex::new(None);
        restore(
            &data,
            GUILD,
            &call,
            &snap(61_000, false, false, false),
            |_, to| {
                *asked.lock().unwrap() = Some(to);
                async { SeekOutcome::Done }
            },
        )
        .await
        .unwrap();
        assert_eq!(*asked.lock().unwrap(), Some(Duration::from_millis(58_000)));
    }

    #[tokio::test]
    async fn no_seek_for_a_track_that_had_barely_started() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        let asked = std::sync::Mutex::new(false);
        restore(
            &data,
            GUILD,
            &call,
            &snap(4_999, false, false, false),
            |_, _| {
                *asked.lock().unwrap() = true;
                async { SeekOutcome::Done }
            },
        )
        .await
        .unwrap();
        assert!(!*asked.lock().unwrap());
    }

    /// The 5 s threshold is inclusive: exactly 5 000 ms is sought (to 2 000).
    #[tokio::test]
    async fn exactly_five_seconds_is_sought() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        let asked = std::sync::Mutex::new(None);
        restore(
            &data,
            GUILD,
            &call,
            &snap(5_000, false, false, false),
            |_, to| {
                *asked.lock().unwrap() = Some(to);
                async { SeekOutcome::Done }
            },
        )
        .await
        .unwrap();
        assert_eq!(*asked.lock().unwrap(), Some(Duration::from_millis(2_000)));
    }

    /// Review Focus 3: songbird removes a track whose seek failed; the restore
    /// puts the song back first, fresh, rather than skipping it.
    #[tokio::test]
    async fn a_failed_seek_requeues_the_current_track_first() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        let dead = std::sync::Mutex::new(None);
        restore(
            &data,
            GUILD,
            &call,
            &snap(61_000, false, false, false),
            |h, _| {
                *dead.lock().unwrap() = Some(h.uuid());
                async { SeekOutcome::Failed }
            },
        )
        .await
        .unwrap();
        let ids: Vec<_> = call
            .lock()
            .await
            .queue()
            .current_queue()
            .iter()
            .map(|h| h.uuid())
            .collect();
        assert_eq!(ids.len(), 3, "the dead handle is gone, nothing doubled");
        assert_ne!(Some(ids[0]), *dead.lock().unwrap(), "a fresh track");
        assert_eq!(
            now_queued(&call).await,
            vec![("s0".into(), 100), ("s1".into(), 200), ("s2".into(), 1)]
        );
    }

    #[tokio::test]
    async fn a_timed_out_seek_leaves_the_track_where_it_is() {
        let (data, _rx) = data_with_audit();
        let call = offline_call();
        let first = std::sync::Mutex::new(None);
        restore(
            &data,
            GUILD,
            &call,
            &snap(61_000, false, false, false),
            |h, _| {
                *first.lock().unwrap() = Some(h.uuid());
                async { SeekOutcome::TimedOut }
            },
        )
        .await
        .unwrap();
        let ids: Vec<_> = call
            .lock()
            .await
            .queue()
            .current_queue()
            .iter()
            .map(|h| h.uuid())
            .collect();
        assert_eq!(Some(ids[0]), *first.lock().unwrap());
        assert_eq!(ids.len(), 3);
    }

    #[tokio::test]
    async fn repeat_paused_and_autoplay_come_back() {
        let (data, mut rx) = data_with_audit();
        let call = offline_call();
        restore(
            &data,
            GUILD,
            &call,
            &snap(0, true, true, true),
            |_, _| async { SeekOutcome::Done },
        )
        .await
        .unwrap();
        let actions: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|e| e.action)
            .collect();
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, crate::music::audit::Action::Repeat { on: true })),
            "{actions:?}"
        );
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, crate::music::audit::Action::Pause)),
            "{actions:?}"
        );
        assert!(data.get_autoplay(GUILD).await);
        let (data, _rx) = data_with_audit();
        restore(
            &data,
            GUILD,
            &offline_call(),
            &snap(0, false, false, false),
            |_, _| async { SeekOutcome::Done },
        )
        .await
        .unwrap();
        assert!(!data.get_autoplay(GUILD).await);
    }

    #[tokio::test]
    async fn the_restore_is_recorded_as_the_bot_resuming() {
        let (data, mut rx) = data_with_audit();
        restore(
            &data,
            GUILD,
            &offline_call(),
            &snap(0, false, false, false),
            |_, _| async { SeekOutcome::Done },
        )
        .await
        .unwrap();
        let first = rx.try_recv().expect("an audit event");
        assert_eq!(first.actor.command(), "resume");
    }
}
