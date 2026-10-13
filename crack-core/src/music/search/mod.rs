//! One track search over every source the bot implements: the queue, this
//! server's history, YouTube and SoundCloud. Every hit is a [`SavedTrack`],
//! the type queue resume plays from, so any hit can be added.
//! Spec: docs/superpowers/specs/2026-10-13-dashboard-search-and-add-design.md

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use songbird::input::AuxMetadata;
use std::{future::Future, time::Duration};

pub use crack_types::SavedTrack;

/// The most hits one source returns.
pub const HITS_PER_SOURCE: usize = 10;
/// The bound on a source that reads only memory or the database.
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(2);
/// The bound on a source that asks YouTube or SoundCloud, including the wait
/// for a permit.
pub const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(8);
/// External searches running at once, bot-wide: web search must not hammer
/// YouTube from the bot's one residential IP.
pub const EXTERNAL_SEARCHES: usize = 4;

/// Where a hit came from. The declaration order is the display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Queue,
    History,
    YouTube,
    SoundCloud,
}

impl Source {
    const ALL: [Source; 4] = [
        Source::Queue,
        Source::History,
        Source::YouTube,
        Source::SoundCloud,
    ];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Which sources to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceSet(u8);

impl SourceSet {
    #[must_use]
    pub fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub fn all() -> Self {
        Source::ALL.into_iter().fold(Self::empty(), Self::with)
    }

    #[must_use]
    pub fn with(self, s: Source) -> Self {
        Self(self.0 | s.bit())
    }

    #[must_use]
    pub fn contains(&self, s: Source) -> bool {
        self.0 & s.bit() != 0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

/// Why a query was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryRefused {
    TooShort,
    TooLong,
    ControlCharacters,
}

/// Search text, validated once: trimmed, 2 to 100 characters (not bytes),
/// no control characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query(String);

impl Query {
    pub const MIN_CHARS: usize = 2;
    pub const MAX_CHARS: usize = 100;

    pub fn new(raw: &str) -> Result<Query, QueryRefused> {
        let text = raw.trim();
        let chars = text.chars().count();
        if chars < Self::MIN_CHARS {
            return Err(QueryRefused::TooShort);
        }
        if chars > Self::MAX_CHARS {
            return Err(QueryRefused::TooLong);
        }
        if text.chars().any(char::is_control) {
            return Err(QueryRefused::ControlCharacters);
        }
        Ok(Query(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One track a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub track: SavedTrack,
    pub detail: HitDetail,
}

/// What a source knows about its hit beyond the track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitDetail {
    /// Its place in the queue; 0 is the playing track.
    Queue { position: usize },
    /// How often this server queued it, and when last.
    History { plays: u32, last: DateTime<Utc> },
    /// YouTube or SoundCloud.
    Found,
}

/// One source's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceResult {
    Hits(Vec<Hit>),
    /// A `/gp` game owns playback: the queue would give the answers away.
    Hidden,
    /// The bot runs without a database: there is no history.
    NoDatabase,
    Failed,
    TimedOut,
}

/// A hit's track from search metadata, or `None` without an http(s) URL: a
/// hit must be both a safe link and something `build_track` will play.
#[cfg_attr(not(test), expect(dead_code, reason = "wired into search() in Task 4"))]
pub(crate) fn saved_from_metadata(meta: &AuxMetadata) -> Option<SavedTrack> {
    let url = crack_types::ytdl_url(meta.source_url.as_deref()?).ok()?;
    Some(SavedTrack {
        url,
        title: meta.title.clone(),
        artist: meta.artist.clone(),
        duration: meta.duration,
    })
}

/// A source's search, its bound, and which source it is.
#[cfg_attr(not(test), expect(dead_code, reason = "wired into search() in Task 4"))]
pub(crate) type Job<'a> = (Source, Duration, BoxFuture<'a, SourceResult>);

/// Run every job at once, each under its own bound, and answer in the fixed
/// source order. A source that hangs is `TimedOut`; nothing one source does
/// changes another's answer.
#[cfg_attr(not(test), expect(dead_code, reason = "wired into search() in Task 4"))]
pub(crate) async fn gather(jobs: Vec<Job<'_>>) -> Vec<(Source, SourceResult)> {
    let runs = jobs.into_iter().map(|(source, bound, job)| async move {
        match tokio::time::timeout(bound, job).await {
            Ok(result) => (source, result),
            Err(_) => {
                tracing::warn!("search: {source:?} timed out after {bound:?}");
                (source, SourceResult::TimedOut)
            },
        }
    });
    let mut out = futures::future::join_all(runs).await;
    out.sort_by_key(|(source, _)| *source);
    out
}

/// The bot-wide gate on external searches.
#[expect(dead_code, reason = "wired into search() in Task 4")]
pub(crate) static EXTERNAL: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(EXTERNAL_SEARCHES);

/// Run `fut` holding one of `gate`'s permits.
#[cfg_attr(not(test), expect(dead_code, reason = "wired into search() in Task 4"))]
pub(crate) async fn gated<T>(gate: &tokio::sync::Semaphore, fut: impl Future<Output = T>) -> T {
    let _permit = gate
        .acquire()
        .await
        .expect("the search gate is never closed");
    fut.await
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use std::sync::Arc;

    #[test]
    fn a_query_is_trimmed_and_bounded_in_characters() {
        assert_eq!(Query::new("  abba \n").unwrap().as_str(), "abba");
        assert_eq!(Query::new("a"), Err(QueryRefused::TooShort));
        assert_eq!(Query::new("   "), Err(QueryRefused::TooShort));
        assert!(Query::new("ab").is_ok());
        // Characters, not bytes: 100 CJK characters are 300 bytes.
        let cjk100: String = "音".repeat(100);
        assert!(Query::new(&cjk100).is_ok());
        let cjk101: String = "音".repeat(101);
        assert_eq!(Query::new(&cjk101), Err(QueryRefused::TooLong));
        assert_eq!(Query::new(&"a".repeat(101)), Err(QueryRefused::TooLong));
    }

    #[test]
    fn a_query_refuses_control_characters_inside() {
        assert_eq!(
            Query::new("ab\u{0}cd"),
            Err(QueryRefused::ControlCharacters)
        );
        assert_eq!(Query::new("ab\ncd"), Err(QueryRefused::ControlCharacters));
        assert_eq!(
            Query::new("ab\u{1b}[31m"),
            Err(QueryRefused::ControlCharacters)
        );
    }

    #[test]
    fn only_http_metadata_becomes_a_saved_track() {
        let meta = |url: Option<&str>| AuxMetadata {
            title: Some("Song".into()),
            artist: Some("Band".into()),
            duration: Some(Duration::from_secs(61)),
            source_url: url.map(str::to_owned),
            ..Default::default()
        };
        assert_eq!(
            saved_from_metadata(&meta(Some("https://www.youtube.com/watch?v=x"))),
            Some(SavedTrack {
                url: "https://www.youtube.com/watch?v=x".into(),
                title: Some("Song".into()),
                artist: Some("Band".into()),
                duration: Some(Duration::from_secs(61)),
            })
        );
        assert_eq!(saved_from_metadata(&meta(None)), None);
        assert_eq!(
            saved_from_metadata(&meta(Some("javascript:alert(1)"))),
            None
        );
        assert_eq!(saved_from_metadata(&meta(Some("-rf"))), None);
        assert_eq!(saved_from_metadata(&meta(Some("file:///etc/passwd"))), None);
    }

    #[test]
    fn a_source_set_holds_what_was_added() {
        let s = SourceSet::empty()
            .with(Source::History)
            .with(Source::SoundCloud);
        assert!(s.contains(Source::History) && s.contains(Source::SoundCloud));
        assert!(!s.contains(Source::Queue) && !s.contains(Source::YouTube));
        assert!(SourceSet::empty().is_empty());
        assert!([
            Source::Queue,
            Source::History,
            Source::YouTube,
            Source::SoundCloud
        ]
        .into_iter()
        .all(|x| SourceSet::all().contains(x)));
    }

    fn hits(n: usize) -> SourceResult {
        SourceResult::Hits(
            (0..n)
                .map(|i| Hit {
                    track: SavedTrack {
                        url: format!("https://x/{i}"),
                        ..Default::default()
                    },
                    detail: HitDetail::Found,
                })
                .collect(),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn gather_keeps_what_answered_when_another_source_hangs_or_fails() {
        let jobs: Vec<Job<'_>> = vec![
            (
                Source::SoundCloud,
                EXTERNAL_TIMEOUT,
                Box::pin(std::future::pending()),
            ),
            (
                Source::YouTube,
                EXTERNAL_TIMEOUT,
                Box::pin(async { SourceResult::Failed }),
            ),
            (Source::Queue, LOCAL_TIMEOUT, Box::pin(async { hits(2) })),
        ];
        let out = gather(jobs).await;
        assert_eq!(
            out,
            vec![
                (Source::Queue, hits(2)),
                (Source::YouTube, SourceResult::Failed),
                (Source::SoundCloud, SourceResult::TimedOut),
            ],
            "fixed order, each source its own status"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn gather_applies_each_sources_own_timeout() {
        let slow_local: Job<'_> = (
            Source::History,
            LOCAL_TIMEOUT,
            Box::pin(async {
                tokio::time::sleep(LOCAL_TIMEOUT + Duration::from_millis(1)).await;
                hits(1)
            }),
        );
        let slow_external: Job<'_> = (
            Source::YouTube,
            EXTERNAL_TIMEOUT,
            Box::pin(async {
                tokio::time::sleep(LOCAL_TIMEOUT + Duration::from_millis(1)).await;
                hits(1)
            }),
        );
        let out = gather(vec![slow_local, slow_external]).await;
        assert_eq!(
            out,
            vec![
                (Source::History, SourceResult::TimedOut),
                (Source::YouTube, hits(1))
            ]
        );
    }

    #[tokio::test]
    async fn the_gate_bounds_how_many_run_at_once() {
        let gate = Arc::new(tokio::sync::Semaphore::new(2));
        let running = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let (gate, running, most) = (gate.clone(), running.clone(), most.clone());
            tasks.push(tokio::spawn(async move {
                gated(&gate, async {
                    let now = running.fetch_add(1, SeqCst) + 1;
                    most.fetch_max(now, SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    running.fetch_sub(1, SeqCst);
                })
                .await;
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(most.load(SeqCst), 2);
    }
}
