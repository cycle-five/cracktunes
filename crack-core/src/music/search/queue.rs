//! The queue as a search source: what is queued now, matched on title and
//! artist. Hidden while a `/gp` game owns playback.

use super::{Hit, HitDetail, Query, SavedTrack, SourceResult, HITS_PER_SOURCE};
use crate::music::remote::{self, QueueState};
use crate::music::MusicData;
use serenity::all::GuildId;

/// The queue in `guild`, searched for `q`.
pub(crate) async fn search_queue(data: &MusicData, guild: GuildId, q: &Query) -> SourceResult {
    queue_hits(remote::queue_state(data, guild).await, q)
}

/// The tracks in `state` whose title or artist contains `q`, ignoring case,
/// in queue order. A track without an http(s) URL is left out: it could be
/// neither linked nor added.
pub(crate) fn queue_hits(state: QueueState, q: &Query) -> SourceResult {
    let tracks = match state {
        QueueState::Hidden => return SourceResult::Hidden,
        QueueState::Idle => return SourceResult::Hits(Vec::new()),
        QueueState::Playing { tracks, .. } => tracks,
    };
    let needle = q.as_str().to_lowercase();
    let contains = |s: &Option<String>| {
        s.as_deref()
            .is_some_and(|s| s.to_lowercase().contains(&needle))
    };
    let hits = tracks
        .into_iter()
        .enumerate()
        .filter(|(_, t)| contains(&t.title) || contains(&t.artist))
        .filter_map(|(position, t)| {
            let url = crack_types::ytdl_url(t.url.as_deref()?).ok()?;
            Some(Hit {
                track: SavedTrack {
                    url,
                    title: t.title,
                    artist: t.artist,
                    duration: t.duration,
                },
                detail: HitDetail::Queue { position },
            })
        })
        .take(HITS_PER_SOURCE)
        .collect();
    SourceResult::Hits(hits)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::remote::{QueueState, TrackSummary};
    use serenity::all::ChannelId;
    use std::time::Duration;
    use uuid::Uuid;

    fn t(n: u128, title: Option<&str>, artist: Option<&str>, url: Option<&str>) -> TrackSummary {
        TrackSummary {
            id: Uuid::from_u128(n),
            title: title.map(str::to_owned),
            artist: artist.map(str::to_owned),
            url: url.map(str::to_owned),
            duration: Some(Duration::from_secs(n as u64)),
            requester: None,
        }
    }

    fn playing(tracks: Vec<TrackSummary>) -> QueueState {
        QueueState::Playing {
            bot_channel: ChannelId::new(7),
            tracks,
            paused: false,
            looping: false,
        }
    }

    fn q(s: &str) -> Query {
        Query::new(s).unwrap()
    }

    #[test]
    fn matches_title_or_artist_case_insensitively_with_positions() {
        let state = playing(vec![
            t(1, Some("Dancing Queen"), Some("ABBA"), Some("https://y/1")),
            t(2, Some("Other"), Some("Björk"), Some("https://y/2")),
            t(3, Some("Waterloo"), Some("abba"), Some("https://y/3")),
        ]);
        let SourceResult::Hits(hits) = queue_hits(state, &q("abba")) else {
            panic!("hits expected")
        };
        let got: Vec<(String, HitDetail)> =
            hits.into_iter().map(|h| (h.track.url, h.detail)).collect();
        assert_eq!(
            got,
            vec![
                ("https://y/1".into(), HitDetail::Queue { position: 0 }),
                ("https://y/3".into(), HitDetail::Queue { position: 2 }),
            ]
        );
        // Non-ASCII case folding.
        let state = playing(vec![t(
            2,
            Some("Other"),
            Some("Björk"),
            Some("https://y/2"),
        )]);
        assert!(matches!(queue_hits(state, &q("BJÖRK")), SourceResult::Hits(h) if h.len() == 1));
    }

    #[test]
    fn a_hit_carries_the_track_as_saved_and_drops_non_web_urls() {
        let state = playing(vec![
            t(1, Some("Song"), Some("Band"), Some("https://y/1")),
            t(2, Some("Song two"), None, None),
            t(3, Some("Song three"), None, Some("file:///x.mp3")),
        ]);
        let SourceResult::Hits(hits) = queue_hits(state, &q("song")) else {
            panic!("hits expected")
        };
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].track,
            SavedTrack {
                url: "https://y/1".into(),
                title: Some("Song".into()),
                artist: Some("Band".into()),
                duration: Some(Duration::from_secs(1)),
            }
        );
    }

    #[test]
    fn caps_at_hits_per_source_and_idle_or_hidden_answer_plainly() {
        let many: Vec<TrackSummary> = (0..25)
            .map(|i| t(i, Some("same"), None, Some(&format!("https://y/{i}"))))
            .collect();
        assert!(matches!(
            queue_hits(playing(many), &q("same")),
            SourceResult::Hits(h) if h.len() == HITS_PER_SOURCE
        ));
        assert_eq!(
            queue_hits(QueueState::Idle, &q("same")),
            SourceResult::Hits(vec![])
        );
        assert_eq!(
            queue_hits(QueueState::Hidden, &q("same")),
            SourceResult::Hidden
        );
    }
}
