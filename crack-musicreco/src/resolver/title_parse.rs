use crate::resolver::SeedResolver;
use crate::{RawTrack, Result, Seed};
use async_trait::async_trait;

/// Separators seen in real titles, longest-first so " — " is tried before " - ".
///
/// 🪤 The second and third are EN DASH and EM DASH, not hyphens. A measured
/// Queen title uses the en dash; matching only '-' loses it silently.
const SEPARATORS: &[&str] = &[" — ", " – ", " - ", " | "];

/// Bracketed suffixes that are packaging, not part of a track name.
const NOISE_WORDS: &[&str] = &[
    "official",
    "video",
    "audio",
    "lyric",
    "remaster",
    "hd",
    "4k",
    "mv",
    "visualizer",
];

/// A featured-artist credit. Dropped from the seed wherever it appears: for a
/// recording lookup "Get Lucky" is the title and "Pharrell Williams" is a
/// credit, and carrying the credit into the query is what makes MusicBrainz
/// miss.
const FEAT_MARKERS: &[&str] = &["feat.", "feat ", "ft.", "ft "];

/// Strip "(Official Music Video)" / "[HD]" style suffixes and featured-artist
/// credits, whether bracketed or trailing plain text.
fn clean_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut buf = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' => {
                // 🪤 Only the OUTERMOST bracket resets the buffer. Clearing on
                // every open bracket discarded everything before a nested one,
                // so "Song (Live (2011))" came out as "Song (2011)()" -- the
                // outer text lost and an empty pair emitted.
                if depth == 0 {
                    buf.clear();
                } else {
                    buf.push(ch);
                }
                depth += 1;
            },
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth > 0 {
                    buf.push(ch);
                    continue;
                }
                let lower = buf.to_lowercase();
                let is_noise = NOISE_WORDS.iter().any(|w| lower.contains(w))
                    // 🪤 "(feat. Pharrell Williams)" is at least as common on
                    // YouTube as the trailing plain-text form, and the marker
                    // pass below cannot see it: that looks for " feat. " with a
                    // LEADING SPACE, and here the marker is preceded by '('.
                    || FEAT_MARKERS.iter().any(|m| lower.trim_start().starts_with(m));
                if !is_noise {
                    out.push('(');
                    out.push_str(&buf);
                    out.push(')');
                }
                buf.clear();
            },
            _ if depth > 0 => buf.push(ch),
            _ => out.push(ch),
        }
    }
    let lower = out.to_lowercase();
    for marker in FEAT_MARKERS {
        if let Some(i) = lower.find(&format!(" {marker}")) {
            out.truncate(i);
            break;
        }
    }
    out.trim().to_string()
}

/// Offline, free, and always tried first.
#[derive(Debug, Default, Clone, Copy)]
pub struct TitleParseResolver;

impl TitleParseResolver {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SeedResolver for TitleParseResolver {
    fn name(&self) -> &'static str {
        "title-parse"
    }

    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>> {
        // A supplied artist is a fact, not a guess.
        if let Some(artist) = raw.artist.as_ref().filter(|a| !a.trim().is_empty()) {
            return Ok(Some(Seed {
                artist: artist.trim().to_string(),
                title: clean_title(&raw.title),
                mbid: None,
                confidence: 100,
            }));
        }
        for sep in SEPARATORS {
            if let Some((artist, rest)) = raw.title.split_once(sep) {
                let title = clean_title(rest);
                if artist.trim().is_empty() || title.is_empty() {
                    continue;
                }
                return Ok(Some(Seed {
                    artist: artist.trim().to_string(),
                    title,
                    mbid: None,
                    // A parse is a guess until something checks it. MusicBrainz
                    // raises this; on its own it stays below a metered floor.
                    confidence: 50,
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(title: &str) -> RawTrack {
        RawTrack {
            title: title.into(),
            artist: None,
            uploader: None,
        }
    }

    #[tokio::test]
    async fn parses_the_titles_we_measured() {
        // Every row here came from running yt-dlp against a real video.
        let cases = [
            (
                "Guns N' Roses - Sweet Child O' Mine (Official Music Video)",
                Some(("Guns N' Roses", "Sweet Child O' Mine")),
            ),
            // 🪤 EN DASH, not a hyphen. A splitter that only knows '-' silently
            // yields no seed for this one.
            (
                "Queen – Bohemian Rhapsody (Official Video Remastered)",
                Some(("Queen", "Bohemian Rhapsody")),
            ),
            (
                "Daft Punk - Get Lucky (Official Audio) ft. Pharrell Williams",
                Some(("Daft Punk", "Get Lucky")),
            ),
            // No separator: no seed, and therefore no metered call.
            ("Never Gonna Give You Up", None),
        ];
        let r = TitleParseResolver::new();
        for (title, want) in cases {
            let got = r.resolve(&raw(title)).await.expect("never errors");
            match want {
                Some((a, t)) => {
                    let s = got.unwrap_or_else(|| panic!("no seed for {title}"));
                    assert_eq!((s.artist.as_str(), s.title.as_str()), (a, t), "{title}");
                    // 🪤 Was untested. Without this, changing the parse path's
                    // confidence from 50 to 0 -- or to 99, which WOULD clear a
                    // metered floor -- passed both tests in this file.
                    assert_eq!(
                        s.confidence, 50,
                        "a parse is a guess; only MusicBrainz raises this ({title})"
                    );
                },
                None => assert!(got.is_none(), "{title} should yield no seed"),
            }
        }
    }

    /// 🪤 Every fixture above splits on `" - "`, whose surrounding spaces
    /// `split_once` already consumes -- so deleting `.trim()` from the artist
    /// chunk produced byte-identical output on all of them and would have
    /// shipped silently. This is the one shape that needs it: padding that the
    /// separator itself does not absorb.
    #[tokio::test]
    async fn padding_around_the_separator_is_trimmed() {
        let r = TitleParseResolver::new();
        let seed = r
            .resolve(&raw("  Pink Floyd   -   Time  "))
            .await
            .unwrap()
            .expect("a padded title is still a title");
        assert_eq!(seed.artist, "Pink Floyd");
        assert_eq!(seed.title, "Time");
    }

    /// Two defects found in review, neither reachable from the brief's
    /// fixtures.
    #[tokio::test]
    async fn brackets_that_nest_and_credits_that_hide_in_them() {
        let r = TitleParseResolver::new();

        // `buf.clear()` on every open bracket lost the outer content:
        // this used to come out as "Live Aid (1985)()".
        let seed = r
            .resolve(&raw("Queen - Live Aid (Wembley (1985))"))
            .await
            .unwrap()
            .expect("nested brackets still parse");
        assert_eq!(seed.title, "Live Aid (Wembley (1985))");

        // The trailing-marker pass searches for " feat. " with a leading space,
        // so a credit opening a bracket was invisible to it.
        let seed = r
            .resolve(&raw("Daft Punk - Get Lucky (feat. Pharrell Williams)"))
            .await
            .unwrap()
            .expect("a bracketed credit still parses");
        assert_eq!(seed.title, "Get Lucky");
    }

    #[tokio::test]
    async fn an_explicit_artist_wins_over_parsing() {
        let r = TitleParseResolver::new();
        let seed = r
            .resolve(&RawTrack {
                title: "Anything At All".into(),
                artist: Some("Real Artist".into()),
                uploader: None,
            })
            .await
            .unwrap()
            .expect("explicit artist is a seed");
        assert_eq!(seed.artist, "Real Artist");
        assert_eq!(seed.title, "Anything At All");
        assert_eq!(seed.confidence, 100, "a supplied artist is not a guess");
    }
}
