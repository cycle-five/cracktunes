//! The one place a track title, link, duration or URL becomes Discord text.
//!
//! Every message about a track goes through here, so an empty title, a
//! markdown character in a title, a relative URL or an unknown duration is
//! handled once. Before this, `SkipTo` printed "Skipped to **!" for a track
//! with an empty title, and `/play` printed "Track duration: 00:00".
use crate::messaging::messages::{
    PROGRESS_ENDS, PROGRESS_PAUSED, PROGRESS_PAUSED_AT, PROGRESS_STARTED, TRACK_UNTITLED,
};
use crate::music::audit::TrackRef;
use songbird::input::AuxMetadata;
use std::time::Duration;
use url::Url;

/// A title inside a sentence ("Skipped to **…**").
pub const INLINE_TITLE_MAX: usize = 60;
/// Discord's limits, in characters.
pub const EMBED_TITLE_MAX: usize = 256;
pub const CONTENT_MAX: usize = 2000;
pub const DESCRIPTION_MAX: usize = 4096;
pub const FIELD_MAX: usize = 1024;
pub const AUTHOR_MAX: usize = 256;

/// What a message knows about a track. Raw: everything is checked and
/// escaped when it is rendered, never before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackLabel {
    pub title: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
}

impl TrackLabel {
    #[must_use]
    pub fn from_metadata(meta: &AuxMetadata) -> Self {
        Self {
            title: meta.title.clone(),
            url: meta.source_url.clone(),
            duration: meta.duration,
        }
    }

    #[must_use]
    pub fn from_ref(track: &TrackRef) -> Self {
        Self {
            title: track.title.clone(),
            url: track.url.clone(),
            duration: None,
        }
    }

    /// The title for display: trimmed, `(untitled)` when blank, cut at `max`
    /// characters with `…`, then escaped -- it is third-party text.
    #[must_use]
    pub fn title_text(&self, max: usize) -> String {
        match self.title.as_deref().map(str::trim) {
            Some(t) if !t.is_empty() => escape(&cap(t, max)),
            _ => TRACK_UNTITLED.to_owned(),
        }
    }

    /// `[**title**](url)` when the URL is http(s), else `**title**`.
    #[must_use]
    pub fn linked(&self, max: usize) -> String {
        let title = self.title_text(max);
        match http_url(self.url.as_deref()) {
            Some(url) => format!("[**{title}**]({})", link_target(&url)),
            None => format!("**{title}**"),
        }
    }
}

/// A URL as a markdown link target: parentheses would end the link early.
fn link_target(url: &Url) -> String {
    url.as_str().replace('(', "%28").replace(')', "%29")
}

/// `raw` cut at `max` characters with `…`, or whole if it fits. Not escaped.
#[must_use]
pub fn cap(raw: &str, max: usize) -> String {
    let cut: String = raw.chars().take(max).collect();
    if raw.chars().count() > max {
        format!("{cut}…")
    } else {
        cut
    }
}

/// `s` cut so that it, `…` included, is at most `max` characters: Discord
/// rejects a message over its limits, and a trimmed one is better than none.
#[must_use]
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        cap(s, max.saturating_sub(1))
    }
}

/// Escape Discord markdown, links and mentions in text we did not write, and
/// flatten line breaks.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // A newline would split the one-line entry, and could split a page.
            '\n' | '\r' => out.push(' '),
            // `[`/`]` make masked links; `<` starts mentions and timestamps;
            // `@` starts @everyone/@here.
            '*' | '_' | '`' | '~' | '|' | '>' | '<' | '[' | ']' | '\\' | '@' => {
                out.push('\\');
                out.push(c);
            },
            _ => out.push(c),
        }
    }
    out
}

/// `raw` as an absolute http(s) URL, or `None`. Relative paths, empty
/// strings and other schemes (`javascript:`, `file:`) are all `None`.
#[must_use]
pub fn http_url(raw: Option<&str>) -> Option<Url> {
    let url = Url::parse(raw?.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// `m:ss`, or `h:mm:ss` from an hour up.
#[must_use]
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A track's length for display, or `None` when it is unknown or rounds to
/// zero -- never `0:00`.
#[must_use]
pub fn duration_text(d: Option<Duration>) -> Option<String> {
    let d = d?;
    (d.as_secs() > 0).then(|| clock(d))
}

/// Where a track is, for the now-playing progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Playing {
        position: Duration,
        duration: Option<Duration>,
    },
    /// `position` is `None` when the track would not say (see `get_info`).
    Paused { position: Option<Duration> },
}

/// The progress line. Playing with a known length: `4:33 · ends <t:…:R>`,
/// which Discord counts down by itself. Without one (a live stream):
/// `Started <t:…:R>`. Paused: `Paused at 1:12`, or just `Paused`.
#[must_use]
pub fn progress_text(p: &Progress, now_unix: i64) -> String {
    match *p {
        Progress::Playing {
            position,
            duration: Some(d),
        } if d.as_secs() > 0 => {
            let remaining = d.saturating_sub(position).as_secs() as i64;
            format!(
                "{} · {PROGRESS_ENDS} <t:{}:R>",
                clock(d),
                now_unix + remaining
            )
        },
        Progress::Playing { position, .. } => {
            format!(
                "{PROGRESS_STARTED} <t:{}:R>",
                now_unix - position.as_secs() as i64
            )
        },
        Progress::Paused {
            position: Some(position),
        } => format!("{PROGRESS_PAUSED_AT} {}", clock(position)),
        Progress::Paused { position: None } => PROGRESS_PAUSED.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(title: Option<&str>, url: Option<&str>) -> TrackLabel {
        TrackLabel {
            title: title.map(str::to_owned),
            url: url.map(str::to_owned),
            duration: None,
        }
    }

    #[test]
    fn a_blank_or_missing_title_is_untitled() {
        for t in [None, Some(""), Some("   "), Some("\n")] {
            assert_eq!(label(t, None).title_text(60), "(untitled)", "{t:?}");
        }
    }

    #[test]
    fn a_title_is_trimmed_escaped_and_cut() {
        assert_eq!(
            label(Some("  Want You Bad "), None).title_text(60),
            "Want You Bad"
        );
        assert_eq!(
            label(Some("**[x](y)** @everyone"), None).title_text(60),
            "\\*\\*\\[x\\](y)\\*\\* \\@everyone"
        );
        assert_eq!(label(Some("abcdef"), None).title_text(3), "abc…");
        // A multibyte character straddling the cut must not panic.
        assert_eq!(label(Some("ééééé"), None).title_text(2), "éé…");
    }

    #[test]
    fn only_an_http_url_makes_a_link() {
        let t = Some("Song");
        assert_eq!(
            label(t, Some("https://youtu.be/x")).linked(60),
            "[**Song**](https://youtu.be/x)"
        );
        for bad in [
            None,
            Some(""),
            Some("/watch?v=x"),
            Some("javascript:alert(1)"),
            Some("file:///etc/passwd"),
        ] {
            assert_eq!(label(t, bad).linked(60), "**Song**", "{bad:?}");
        }
        assert_eq!(
            label(t, Some("https://en.wikipedia.org/wiki/Foo_(band)")).linked(60),
            "[**Song**](https://en.wikipedia.org/wiki/Foo_%28band%29)"
        );
    }

    #[test]
    fn durations_are_m_ss_then_h_mm_ss_and_never_zero() {
        assert_eq!(duration_text(None), None);
        assert_eq!(duration_text(Some(Duration::ZERO)), None);
        assert_eq!(duration_text(Some(Duration::from_millis(900))), None);
        assert_eq!(
            duration_text(Some(Duration::from_secs(5))).as_deref(),
            Some("0:05")
        );
        assert_eq!(
            duration_text(Some(Duration::from_secs(273))).as_deref(),
            Some("4:33")
        );
        assert_eq!(
            duration_text(Some(Duration::from_secs(3599))).as_deref(),
            Some("59:59")
        );
        assert_eq!(
            duration_text(Some(Duration::from_secs(3600))).as_deref(),
            Some("1:00:00")
        );
    }

    #[test]
    fn clip_keeps_the_result_within_the_limit() {
        assert_eq!(clip("hello", 5), "hello");
        assert_eq!(clip("hello!", 5), "hell…");
        assert_eq!(clip("hello!", 5).chars().count(), 5);
    }

    #[test]
    fn the_progress_line_in_each_state() {
        let now = 1_000_000;
        let playing = Progress::Playing {
            position: Duration::from_secs(73),
            duration: Some(Duration::from_secs(273)),
        };
        assert_eq!(progress_text(&playing, now), "4:33 · ends <t:1000200:R>");
        let live = Progress::Playing {
            position: Duration::from_secs(60),
            duration: None,
        };
        assert_eq!(progress_text(&live, now), "Started <t:999940:R>");
        let paused = Progress::Paused {
            position: Some(Duration::from_secs(72)),
        };
        assert_eq!(progress_text(&paused, now), "Paused at 1:12");
        assert_eq!(
            progress_text(&Progress::Paused { position: None }, now),
            "Paused"
        );
    }

    #[test]
    fn a_label_reads_metadata_and_audit_refs() {
        let meta = AuxMetadata {
            title: Some("T".into()),
            source_url: Some("https://x.y/z".into()),
            duration: Some(Duration::from_secs(9)),
            ..Default::default()
        };
        assert_eq!(
            TrackLabel::from_metadata(&meta),
            TrackLabel {
                title: Some("T".into()),
                url: Some("https://x.y/z".into()),
                duration: Some(Duration::from_secs(9)),
            }
        );
        let r = TrackRef {
            title: Some("R".into()),
            url: None,
        };
        assert_eq!(TrackLabel::from_ref(&r), label(Some("R"), None));
    }
}
