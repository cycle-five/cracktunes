use serde::{Deserialize, Serialize};

/// What the bot knows about the track that just ended.
///
/// 🪤 `artist` is almost always `None`: measured, yt-dlp returns `artist: NA`
/// and `track: NA` for ordinary music videos, leaving only the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrack {
    pub title: String,
    pub artist: Option<String>,
    pub uploader: Option<String>,
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
/// musicatlas returns a video id; ReccoBeats never does. Flattening these to a
/// single "url" field would make a ReccoBeats result look directly playable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Playable {
    YouTubeId(String),
    SearchQuery(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    pub artist: String,
    pub title: String,
    pub playable: Playable,
    pub isrc: Option<String>,
    pub source: &'static str,
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
            playable: Playable::SearchQuery("Queen - Bohemian Rhapsody".into()),
            isrc: None,
            source: "test",
        };
        assert_eq!(r.search_text(), "Queen - Bohemian Rhapsody");
    }

    #[test]
    #[allow(clippy::bool_assert_comparison)]
    fn a_youtube_id_is_not_a_search() {
        let r = Recommendation {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            playable: Playable::YouTubeId("fJ9rUzIMcZQ".into()),
            isrc: None,
            source: "test",
        };
        assert_eq!(r.youtube_id(), Some("fJ9rUzIMcZQ"));
        // 🔑 The whole reason Playable is an enum: a caller must not be able to
        // treat a ReccoBeats result as if it had a video id.
        assert_eq!(r.youtube_id().is_none(), false);
    }
}
