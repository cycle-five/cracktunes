//! The text of the notice a track that fails to play leaves in the channel.
//!
//! Only one of the [`FailReason`]s ever reaches Discord: the raw error can
//! carry a tool's stderr (v0.17.2's leak), so the detail goes to the log.
use crate::format::{TrackLabel, INLINE_TITLE_MAX};
use crate::messages::{
    TRACK_FAILED, TRACK_FAILED_BROKE_OFF, TRACK_FAILED_FORMAT, TRACK_FAILED_MORE,
    TRACK_FAILED_OPEN, TRACK_FAILED_SEEK, TRACK_FAILED_TRACKS,
};
use songbird::tracks::PlayError;

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

/// `**title**: reason`, the title cut at `INLINE_TITLE_MAX` and escaped: it
/// is third-party text in a sentence. A blank title counts as none: a link
/// yt-dlp could not read (a members-only video, say) is queued with
/// `Some("")`, which rendered as a bare `****`.
fn entry(failure: &Failure) -> String {
    let title = TrackLabel {
        title: failure.title.clone(),
        ..TrackLabel::default()
    }
    .title_text(INLINE_TITLE_MAX);
    format!("**{title}**: {}", failure.reason.text())
}

/// The notice's text: one line for one track, a list for several.
#[must_use]
pub fn render_text(listed: &[Failure], more: usize) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn failed(title: &str, reason: FailReason) -> Failure {
        Failure {
            title: Some(title.to_owned()),
            reason,
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
            render_text(&[failed("Want You Bad", FailReason::Format)], 0),
            "⚠️ Couldn't play **Want You Bad**: that format isn't supported"
        );
    }

    #[test]
    fn several_failures_are_a_counted_list() {
        assert_eq!(
            render_text(
                &[
                    failed("A", FailReason::Format),
                    failed("B", FailReason::Open)
                ],
                0
            ),
            "⚠️ Couldn't play 2 tracks:\n• **A**: that format isn't supported\n• **B**: couldn't open the stream"
        );
    }

    /// An in-sentence title gets the spec's 60 characters (was the audit
    /// log's 40): 11 characters of prefix and 49 `a`s, then `…`.
    #[test]
    fn a_title_is_cut_escaped_and_untitled_has_a_name() {
        let long = format!("@everyone *{}", "a".repeat(100));
        let shown = render_text(&[failed(&long, FailReason::Open)], 0);
        assert_eq!(
            shown,
            format!(
                "⚠️ Couldn't play **\\@everyone \\*{}…**: couldn't open the stream",
                "a".repeat(49)
            )
        );
        let untitled = Failure {
            title: None,
            reason: FailReason::Open,
        };
        assert_eq!(
            render_text(&[untitled], 0),
            "⚠️ Couldn't play **(untitled)**: couldn't open the stream"
        );
    }

    /// What a members-only link produced on TuneTitan, 2026-10-06.
    #[test]
    fn a_blank_title_is_untitled() {
        for blank in ["", "  "] {
            assert_eq!(
                render_text(&[failed(blank, FailReason::Open)], 0),
                "⚠️ Couldn't play **(untitled)**: couldn't open the stream"
            );
        }
    }
}
