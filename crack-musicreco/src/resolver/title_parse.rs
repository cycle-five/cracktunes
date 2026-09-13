use crate::resolver::SeedResolver;
use crate::text::normalize;
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

/// The title without a leading "<artist><separator>", when that artist is the
/// one supplied. Official uploads repeat the artist in the title ("The
/// Offspring - Hit That"), and a seed that keeps it asks every provider for a
/// track by that whole name. Any other text before a separator is kept: it is
/// not this artist's name, and cutting it would be a guess.
fn strip_artist_prefix<'a>(title: &'a str, artist: &str) -> &'a str {
    let artist = normalize(artist);
    SEPARATORS
        .iter()
        .filter_map(|sep| title.split_once(sep))
        .find(|(head, _)| normalize(head) == artist)
        .map_or(title, |(_, rest)| rest)
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
            let title = clean_title(strip_artist_prefix(&raw.title, artist));
            // 🪤 L6 (Task 5 review): a supplied artist with a title that
            // cleans to empty (e.g. just "(Official Video)") used to produce
            // a seed with an empty title -- a guaranteed non-match for every
            // downstream consumer (MusicBrainz, musicatlas, ReccoBeats).
            // Fixed at the source so every caller of this resolver gets it,
            // not just the ones that remember to check.
            if title.is_empty() {
                return Ok(None);
            }
            return Ok(Some(Seed {
                artist: artist.trim().to_string(),
                title,
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

    /// 🪤 Measured in production on v0.11.0: yt-dlp supplied `artist: "The
    /// Offspring"` for "The Offspring - Hit That (Official Music Video)", and
    /// the seed kept the artist in its title -- so every provider was asked
    /// for a track called "The Offspring - Hit That". Official uploads put the
    /// artist in both places, so this is the common shape, not an edge.
    #[tokio::test]
    async fn a_supplied_artist_repeated_at_the_front_of_the_title_is_removed() {
        let r = TitleParseResolver::new();
        let cases = [
            (
                "The Offspring",
                "The Offspring - Hit That (Official Music Video)",
                "Hit That",
            ),
            // EN DASH, and the tag's case differs from the title's.
            (
                "QUEEN",
                "Queen – Bohemian Rhapsody (Official Video Remastered)",
                "Bohemian Rhapsody",
            ),
        ];
        for (artist, title, want) in cases {
            let seed = r
                .resolve(&RawTrack {
                    title: title.into(),
                    artist: Some(artist.into()),
                    uploader: None,
                })
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("no seed for {title}"));
            assert_eq!(seed.title, want, "{title}");
            assert_eq!(seed.artist, artist, "the supplied artist is kept as given");
            assert_eq!(seed.confidence, 100, "{title}");
        }
    }

    /// Only the SUPPLIED artist is removed. A title that opens with someone
    /// else is not this artist's name repeated, and cutting it would turn
    /// "Daft Punk - Get Lucky" credited to Pharrell into a seed for "Get Lucky"
    /// by Pharrell -- a guess dressed up as a fact at confidence 100.
    #[tokio::test]
    async fn a_different_artist_at_the_front_of_the_title_is_kept() {
        let r = TitleParseResolver::new();
        let seed = r
            .resolve(&RawTrack {
                title: "Daft Punk - Get Lucky".into(),
                artist: Some("Pharrell Williams".into()),
                uploader: None,
            })
            .await
            .unwrap()
            .expect("explicit artist is a seed");
        assert_eq!(seed.title, "Daft Punk - Get Lucky");
    }

    /// L6 (Task 5 review): before this, `artist: Some("Queen")` with a title
    /// of just `"(Official Video)"` produced `Seed { title: "", .. }` -- a
    /// query with nothing to search for.
    #[tokio::test]
    async fn a_supplied_artist_with_a_title_that_cleans_to_empty_yields_no_seed() {
        let r = TitleParseResolver::new();
        let got = r
            .resolve(&RawTrack {
                title: "(Official Video)".into(),
                artist: Some("Queen".into()),
                uploader: None,
            })
            .await
            .unwrap();
        assert!(
            got.is_none(),
            "an empty-after-cleaning title has nothing to search for"
        );
    }
}
