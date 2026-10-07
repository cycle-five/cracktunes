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
use crate::messaging::messages::{
    TRACK_FAILED, TRACK_FAILED_BROKE_OFF, TRACK_FAILED_FORMAT, TRACK_FAILED_MORE,
    TRACK_FAILED_OPEN, TRACK_FAILED_SEEK, TRACK_FAILED_TRACKS, TRACK_FAILED_UNTITLED,
};
use crate::messaging::status::{StatusTransport, TransportError};
use crate::music::audit_view::{cap, escape, TITLE_MAX};
use crate::utils::get_track_handle_metadata;
use crate::Data;
use serenity::all::{CreateEmbed, GenericChannelId, GuildId, MessageId};
use songbird::tracks::{PlayError, PlayMode, TrackHandle, TrackState};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A failure this soon after the last one joins its notice.
pub const WINDOW: Duration = Duration::from_secs(30);
/// A notice names at most this many tracks, then counts the rest.
pub const LISTED_MAX: usize = 10;

/// Why a track did not play, in words fit for the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// The stream could not be opened: yt-dlp or the source refused.
    Open,
    /// The stream opened, but nothing in it could be read.
    Format,
    /// It played, then a frame could not be decoded.
    BrokeOff,
    /// A seek inside it failed.
    Seek,
}

impl FailReason {
    #[must_use]
    pub fn of(err: &PlayError) -> Self {
        match err {
            PlayError::Create(_) => Self::Open,
            PlayError::Parse(_) => Self::Format,
            PlayError::Decode(_) => Self::BrokeOff,
            PlayError::Seek(_) => Self::Seek,
            // `PlayError` is non_exhaustive. Whatever a new variant means,
            // the stream did not play.
            _ => Self::Open,
        }
    }

    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Open => TRACK_FAILED_OPEN,
            Self::Format => TRACK_FAILED_FORMAT,
            Self::BrokeOff => TRACK_FAILED_BROKE_OFF,
            Self::Seek => TRACK_FAILED_SEEK,
        }
    }
}

/// One track that did not play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub title: Option<String>,
    pub reason: FailReason,
}

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

/// `**title**: reason`, the title cut and escaped: it is third-party text.
/// A blank title counts as none: a link yt-dlp could not read (a members-only
/// video, say) is queued with `Some("")`, which rendered as a bare `****`.
fn entry(failure: &Failure) -> String {
    let title = match failure.title.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => escape(&cap(t, TITLE_MAX)),
        _ => TRACK_FAILED_UNTITLED.to_owned(),
    };
    format!("**{title}**: {}", failure.reason.text())
}

/// The notice's text: one line for one track, a list for several.
#[must_use]
pub fn render(listed: &[Failure], more: usize) -> String {
    match (listed, more) {
        ([one], 0) => format!("{TRACK_FAILED} {}", entry(one)),
        _ => {
            let total = listed.len() + more;
            let mut out = format!("{TRACK_FAILED} {total} {TRACK_FAILED_TRACKS}:");
            for failure in listed {
                out.push_str("\n• ");
                out.push_str(&entry(failure));
            }
            if more > 0 {
                out.push_str(&format!("\n+{more} {TRACK_FAILED_MORE}"));
            }
            out
        },
    }
}

/// Add `failure` to the guild's notice in `channel`: edit the open one if it
/// is recent and in the same channel, else post a new one. Best effort -- a
/// notice that cannot be delivered is logged, never raised.
pub async fn report(
    transport: &dyn StatusTransport,
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
    let embed = CreateEmbed::new().description(render(&listed, more));

    if let Some(id) = open {
        match transport.edit(channel, id, embed.clone()).await {
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
    match transport.send(channel, embed).await {
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
    transport: &dyn StatusTransport,
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
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Send(u64, String),
        Edit(u64, u64, String),
    }

    /// A stand-in Discord that records what it was asked to show.
    #[derive(Default)]
    struct Fake {
        ops: std::sync::Mutex<Vec<Op>>,
        edit_error: std::sync::Mutex<Option<TransportError>>,
        send_error: std::sync::Mutex<Option<TransportError>>,
        sent: AtomicU64,
    }

    impl Fake {
        fn ops(&self) -> Vec<Op> {
            self.ops.lock().unwrap().clone()
        }
    }

    fn text(embed: &CreateEmbed<'static>) -> String {
        let value = serde_json::to_value(embed).expect("embed serializes");
        value["description"].as_str().unwrap_or_default().to_owned()
    }

    #[async_trait]
    impl StatusTransport for Fake {
        async fn send(
            &self,
            channel: GenericChannelId,
            embed: CreateEmbed<'static>,
        ) -> Result<MessageId, TransportError> {
            self.ops
                .lock()
                .unwrap()
                .push(Op::Send(channel.get(), text(&embed)));
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
            embed: CreateEmbed<'static>,
        ) -> Result<(), TransportError> {
            self.ops
                .lock()
                .unwrap()
                .push(Op::Edit(channel.get(), id.get(), text(&embed)));
            match self.edit_error.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }

        async fn delete(
            &self,
            _channel: GenericChannelId,
            _id: MessageId,
        ) -> Result<(), TransportError> {
            unreachable!("a failure notice is never deleted")
        }

        fn last_message_id(
            &self,
            _guild: GuildId,
            _channel: GenericChannelId,
        ) -> Option<MessageId> {
            None
        }
    }

    // ---- wording ----

    /// Pinned against literals, not the constants: a test comparing a
    /// constant to itself passes whatever the constant says.
    #[test]
    fn each_reason_reads_as_a_person_would_say_it() {
        assert_eq!(FailReason::Open.text(), "couldn't open the stream");
        assert_eq!(FailReason::Format.text(), "that format isn't supported");
        assert_eq!(FailReason::BrokeOff.text(), "the stream broke off partway");
        assert_eq!(FailReason::Seek.text(), "couldn't seek in it");
    }

    #[test]
    fn songbirds_errors_map_to_their_reasons() {
        use songbird::input::AudioStreamError;
        use symphonia::core::errors::Error as SymphoniaError;
        let sym = || Arc::new(SymphoniaError::Unsupported("adts: only 1 aac frame"));
        assert_eq!(
            FailReason::of(&PlayError::Create(Arc::new(AudioStreamError::Fail(
                "yt-dlp: secret stderr".into()
            )))),
            FailReason::Open
        );
        assert_eq!(FailReason::of(&PlayError::Parse(sym())), FailReason::Format);
        assert_eq!(
            FailReason::of(&PlayError::Decode(sym())),
            FailReason::BrokeOff
        );
        assert_eq!(FailReason::of(&PlayError::Seek(sym())), FailReason::Seek);
    }

    #[test]
    fn one_failure_is_one_line() {
        assert_eq!(
            render(&[failed("Want You Bad", FailReason::Format)], 0),
            "⚠️ Couldn't play **Want You Bad**: that format isn't supported"
        );
    }

    #[test]
    fn several_failures_are_a_counted_list() {
        assert_eq!(
            render(
                &[
                    failed("A", FailReason::Format),
                    failed("B", FailReason::Open)
                ],
                0
            ),
            "⚠️ Couldn't play 2 tracks:\n• **A**: that format isn't supported\n• **B**: couldn't open the stream"
        );
    }

    #[test]
    fn past_the_cap_the_rest_are_counted() {
        let listed: Vec<_> = (0..LISTED_MAX)
            .map(|i| failed(&format!("t{i}"), FailReason::Open))
            .collect();
        let shown = render(&listed, 3);
        assert!(shown.starts_with("⚠️ Couldn't play 13 tracks:"), "{shown}");
        assert!(shown.ends_with("\n+3 more"), "{shown}");
        assert_eq!(shown.matches("\n• ").count(), LISTED_MAX);
    }

    #[test]
    fn a_title_is_cut_escaped_and_untitled_has_a_name() {
        let long = format!("@everyone *{}", "a".repeat(TITLE_MAX));
        let shown = render(&[failed(&long, FailReason::Open)], 0);
        assert!(shown.contains("**\\@everyone \\*"), "{shown}");
        assert!(shown.contains("…**"), "{shown}");
        let untitled = Failure {
            title: None,
            reason: FailReason::Open,
        };
        assert_eq!(
            render(&[untitled], 0),
            "⚠️ Couldn't play **(untitled)**: couldn't open the stream"
        );
    }

    /// What a members-only link produced on TuneTitan, 2026-10-06.
    #[test]
    fn a_blank_title_is_untitled() {
        for blank in ["", "  "] {
            assert_eq!(
                render(&[failed(blank, FailReason::Open)], 0),
                "⚠️ Couldn't play **(untitled)**: couldn't open the stream"
            );
        }
    }

    // ---- edit or post ----

    #[tokio::test]
    async fn the_first_failure_posts_and_the_next_one_edits() {
        let fake = Fake::default();
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
        assert_eq!(
            fake.ops(),
            vec![
                Op::Send(5, "⚠️ Couldn't play **A**: that format isn't supported".into()),
                Op::Edit(
                    5,
                    1000,
                    "⚠️ Couldn't play 2 tracks:\n• **A**: that format isn't supported\n• **B**: couldn't open the stream".into()
                ),
            ]
        );
    }

    /// The window runs from the latest failure, so a steady trickle keeps
    /// editing one notice.
    #[tokio::test]
    async fn the_window_runs_from_the_latest_failure() {
        let fake = Fake::default();
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
        assert!(matches!(ops[0], Op::Send(5, _)));
        assert!(matches!(ops[1], Op::Edit(5, 1000, _)));
        assert!(matches!(ops[2], Op::Edit(5, 1000, _)));
    }

    #[tokio::test]
    async fn after_the_window_a_new_notice_starts_fresh() {
        let fake = Fake::default();
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
        assert_eq!(
            fake.ops()[1],
            Op::Send(5, "⚠️ Couldn't play **B**: couldn't open the stream".into())
        );
    }

    #[tokio::test]
    async fn a_failure_in_another_channel_starts_its_own_notice() {
        let fake = Fake::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Open), t0).await;
        report(&fake, &mut slot, ch(6), failed("B", FailReason::Open), t0).await;
        assert_eq!(
            fake.ops()[1],
            Op::Send(6, "⚠️ Couldn't play **B**: couldn't open the stream".into())
        );
    }

    #[tokio::test]
    async fn a_deleted_notice_is_posted_again_with_everything_so_far() {
        let fake = Fake::default();
        let mut slot = None;
        let t0 = Instant::now();
        report(&fake, &mut slot, ch(5), failed("A", FailReason::Open), t0).await;
        *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        report(&fake, &mut slot, ch(5), failed("B", FailReason::Open), t0).await;
        let ops = fake.ops();
        assert_eq!(ops.len(), 3, "{ops:?}");
        assert!(matches!(&ops[2], Op::Send(5, t) if t.starts_with("⚠️ Couldn't play 2 tracks:")));
        // The new message is the one later failures edit.
        *fake.edit_error.lock().unwrap() = None;
        report(&fake, &mut slot, ch(5), failed("C", FailReason::Open), t0).await;
        assert!(matches!(fake.ops()[3], Op::Edit(5, 1001, _)));
    }

    /// A whole dead playlist stays one message, and the list stops growing.
    #[tokio::test]
    async fn a_burst_past_the_cap_counts_the_rest() {
        let fake = Fake::default();
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
        assert_eq!(
            ops.iter().filter(|op| matches!(op, Op::Send(..))).count(),
            1
        );
        let Some(Op::Edit(5, 1000, last)) = ops.last() else {
            panic!("{ops:?}");
        };
        assert!(last.starts_with("⚠️ Couldn't play 12 tracks:"), "{last}");
        assert!(last.ends_with("\n+2 more"), "{last}");
        assert_eq!(last.matches("\n• ").count(), LISTED_MAX);
    }

    #[tokio::test]
    async fn an_undeliverable_notice_leaves_nothing_to_edit() {
        let fake = Fake::default();
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
        let fake = Fake::default();
        notify(
            &data,
            &fake,
            GUILD,
            ch(5),
            vec![failed("A", FailReason::Format)],
            Instant::now(),
        )
        .await;
        assert_eq!(
            fake.ops(),
            vec![Op::Send(
                5,
                "⚠️ Couldn't play **A**: that format isn't supported".into()
            )]
        );
    }

    #[tokio::test]
    async fn a_guild_in_a_gp_game_hears_nothing_from_here() {
        use crate::commands::music::{gp::GpReveal, gp_prompts::GpCategory};
        let data = data();
        data.gp_start(
            GUILD,
            serenity::all::UserId::new(7),
            "host".into(),
            serenity::all::ChannelId::new(8),
            ch(9),
            GpCategory::Nostalgia,
            vec!["p1".into()],
            60,
            None,
            GpReveal::default(),
            true,
            0,
        )
        .expect("game starts");
        let fake = Fake::default();
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
