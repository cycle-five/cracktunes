use serde::{Deserialize, Serialize};

/// What the bot knows about the track that just ended.
///
/// 🪤 `artist` is usually `None`, and when set it is often not the artist.
/// Measured on production (2026-09-13): `/play <url>` resolves with no artist
/// at all, and a keyword search reports the uploading channel as the artist --
/// "MrCalienteLP" for a fan upload of The Offspring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrack {
    pub title: String,
    pub artist: Option<String>,
    pub uploader: Option<String>,
    /// The YouTube video id, when the track is a YouTube video. YouTube's Mix
    /// needs nothing else: no seed and no parsing.
    pub video_id: Option<String>,
}

/// A seed worth spending a metered call on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seed {
    pub artist: String,
    pub title: String,
    pub mbid: Option<String>,
    /// 0-100. MusicBrainz's search score, or 100 when the caller supplied the
    /// artist directly. Below the configured floor, metered providers are
    /// skipped rather than guessed at.
    pub confidence: u8,
}

/// 🔑 Providers differ in how playable their results are and the type says so.
/// YouTube's Mix and musicatlas return a video id; Deezer never does.
/// Flattening these to a single "url" field would make a Deezer result look
/// directly playable.
///
/// 🔑 Adjacently tagged (`tag` + `content`), not serde's default external
/// tagging. The house rule is an explicit discriminant on the wire; internal
/// tagging (`tag` alone) cannot express these, because serde's internally
/// tagged representation requires newtype variants to wrap a map and both of
/// these wrap a `String`. Adjacent tagging keeps the named discriminant and
/// works with the shapes we actually have.
///
/// Settled here rather than later because Tasks 3-5 add provider-facing enums
/// that will copy whatever this one does, and because the cache format becomes
/// expensive to change once anything is persisted in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Playable {
    #[serde(rename = "youtube_id")]
    YouTubeId(String),
    SearchQuery(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    pub artist: String,
    pub title: String,
    pub playable: Playable,
    pub isrc: Option<String>,
    /// Which provider produced this.
    ///
    /// 🪤 `String`, not `&'static str`, and the difference is a hard compile
    /// error rather than a style preference. A `&'static str` field makes the
    /// derived `Deserialize` implementation valid **only for `'de = 'static`**,
    /// so it is not `DeserializeOwned` -- and every real input is a buffer that
    /// does not outlive the call:
    ///
    /// ```text
    /// error[E0597]: `json` does not live long enough
    ///   argument requires that `json` is borrowed for `'static`
    /// ```
    ///
    /// Task 7's cache read (`serde_json::from_value::<Vec<Recommendation>>`)
    /// would not have compiled at all. Serialize is unaffected -- string
    /// literals *are* `'static` -- which is exactly why constructing and
    /// serializing in tests hid this completely.
    ///
    /// This is the project's own "&str except when the value must outlive its
    /// source" rule landing on the `String` side.
    pub source: String,
}

impl Recommendation {
    /// The video id, when this provider supplied one.
    #[must_use]
    pub fn youtube_id(&self) -> Option<&str> {
        match &self.playable {
            Playable::YouTubeId(id) => Some(id),
            Playable::SearchQuery(_) => None,
        }
    }

    /// A search string usable by the bot's existing search path.
    #[must_use]
    pub fn search_text(&self) -> String {
        match &self.playable {
            Playable::SearchQuery(q) => q.clone(),
            Playable::YouTubeId(_) => format!("{} - {}", self.artist, self.title),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_query_reads_as_artist_then_title() {
        let r = Recommendation {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            // 🪤 Deliberately NOT "Queen - Bohemian Rhapsody". The original
            // fixture used exactly `format!("{artist} - {title}")`, so swapping
            // the two arms of `search_text` produced identical output and the
            // test could not tell them apart. A provider's own query string is
            // not usually the artist and title concatenated, so this is also
            // the more realistic value.
            playable: Playable::SearchQuery("bohemian rhapsody official video".into()),
            isrc: None,
            source: "test".into(),
        };
        assert_eq!(r.search_text(), "bohemian rhapsody official video");
    }

    #[test]
    fn a_youtube_id_falls_back_to_artist_and_title() {
        let r = Recommendation {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            playable: Playable::YouTubeId("fJ9rUzIMcZQ".into()),
            isrc: None,
            source: "test".into(),
        };
        assert_eq!(r.search_text(), "Queen - Bohemian Rhapsody");
    }

    /// 🪤 The one assertion that would have caught the `&'static str` bug.
    /// Constructing and serializing a `Recommendation` works fine with a
    /// `'static` field; only a round trip through an owned buffer fails, and
    /// it fails at COMPILE time, so this test existing at all is the guard.
    #[test]
    fn a_recommendation_round_trips_through_an_owned_buffer() {
        let r = Recommendation {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            playable: Playable::YouTubeId("fJ9rUzIMcZQ".into()),
            isrc: Some("GBUM71029604".into()),
            source: "musicatlas".into(),
        };
        let encoded: String = serde_json::to_string(&r).unwrap();
        let decoded: Recommendation = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, r);
        // The discriminant is named on the wire, not positional.
        assert!(
            encoded.contains(r#""type":"youtube_id""#),
            "expected an explicit tag, got {encoded}"
        );
    }

    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn a_youtube_id_is_not_a_search() {
        let r = Recommendation {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            playable: Playable::YouTubeId("fJ9rUzIMcZQ".into()),
            isrc: None,
            source: "test".into(),
        };
        assert_eq!(r.youtube_id(), Some("fJ9rUzIMcZQ"));
        // 🔑 The whole reason Playable is an enum: a caller must not be able to
        // treat a Deezer result as if it had a video id.
        assert_eq!(r.youtube_id().is_none(), false);
    }
}
