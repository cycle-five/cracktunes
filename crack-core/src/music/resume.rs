//! Picking the music queue back up after a restart (#595). Spec:
//! docs/superpowers/specs/2026-10-09-queue-resume-design.md
//!
//! Shutdown writes each playing guild's queue down ([`queue_shutdown`]); the
//! guild-create handler claims it and, if it is worth it, rejoins and rebuilds
//! the queue ([`queue_resume_guild`], Task 4).

use crate::db::queue_snapshot::{save_all, QueueSnapshot, SnapshotTrack};
use crate::guild::operations::GuildSettingsOperations;
use crate::music::ops::TRACK_INFO_TIMEOUT;
use crate::utils::{get_requesting_user, get_track_handle_metadata};
use crate::Data;
use serenity::all::{ChannelId, GuildId};
use songbird::tracks::{LoopState, PlayMode, TrackState};
use songbird::Call;
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
}
