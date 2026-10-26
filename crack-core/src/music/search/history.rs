//! This server's history as a search source: tracks it has queued, without
//! who queued them.

use super::{Hit, HitDetail, Query, SavedTrack, SourceResult, HITS_PER_SOURCE};
use crate::db::play_history::{search_play_log, HistoryTrack};
use crate::music::MusicData;
use chrono::{DateTime, Utc};
use serenity::all::GuildId;
use std::time::Duration;

/// History hits for `q` in `guild` since `since` (`None`: all of it).
pub(crate) async fn search_history(
    data: &MusicData,
    guild: GuildId,
    q: &Query,
    since: Option<DateTime<Utc>>,
) -> SourceResult {
    let Some(pool) = data.database_pool.clone() else {
        return SourceResult::NoDatabase;
    };
    match search_play_log(&pool, guild, q.as_str(), since, HITS_PER_SOURCE as i64).await {
        Ok(rows) => history_hits(rows),
        Err(e) => {
            tracing::warn!("search: history read failed in {guild}: {e}");
            SourceResult::Failed
        },
    }
}

/// Rows as hits; a row without an http(s) URL is left out.
pub(crate) fn history_hits(rows: Vec<HistoryTrack>) -> SourceResult {
    SourceResult::Hits(
        rows.into_iter()
            .filter_map(|r| {
                let url = crack_types::ytdl_url(&r.url).ok()?;
                Some(Hit {
                    track: SavedTrack {
                        url,
                        title: r.title,
                        artist: r.artist,
                        duration: r.duration_secs.map(Duration::from_secs),
                    },
                    detail: HitDetail::History {
                        plays: r.plays,
                        last: r.last,
                    },
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::db::play_history::HistoryTrack;
    use chrono::TimeZone;

    fn row(url: &str, title: Option<&str>, duration_secs: Option<u64>) -> HistoryTrack {
        HistoryTrack {
            url: url.into(),
            title: title.map(str::to_owned),
            artist: Some("A".into()),
            duration_secs,
            plays: 3,
            last: Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn rows_become_hits_and_non_web_urls_are_dropped() {
        let SourceResult::Hits(hits) = history_hits(vec![
            row("https://y/1", Some("One"), Some(61)),
            row("ftp://y/2", Some("Two"), None),
            row("https://y/3", None, None),
        ]) else {
            panic!("hits expected")
        };
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0],
            Hit {
                track: SavedTrack {
                    url: "https://y/1".into(),
                    title: Some("One".into()),
                    artist: Some("A".into()),
                    duration: Some(std::time::Duration::from_secs(61)),
                },
                detail: HitDetail::History {
                    plays: 3,
                    last: Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap(),
                },
            }
        );
        assert_eq!(hits[1].track.title, None);
    }

    #[tokio::test]
    async fn no_pool_is_no_database() {
        let data = crate::music::ops::test_support::test_music_data();
        let q = Query::new("abba").unwrap();
        assert_eq!(
            search_history(&data, serenity::all::GuildId::new(1), &q, None).await,
            SourceResult::NoDatabase
        );
    }
}
