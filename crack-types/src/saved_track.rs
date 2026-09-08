//! A track as data.
//!
//! A resolved track carries live handles -- a `rusty_ytdl::Video`, a songbird
//! `YoutubeDl` -- that cannot be written down. Playing one back never needs
//! them: `build_track` reads the URL and the display metadata and nothing else.
//! This is that much of a track, in a shape a database row or a JSON blob can
//! hold, and enough to rebuild a playable track from.
//!
//! It is what a `/gp` game saves for each song. Queue persistence, if it comes,
//! saves the same thing.

use crate::AuxMetadata;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedTrack {
    /// The source URL yt-dlp is handed to play it.
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<Duration>,
}

impl SavedTrack {
    /// The metadata a rebuilt track carries: what the queue and the reveal show,
    /// plus the URL `build_track` plays from.
    pub fn to_metadata(&self) -> AuxMetadata {
        AuxMetadata {
            title: self.title.clone(),
            artist: self.artist.clone(),
            duration: self.duration,
            source_url: Some(self.url.clone()),
            ..AuxMetadata::default()
        }
    }

    /// Whole seconds, for a `BIGINT` column. Sub-second precision is not worth a
    /// float: nothing that reads it back needs better than a second.
    pub fn duration_secs(&self) -> Option<i64> {
        self.duration.map(|d| d.as_secs() as i64)
    }

    pub fn from_secs(
        url: String,
        title: Option<String>,
        artist: Option<String>,
        duration_secs: Option<i64>,
    ) -> Self {
        Self {
            url,
            title,
            artist,
            duration: duration_secs
                .and_then(|s| u64::try_from(s).ok())
                .map(Duration::from_secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips_through_seconds() {
        let saved = SavedTrack {
            url: "https://www.youtube.com/watch?v=abc".into(),
            title: Some("A Song".into()),
            artist: Some("Someone".into()),
            duration: Some(Duration::from_secs(214)),
        };
        let back = SavedTrack::from_secs(
            saved.url.clone(),
            saved.title.clone(),
            saved.artist.clone(),
            saved.duration_secs(),
        );
        assert_eq!(back, saved);
        let m = saved.to_metadata();
        assert_eq!(
            m.source_url.as_deref(),
            Some("https://www.youtube.com/watch?v=abc")
        );
        assert_eq!(m.duration, Some(Duration::from_secs(214)));
    }

    #[test]
    fn a_negative_duration_is_unknown_not_a_panic() {
        assert_eq!(
            SavedTrack::from_secs("u".into(), None, None, Some(-1)).duration,
            None
        );
    }
}
