//! The notice a track that fails to play leaves in the channel.
//!
//! songbird reports a track it could not open, read or keep decoding as an
//! `End` whose state is `Errored`, then moves on to the next one. Until this
//! the channel heard nothing: SoundCloud's undecodable streams (v0.20.1) just
//! emptied the queue in silence.
//!
//! Failures close together share one message, edited as they arrive, so a
//! playlist of dead links is one notice rather than fifty. The raw error never
//! reaches Discord -- it can carry a tool's stderr (v0.17.2's leak) -- only one
//! of the [`FailReason`]s, and the detail goes to the log.
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::{render, RenderCx};
use crate::messaging::transport::{Transport, TransportError};
use crate::utils::get_track_handle_metadata;
use crate::Data;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use songbird::tracks::{PlayMode, TrackHandle, TrackState};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use super::failure::{render_text, FailReason, Failure};

/// A failure this soon after the last one joins its notice.
pub const WINDOW: Duration = Duration::from_secs(30);
/// A notice names at most this many tracks, then counts the rest.
pub const LISTED_MAX: usize = 10;

/// The notice on screen in a guild, and what it lists.
#[derive(Debug, Clone)]
pub struct FailureNotice {
    channel: GenericChannelId,
    id: MessageId,
    last: Instant,
    listed: Vec<Failure>,
    more: usize,
}

impl FailureNotice {
    /// Does a failure in `channel` at `now` join this notice?
    fn continues(&self, channel: GenericChannelId, now: Instant) -> bool {
        self.channel == channel && now.saturating_duration_since(self.last) <= WINDOW
    }
}

/// The guild's notice, if one is still open to edits.
pub type NoticeSlot = Option<FailureNotice>;

impl Data {
    /// The guild's failure-notice slot, created on first use.
    ///
    /// 🪤 The `.clone()` matters: a dashmap reference held across the caller's
    /// `.lock().await` deadlocks the shard (see `lease.rs::lock_queue`).
    pub fn failure_notice_slot(&self, guild: GuildId) -> Arc<tokio::sync::Mutex<NoticeSlot>> {
        self.failure_notices
            .entry(guild)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
            .clone()
    }
}

/// Add `failure` to the guild's notice in `channel`: edit the open one if it
/// is recent and in the same channel, else post a new one. Best effort -- a
/// notice that cannot be delivered is logged, never raised.
pub async fn report(
    transport: &dyn Transport,
    slot: &mut NoticeSlot,
    channel: GenericChannelId,
    failure: Failure,
    now: Instant,
) {
    let (mut listed, mut more, open) = match slot.take() {
        Some(notice) if notice.continues(channel, now) => {
            (notice.listed, notice.more, Some(notice.id))
        },
        _ => (Vec::new(), 0, None),
    };
    if listed.len() < LISTED_MAX {
        listed.push(failure);
    } else {
        more += 1;
    }
    // An embed: a mention in a title never pings.
    let out = render(
        &CrackedMessage::TrackFailed {
            listed: listed.clone(),
            more,
        },
        &RenderCx::now(),
    );

    if let Some(id) = open {
        match transport.edit(channel, id, out.clone()).await {
            Ok(()) => {
                *slot = Some(FailureNotice {
                    channel,
                    id,
                    last: now,
                    listed,
                    more,
                });
                return;
            },
            // Deleted by hand or by `/clean`: post it again below.
            Err(TransportError::UnknownMessage) => {},
            Err(TransportError::Other(err)) => {
                tracing::warn!("track failed: could not edit {id} in {channel}: {err}");
            },
        }
    }
    match transport.send(channel, out).await {
        Ok(id) => {
            *slot = Some(FailureNotice {
                channel,
                id,
                last: now,
                listed,
                more,
            });
        },
        Err(err) => tracing::warn!("track failed: could not post to {channel}: {err:?}"),
    }
}

/// The tracks in an `End` event that ended in error, with their titles.
pub async fn failures(tracks: &[(&TrackState, &TrackHandle)]) -> Vec<Failure> {
    let mut out = Vec::new();
    for (state, handle) in tracks {
        let PlayMode::Errored(err) = &state.playing else {
            continue;
        };
        let title = get_track_handle_metadata(handle)
            .await
            .ok()
            .and_then(|meta| meta.title);
        // The full error stays here, in the log.
        tracing::info!("track {} failed: {err}", handle.uuid());
        out.push(Failure {
            title,
            reason: FailReason::of(err),
        });
    }
    out
}

/// Tell `channel` about `failures` in `guild`. A `/gp` game reports its own
/// dead songs in its reveal, so a guild it owns hears nothing from here.
pub async fn notify(
    data: &Data,
    transport: &dyn Transport,
    guild: GuildId,
    channel: GenericChannelId,
    failures: Vec<Failure>,
    now: Instant,
) {
    if failures.is_empty() || data.gp_is_active(guild) {
        return;
    }
    let slot = data.failure_notice_slot(guild);
    let mut slot = slot.lock().await;
    for failure in failures {
        report(transport, &mut slot, channel, failure, now).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::test_support::{FakeTransport, Op};
    use songbird::tracks::PlayError;

    const GUILD: GuildId = GuildId::new(1);

    fn ch(id: u64) -> GenericChannelId {
        GenericChannelId::new(id)
    }

    fn failed(title: &str, reason: FailReason) -> Failure {
        Failure {
            title: Some(title.to_owned()),
            reason,
        }
    }

    // ---- wording ----

    #[test]
    fn past_the_cap_the_rest_are_counted() {
        let listed: Vec<_> = (0..LISTED_MAX)
            .map(|i| failed(&format!("t{i}"), FailReason::Open))
            .collect();
        let shown = render_text(&listed, 3);
        assert!(shown.starts_with("⚠️ Couldn't play 13 tracks:"), "{shown}");
        assert!(shown.ends_with("\n+3 more"), "{shown}");
        assert_eq!(shown.matches("\n• ").count(), LISTED_MAX);
    }

    // ---- edit or post ----

    #[tokio::test]
    async fn the_first_failure_posts_and_the_next_one_edits() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Format), t0).await;
        report(
            &fake,
            &mut slot,
            ch(5),
            failed("B", FailReason::Open),
            t0 + WINDOW,
        )
        .await;
        assert_eq!(fake.ops(), vec![Op::Send(5), Op::Edit(5, 1000)]);
        assert_eq!(
            fake.texts(),
            vec![
                "⚠️ Couldn't play **A**: that format isn't supported",
                "⚠️ Couldn't play 2 tracks:\n• **A**: that format isn't supported\n• **B**: couldn't open the stream",
            ]
        );
    }

    /// The window runs from the latest failure, so a steady trickle keeps
    /// editing one notice.
    #[tokio::test]
    async fn the_window_runs_from_the_latest_failure() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        for i in 0..3u32 {
            report(
                &fake,
                &mut slot,
                ch(5),
                failed("A", FailReason::Open),
                t0 + WINDOW * i,
            )
            .await;
        }
        let ops = fake.ops();
        assert!(matches!(ops[0], Op::Send(5)));
        assert!(matches!(ops[1], Op::Edit(5, 1000)));
        assert!(matches!(ops[2], Op::Edit(5, 1000)));
    }

    #[tokio::test]
    async fn after_the_window_a_new_notice_starts_fresh() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Open), t0).await;
        report(
            &fake,
            &mut slot,
            ch(5),
            failed("B", FailReason::Open),
            t0 + WINDOW + Duration::from_millis(1),
        )
        .await;
        assert_eq!(fake.ops()[1], Op::Send(5));
        assert_eq!(
            fake.texts()[1],
            "⚠️ Couldn't play **B**: couldn't open the stream"
        );
    }

    #[tokio::test]
    async fn a_failure_in_another_channel_starts_its_own_notice() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Open), t0).await;
        report(&fake, &mut slot, ch(6), failed("B", FailReason::Open), t0).await;
        assert_eq!(fake.ops()[1], Op::Send(6));
        assert_eq!(
            fake.texts()[1],
            "⚠️ Couldn't play **B**: couldn't open the stream"
        );
    }

    #[tokio::test]
    async fn a_deleted_notice_is_posted_again_with_everything_so_far() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Open), t0).await;
        *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        report(&fake, &mut slot, ch(5), failed("B", FailReason::Open), t0).await;
        let ops = fake.ops();
        assert_eq!(ops.len(), 3, "{ops:?}");
        assert_eq!(ops[2], Op::Send(5));
        assert!(fake.texts()[2].starts_with("⚠️ Couldn't play 2 tracks:"));
        // The new message is the one later failures edit.
        *fake.edit_error.lock().unwrap() = None;
        report(&fake, &mut slot, ch(5), failed("C", FailReason::Open), t0).await;
        assert!(matches!(fake.ops()[3], Op::Edit(5, 1001)));
    }

    /// A whole dead playlist stays one message, and the list stops growing.
    #[tokio::test]
    async fn a_burst_past_the_cap_counts_the_rest() {
        let fake = FakeTransport::default();
        let mut slot = None;
        let t0 = Instant::now();
        for i in 0..LISTED_MAX + 2 {
            report(
                &fake,
                &mut slot,
                ch(5),
                failed(&format!("t{i}"), FailReason::Open),
                t0,
            )
            .await;
        }
        let ops = fake.ops();
        assert_eq!(ops.iter().filter(|op| matches!(op, Op::Send(_))).count(), 1);
        assert_eq!(ops.last(), Some(&Op::Edit(5, 1000)));
        let texts = fake.texts();
        let last = texts.last().expect("sent something");
        assert!(last.starts_with("⚠️ Couldn't play 12 tracks:"), "{last}");
        assert!(last.ends_with("\n+2 more"), "{last}");
        assert_eq!(last.matches("\n• ").count(), LISTED_MAX);
    }

    #[tokio::test]
    async fn an_undeliverable_notice_leaves_nothing_to_edit() {
        let fake = FakeTransport::default();
        let mut slot = None;
        *fake.send_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        report(
            &fake,
            &mut slot,
            ch(5),
            failed("A", FailReason::Open),
            Instant::now(),
        )
        .await;
        assert!(slot.is_none());
    }

    // ---- the guild ----

    fn data() -> Data {
        Data(Arc::new(crate::DataInner::default()))
    }

    #[tokio::test]
    async fn a_guild_hears_about_its_failed_tracks() {
        let data = data();
        let fake = FakeTransport::default();
        notify(
            &data,
            &fake,
            GUILD,
            ch(5),
            vec![failed("A", FailReason::Format)],
            Instant::now(),
        )
        .await;
        assert_eq!(fake.ops(), vec![Op::Send(5)]);
        assert_eq!(
            fake.texts(),
            vec!["⚠️ Couldn't play **A**: that format isn't supported"]
        );
    }

    #[tokio::test]
    async fn a_guild_in_a_gp_game_hears_nothing_from_here() {
        use crate::commands::music::gp::GpReveal;
        use crate::commands::music::gp_prompts::{GpCategories, GpCategory, GpPrompt};
        let data = data();
        data.gp_start(
            GUILD,
            serenity::all::UserId::new(7),
            "host".into(),
            serenity::all::ChannelId::new(8),
            ch(9),
            GpCategories::from_choice(GpCategory::Nostalgia).unwrap(),
            vec![GpPrompt {
                category: GpCategory::Nostalgia,
                text: "p1".into(),
            }],
            60,
            None,
            GpReveal::default(),
            true,
            0,
        )
        .expect("game starts");
        let fake = FakeTransport::default();
        notify(
            &data,
            &fake,
            GUILD,
            ch(5),
            vec![failed("A", FailReason::Format)],
            Instant::now(),
        )
        .await;
        assert_eq!(fake.ops(), vec![]);
    }

    /// An errored state is reported with the track's title; a track that
    /// ended normally in the same event is not.
    #[tokio::test]
    async fn only_errored_tracks_are_failures() {
        use crate::music::ops::test_support::queue_of;
        use songbird::tracks::{LoopState, ReadyState};
        let (_data, call, _ids, _rx) = queue_of(2).await;
        let handles = call.lock().await.queue().current_queue();
        let state = |playing| TrackState {
            playing,
            volume: 1.0,
            position: Duration::ZERO,
            play_time: Duration::ZERO,
            loops: LoopState::default(),
            ready: ReadyState::Uninitialised,
        };
        let errored = state(PlayMode::Errored(PlayError::Parse(Arc::new(
            symphonia::core::errors::Error::Unsupported("adts"),
        ))));
        let ended = state(PlayMode::End);
        let got = failures(&[(&errored, &handles[0]), (&ended, &handles[1])]).await;
        assert_eq!(got, vec![failed("t0", FailReason::Format)]);
    }
}
