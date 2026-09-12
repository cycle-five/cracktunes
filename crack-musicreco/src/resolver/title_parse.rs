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

/// Strip "(Official Music Video)" / "[HD]" style suffixes, then a trailing
/// "ft. …" / "feat. …" clause.
fn clean_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut buf = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                buf.clear();
            },
            ')' | ']' if depth > 0 => {
                depth -= 1;
                let lower = buf.to_lowercase();
                if !NOISE_WORDS.iter().any(|w| lower.contains(w)) {
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
    for marker in [" ft. ", " feat. ", " ft ", " feat "] {
        if let Some(i) = lower.find(marker) {
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
                },
                None => assert!(got.is_none(), "{title} should yield no seed"),
            }
        }
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
