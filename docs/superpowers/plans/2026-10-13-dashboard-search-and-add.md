# Dashboard Search and Add Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A dashboard member can search the queue, this server's history, YouTube and SoundCloud from one box, and add any hit to the end of the queue or to play next.

**Architecture:** A crack-core search seam (`music::search`) runs the selected sources concurrently, each bounded and isolated, and returns every hit as a `SavedTrack`. A new `ops::add` (plus `remote::add` for context-free callers) enqueues one `SavedTrack` under the playback lease. crack-web keeps each user's last search in a TTL'd memo and answers `GET /g/{g}/search` with opaque hit ids; `POST /g/{g}/add` takes an id, so the browser never names a URL.

**Tech Stack:** Rust 2021 workspace (crack-core, crack-types, crack-testing, crack-web), tokio, songbird (yt-dlp `YoutubeDl`), rusty_ytdl, sqlx 0.9 (postgres, offline `.sqlx`), axum 0.8, vanilla JS under a Trusted-Types CSP.

**Spec:** `docs/superpowers/specs/2026-10-13-dashboard-search-and-add-design.md` — read it, including its **Plan-time amendments** section at the end, which overrides the sections it names.

## Global Constraints

- Work on branch `feat/dashboard-search-add` in `/home/lothrop/projects/cracktunes` (the main checkout). Never create a worktree inside the repo.
- Never `git add -A` or `git add .`; add the files you changed by name.
- Every commit message ends with exactly this trailer and no other `Co-Authored-By` line: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- Typed serde only: never `serde_json::json!` or `serde_json::Value` for data we own. Wire enums use `#[serde(tag = ..., rename_all = "snake_case")]`; one enum per direction.
- User-facing Discord/dashboard strings are `pub const`s in `crack-core/src/messaging/messages.rs` (the #537 convention). Log text stays inline.
- `#[expect(..., reason = "...")]`, never `#[allow]`, for new lint exceptions.
- Format with `cargo +nightly fmt --all` (bare `rustfmt --edition 2024` reorders imports differently and CI fails).
- Lint gate: `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return` must be clean.
- Tests: TDD. Watch each new test fail before implementing. Then **sabotage** each test's central claim once (break the implementation, see the test go red, restore) and record it in `/tmp/claude-1000/-home-lothrop-projects-cracktunes/f42825f7-4dab-4769-98c5-e0f49f01de4f/scratchpad/mutations.md` as `| task | mutation | caught by |`. A mutation nothing catches is reported, not hidden.
- Tests that touch the live network (YouTube, SoundCloud) are `#[ignore = "hits live <site> and needs yt-dlp on PATH"]`.
- db-tests need a local postgres with these exact credentials:
  `docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`
  (bare `docker` here targets a remote host; `--context default` is required). Run them with
  `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres cargo test -p crack-core --features db-tests <filter>`.
- A new `sqlx::query!` needs its `.sqlx` file. With the postgres above running and migrated (`DATABASE_URL=… sqlx migrate run --source migrations` or let a `#[sqlx::test]` run migrate it), regenerate with:
  ```bash
  out=$(mktemp -d)
  DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres \
    SQLX_OFFLINE=false SQLX_OFFLINE_DIR="$out" \
    cargo build -p crack-core --all-targets --features db-tests
  python3 scripts/sqlx_sync.py "$out"
  ```
  and commit the new `.sqlx/query-*.json` by name.
- Constants (from the spec): `HITS_PER_SOURCE = 10`; SoundCloud and the yt-dlp YouTube fallback ask for `5`; `LOCAL_TIMEOUT = 2 s`; `EXTERNAL_TIMEOUT = 8 s`; `EXTERNAL_SEARCHES = 4` permits; `Query` 2..=100 **characters**; `SEARCHES_PER_WINDOW = 5` per `SEARCH_WINDOW = 30 s`; `HIT_TTL = 10 min`; version `0.27.0`.
- No migration in this arc.

## Review Focus

1. **Non-ASCII queries.** A query of CJK or emoji must be bounded by characters, not bytes (a byte-slice panic on CJK bit `/search` before), and match case-insensitively (`björk` finds `Björk`). Pinned in Task 1 (`Query` length in chars, no panic on 101 CJK chars) and Task 2 (queue match on non-ASCII case).
2. **SQL LIKE metacharacters in the query.** `100%`, `a_b` and `AC\DC` must match literally, not as wildcards. Pinned in Task 3 (`contains_pattern` unit test and a db-test).
3. **Metadata with holes.** A history row with a NULL title or `duration = 0`, a queue track with no metadata, a YouTube hit with no URL: hits show "Unknown title"/no duration, and a hit without an http(s) URL never appears. Pinned in Task 1 (`saved_from_metadata`), Task 3 (NULL title, 0 duration) and Task 7 (`HitView` title fallback).
4. **The bot leaves voice, or a game starts, between search and add.** The add answers `not_playing` / `game_in_progress` with the right note, and nothing is enqueued. Pinned in Task 5 (lease refusals) and Task 7 (route maps each refusal).
5. **Double-clicking Add.** One click is one request: the row's buttons are disabled while its request is in flight. Pinned in Task 8 (find.js disables both buttons of a row until its answer lands; checked by hand in the manual list).

---

### Task 1: Search vocabulary and orchestration

**Files:**
- Create: `crack-core/src/music/search/mod.rs`
- Modify: `crack-core/src/music/mod.rs` (add `pub mod search;`)

**Interfaces:**
- Consumes: `crack_types::{SavedTrack, ytdl_url}`, `songbird::input::AuxMetadata`.
- Produces (later tasks rely on these exact names):
  ```rust
  pub use crack_types::SavedTrack;
  pub const HITS_PER_SOURCE: usize = 10;
  pub const LOCAL_TIMEOUT: Duration;      // 2 s
  pub const EXTERNAL_TIMEOUT: Duration;   // 8 s
  pub const EXTERNAL_SEARCHES: usize = 4;
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
  pub enum Source { Queue, History, YouTube, SoundCloud }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct SourceSet { .. }   // fn empty(), fn all(), fn with(self, Source) -> Self, fn contains(&self, Source) -> bool, fn is_empty(&self) -> bool
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Query(String);     // fn new(&str) -> Result<Query, QueryRefused>, fn as_str(&self) -> &str
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum QueryRefused { TooShort, TooLong, ControlCharacters }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Hit { pub track: SavedTrack, pub detail: HitDetail }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum HitDetail { Queue { position: usize }, History { plays: u32, last: DateTime<Utc> }, Found }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum SourceResult { Hits(Vec<Hit>), Hidden, NoDatabase, Failed, TimedOut }
  pub(crate) fn saved_from_metadata(meta: &AuxMetadata) -> Option<SavedTrack>;
  pub(crate) type Job<'a> = (Source, Duration, BoxFuture<'a, SourceResult>);
  pub(crate) async fn gather(jobs: Vec<Job<'_>>) -> Vec<(Source, SourceResult)>;
  pub(crate) async fn gated<T>(gate: &tokio::sync::Semaphore, fut: impl Future<Output = T>) -> T;
  pub(crate) static EXTERNAL: tokio::sync::Semaphore;  // EXTERNAL_SEARCHES permits
  ```

- [ ] **Step 1: Write the failing tests**

Create `crack-core/src/music/search/mod.rs` with only the test module first (plus `use super::*` targets that don't exist yet, so it fails to compile):

```rust
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
        assert_eq!(Query::new("ab\u{0}cd"), Err(QueryRefused::ControlCharacters));
        assert_eq!(Query::new("ab\ncd"), Err(QueryRefused::ControlCharacters));
        assert_eq!(Query::new("ab\u{1b}[31m"), Err(QueryRefused::ControlCharacters));
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
        assert_eq!(saved_from_metadata(&meta(Some("javascript:alert(1)"))), None);
        assert_eq!(saved_from_metadata(&meta(Some("-rf"))), None);
        assert_eq!(saved_from_metadata(&meta(Some("file:///etc/passwd"))), None);
    }

    #[test]
    fn a_source_set_holds_what_was_added() {
        let s = SourceSet::empty().with(Source::History).with(Source::SoundCloud);
        assert!(s.contains(Source::History) && s.contains(Source::SoundCloud));
        assert!(!s.contains(Source::Queue) && !s.contains(Source::YouTube));
        assert!(SourceSet::empty().is_empty());
        assert!([Source::Queue, Source::History, Source::YouTube, Source::SoundCloud]
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
            (Source::SoundCloud, EXTERNAL_TIMEOUT, Box::pin(std::future::pending())),
            (Source::YouTube, EXTERNAL_TIMEOUT, Box::pin(async { SourceResult::Failed })),
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
            vec![(Source::History, SourceResult::TimedOut), (Source::YouTube, hits(1))]
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
```

Add `pub mod search;` to `crack-core/src/music/mod.rs` (after `pub mod resume;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p crack-core --lib music::search`
Expected: compile errors (`Query`, `gather`, … not found).

- [ ] **Step 3: Write the implementation** (above the test module in `search/mod.rs`)

```rust
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
    const ALL: [Source; 4] = [Source::Queue, Source::History, Source::YouTube, Source::SoundCloud];

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
pub(crate) type Job<'a> = (Source, Duration, BoxFuture<'a, SourceResult>);

/// Run every job at once, each under its own bound, and answer in the fixed
/// source order. A source that hangs is `TimedOut`; nothing one source does
/// changes another's answer.
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
pub(crate) static EXTERNAL: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(EXTERNAL_SEARCHES);

/// Run `fut` holding one of `gate`'s permits.
pub(crate) async fn gated<T>(gate: &tokio::sync::Semaphore, fut: impl Future<Output = T>) -> T {
    let _permit = gate.acquire().await.expect("the search gate is never closed");
    fut.await
}
```

If `ytdl_url` returns a `Result<String, _>` whose `Ok` re-serializes `https://www.youtube.com/watch?v=x` unchanged, the first assertion holds; it does (`url::Url` keeps that form).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p crack-core --lib music::search`
Expected: 7 passed. Then `cargo clippy -p crack-core --all-targets -- -D warnings` — `pub(crate)` items unused until Task 4 may warn `dead_code`; if so, add to each such item `#[cfg_attr(not(test), expect(dead_code, reason = "used by search() in Task 4"))]` and remove those attributes in Task 4.

- [ ] **Step 5: Sabotage, record, commit**

Sabotages to try (restore each): count bytes instead of chars in `Query::new`; drop the `is_control` check; return `Some` for a non-http URL; remove the `sort_by_key`; make `gated` not acquire. Each must turn a test red. Record in `mutations.md`.

```bash
cargo +nightly fmt --all
git add crack-core/src/music/search/mod.rs crack-core/src/music/mod.rs
git commit -m "feat(search): the search vocabulary, query validation and the gather/gate orchestration

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: The queue source

**Files:**
- Modify: `crack-core/src/music/remote.rs` (`TrackSummary` gains `artist`; `summary_of` fills it)
- Modify: every `TrackSummary { .. }` literal: `crack-core/src/music/ops/skip.rs:~242`, `crack-core/src/music/ops/edit.rs:~311,322,335`, `crack-web/src/view.rs:~206` (add `artist: None`)
- Create: `crack-core/src/music/search/queue.rs`
- Modify: `crack-core/src/music/search/mod.rs` (`mod queue;`)

**Interfaces:**
- Consumes: Task 1's `Query`, `Hit`, `HitDetail`, `SourceResult`, `HITS_PER_SOURCE`, `saved_from_metadata` is NOT used here; `remote::{QueueState, TrackSummary, queue_state}`.
- Produces: `pub(crate) fn queue_hits(state: QueueState, q: &Query) -> SourceResult` and `pub(crate) async fn search_queue(data: &Data, guild: GuildId, q: &Query) -> SourceResult`.

- [ ] **Step 1: Add the field (a compile-driven step)**

In `remote.rs`:
```rust
pub struct TrackSummary {
    pub id: Uuid,
    pub title: Option<String>,
    /// The artist or uploader, as the track's metadata names it.
    pub artist: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
    pub requester: Option<Requester>,
}
```
and in `summary_of`: `artist: meta.artist,` (before `url: meta.source_url`). Add `artist: None,` to each other literal listed above. Run `cargo check --workspace --all-targets`; expected: clean.

- [ ] **Step 2: Write the failing tests** in `crack-core/src/music/search/queue.rs`

```rust
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
        let state = playing(vec![t(2, Some("Other"), Some("Björk"), Some("https://y/2"))]);
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
        assert_eq!(queue_hits(QueueState::Idle, &q("same")), SourceResult::Hits(vec![]));
        assert_eq!(queue_hits(QueueState::Hidden, &q("same")), SourceResult::Hidden);
    }
}
```

Add `mod queue;` to `search/mod.rs` (and `pub(crate) use queue::search_queue;` is not needed — Task 4 calls `queue::search_queue`).

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p crack-core --lib music::search::queue`
Expected: compile error, `queue_hits` not found.

- [ ] **Step 4: Implement** (top of `search/queue.rs`)

```rust
//! The queue as a search source: what is queued now, matched on title and
//! artist. Hidden while a `/gp` game owns playback.

use super::{Hit, HitDetail, Query, SavedTrack, SourceResult, HITS_PER_SOURCE};
use crate::music::remote::{self, QueueState};
use crate::Data;
use serenity::all::GuildId;

/// The queue in `guild`, searched for `q`.
pub(crate) async fn search_queue(data: &Data, guild: GuildId, q: &Query) -> SourceResult {
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
    let contains = |s: &Option<String>| s.as_deref().is_some_and(|s| s.to_lowercase().contains(&needle));
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
```

- [ ] **Step 5: Run tests, sabotage, commit**

Run: `cargo test -p crack-core --lib music::search && cargo test -p crack-web` — expected: all pass.
Sabotages: drop `to_lowercase` on the needle; match title only; return `Hits` for `Hidden`; remove `.take`; skip the URL check. Record each.

```bash
cargo +nightly fmt --all
git add crack-core/src/music/remote.rs crack-core/src/music/ops/skip.rs crack-core/src/music/ops/edit.rs crack-web/src/view.rs crack-core/src/music/search/queue.rs crack-core/src/music/search/mod.rs
git commit -m "feat(search): the queue source; TrackSummary carries the artist

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The history source

**Files:**
- Create: `crack-core/src/db/play_history.rs`
- Modify: `crack-core/src/db/mod.rs` (`pub mod play_history;`)
- Create: `crack-core/src/music/search/history.rs`
- Modify: `crack-core/src/music/search/mod.rs` (`mod history;`)
- Create: `.sqlx/query-<hash>.json` (generated)

**Interfaces:**
- Consumes: Task 1 types.
- Produces:
  ```rust
  // crack_core::db::play_history
  pub struct HistoryTrack { pub url: String, pub title: Option<String>, pub artist: Option<String>,
                            pub duration_secs: Option<u64>, pub plays: u32, pub last: DateTime<Utc> }
  pub fn contains_pattern(q: &str) -> String;
  pub async fn search_play_log(pool: &PgPool, guild: GuildId, q: &str,
                               since: Option<DateTime<Utc>>, limit: i64) -> sqlx::Result<Vec<HistoryTrack>>;
  // crack_core::music::search::history
  pub(crate) fn history_hits(rows: Vec<HistoryTrack>) -> SourceResult;
  pub(crate) async fn search_history(data: &Data, guild: GuildId, q: &Query,
                                     since: Option<DateTime<Utc>>) -> SourceResult;
  ```

- [ ] **Step 1: Write the failing tests**

In `crack-core/src/db/play_history.rs`, the test module:

```rust
#[cfg(test)]
mod test {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[test]
    fn the_pattern_matches_the_text_literally() {
        assert_eq!(contains_pattern("abba"), "%abba%");
        assert_eq!(contains_pattern("100%"), "%100\\%%");
        assert_eq!(contains_pattern("a_b"), "%a\\_b%");
        assert_eq!(contains_pattern("AC\\DC"), "%AC\\\\DC%");
    }

    /// A metadata row and `plays` play_log rows for it, `mins_ago` apart.
    async fn played(
        pool: &PgPool,
        guild: i64,
        url: &str,
        title: Option<&str>,
        artist: Option<&str>,
        duration: i64,
        mins_ago: &[i64],
    ) {
        let (id,): (i32,) = sqlx::query_as(
            "INSERT INTO metadata (title, artist, source_url, duration) VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(title)
        .bind(artist)
        .bind(url)
        .bind(duration)
        .fetch_one(pool)
        .await
        .unwrap();
        for m in mins_ago {
            sqlx::query(
                "INSERT INTO play_log (user_id, guild_id, metadata_id, created_at)
                 VALUES (1, $1, $2, (now() AT TIME ZONE 'utc') - make_interval(mins => $3::int))",
            )
            .bind(guild)
            .bind(id)
            .bind(*m as i32)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn second_guild(pool: &PgPool) {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause,
                 allow_all_domains, allowed_domains, banned_domains, ignored_channels,
                 old_volume, volume, self_deafen, timeout_seconds, additional_prefixes)
             VALUES (2, 'two', 'r!', false, false, true, '{}', '{}', '{}', 1.0, 1.0, true, 360, '{}')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn one_row_per_track_newest_first_this_guild_only(pool: PgPool) {
        second_guild(&pool).await;
        played(&pool, 1, "https://y/old", Some("Zebra Song"), Some("Zed"), 200, &[300, 200]).await;
        played(&pool, 1, "https://y/new", Some("zebra crossing"), None, 0, &[5]).await;
        played(&pool, 2, "https://y/elsewhere", Some("Zebra elsewhere"), None, 10, &[1]).await;
        played(&pool, 1, "https://y/byartist", Some("Untitled"), Some("The Zebras"), 30, &[60]).await;
        let got = search_play_log(&pool, GuildId::new(1), "ZEBRA", None, 10).await.unwrap();
        let urls: Vec<&str> = got.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, ["https://y/new", "https://y/byartist", "https://y/old"]);
        assert_eq!((got[2].plays, got[2].duration_secs), (2, Some(200)));
        assert_eq!(got[0].duration_secs, None, "0 seconds is unknown");
        assert!(got[0].last > got[1].last && got[1].last > got[2].last);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_window_the_limit_and_literal_wildcards(pool: PgPool) {
        played(&pool, 1, "https://y/recent", Some("100% hits"), None, 1, &[30]).await;
        played(&pool, 1, "https://y/ancient", Some("100% old"), None, 1, &[60 * 48]).await;
        played(&pool, 1, "https://y/decoy", Some("1000 hits"), None, 1, &[10]).await;
        played(&pool, 1, "https://y/null", None, None, 1, &[10]).await;
        let since = Some(Utc::now() - chrono::Duration::hours(24));
        let got = search_play_log(&pool, GuildId::new(1), "100%", since, 10).await.unwrap();
        let urls: Vec<&str> = got.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, ["https://y/recent"], "the % is literal and the window holds");
        let all = search_play_log(&pool, GuildId::new(1), "100%", None, 10).await.unwrap();
        assert_eq!(all.len(), 2);
        let one = search_play_log(&pool, GuildId::new(1), "100%", None, 1).await.unwrap();
        assert_eq!(one.len(), 1);
    }
}
```

In `crack-core/src/music/search/history.rs`, the test module:

```rust
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
        let data = crate::Data::default();
        let q = Query::new("abba").unwrap();
        assert_eq!(
            search_history(&data, serenity::all::GuildId::new(1), &q, None).await,
            SourceResult::NoDatabase
        );
    }
}
```

(If `crate::Data` has no `Default`, build it as `crate::Data(std::sync::Arc::new(crate::DataInner::default()))`, as `ops::test_support` does.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p crack-core --lib play_history music::search::history`
Expected: compile errors.

- [ ] **Step 3: Implement**

`crack-core/src/db/play_history.rs` (above its tests):

```rust
//! Track search over a guild's play log: the dashboard's history source.
//! `play_log` is written when a `/play` path (and, from v0.27.0, a dashboard
//! add) queues a track; `/gp` never writes it, so a search here cannot reveal
//! a running game's songs.

use chrono::{DateTime, Utc};
use serenity::all::GuildId;
use sqlx::PgPool;

/// One track the guild has queued, with how often and when last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTrack {
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Whole seconds; `None` where the log holds 0 (unknown).
    pub duration_secs: Option<u64>,
    pub plays: u32,
    pub last: DateTime<Utc>,
}

/// `q` as an `ILIKE` pattern matching it literally anywhere: `\`, `%` and `_`
/// escaped with `\` (the default escape character), then wrapped in `%`.
#[must_use]
pub fn contains_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// Tracks queued in `guild` whose title or artist contains `q`, one row per
/// URL, newest first. `since` bounds the window (a free server's 24 hours).
/// No member column is read: a search shows tracks, never who queued them.
///
/// 🪤 `play_log.created_at` is `TIMESTAMP` without a time zone, written as UTC
/// by `CURRENT_TIMESTAMP` on a UTC server: compare and read it as naive UTC.
pub async fn search_play_log(
    pool: &PgPool,
    guild: GuildId,
    q: &str,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> sqlx::Result<Vec<HistoryTrack>> {
    let pattern = contains_pattern(q);
    let since = since.map(|t| t.naive_utc());
    let rows = sqlx::query!(
        r#"SELECT m.source_url AS "url!",
                  (array_agg(m.title ORDER BY p.created_at DESC))[1] AS title,
                  (array_agg(m.artist ORDER BY p.created_at DESC))[1] AS artist,
                  (array_agg(m.duration ORDER BY p.created_at DESC))[1] AS "duration!",
                  count(*) AS "plays!",
                  max(p.created_at) AS "last!"
           FROM play_log p
           JOIN metadata m ON m.id = p.metadata_id
           WHERE p.guild_id = $1
             AND m.source_url IS NOT NULL
             AND ($3::timestamp IS NULL OR p.created_at >= $3)
             AND (m.title ILIKE $2 OR m.artist ILIKE $2)
           GROUP BY m.source_url
           ORDER BY max(p.created_at) DESC
           LIMIT $4"#,
        guild.get() as i64,
        pattern,
        since,
        limit,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| HistoryTrack {
            url: r.url,
            title: r.title,
            artist: r.artist,
            duration_secs: u64::try_from(r.duration).ok().filter(|&d| d > 0),
            plays: u32::try_from(r.plays).unwrap_or(u32::MAX),
            last: r.last.and_utc(),
        })
        .collect())
}
```

If sqlx infers `title`/`artist` as non-`Option`, annotate them `AS "title?"` / `AS "artist?"`. Add `pub mod play_history;` to `crack-core/src/db/mod.rs`.

`crack-core/src/music/search/history.rs` (above its tests):

```rust
//! This server's history as a search source: tracks it has queued, without
//! who queued them.

use super::{Hit, HitDetail, Query, SavedTrack, SourceResult, HITS_PER_SOURCE};
use crate::db::play_history::{search_play_log, HistoryTrack};
use crate::Data;
use chrono::{DateTime, Utc};
use serenity::all::GuildId;
use std::time::Duration;

/// History hits for `q` in `guild` since `since` (`None`: all of it).
pub(crate) async fn search_history(
    data: &Data,
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
```

Add `mod history;` to `search/mod.rs`.

- [ ] **Step 4: Generate `.sqlx`, run the tests**

Start the postgres from Global Constraints if it isn't running. Run the `.sqlx` regeneration commands from Global Constraints. Then:
Run: `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres cargo test -p crack-core --features db-tests --lib play_history music::search`
Expected: all pass (the two db-tests included). Also `SQLX_OFFLINE=true cargo check -p crack-core --all-targets` — expected clean (proves the `.sqlx` file is committed-ready).

- [ ] **Step 5: Sabotage, record, commit**

Sabotages: stop escaping `%` in `contains_pattern`; drop `p.guild_id = $1`; drop the `since` clause; `ORDER BY max(p.created_at) ASC`; let `duration 0` through as `Some(0)`; skip the URL check in `history_hits`.

```bash
cargo +nightly fmt --all
git add crack-core/src/db/play_history.rs crack-core/src/db/mod.rs crack-core/src/music/search/history.rs crack-core/src/music/search/mod.rs .sqlx/query-*.json
git status --short   # confirm only the intended new .sqlx file(s) were staged
git commit -m "feat(search): the history source over play_log, one row per track, no member names

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: External sources and `search()`

**Files:**
- Modify: `crack-types/src/http.rs` (add `Site`, `ytdl_search`, tests)
- Modify: `clippy.toml:103-104` (name `ytdl_search` in the two `YoutubeDl::new*` reasons)
- Modify: `crack-testing/src/lib.rs` (add `search_metadata`)
- Create: `crack-core/src/music/search/youtube.rs`, `crack-core/src/music/search/soundcloud.rs`
- Modify: `crack-core/src/music/search/mod.rs` (`mod youtube; mod soundcloud;` and `pub async fn search`)

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces:
  ```rust
  // crack_types
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Site { YouTube, SoundCloud }
  pub fn ytdl_search(client: reqwest::Client, site: Site, query: &str, n: usize) -> songbird::input::YoutubeDl<'static>;
  // crack_testing::CrackTrackClient
  pub async fn search_metadata(&self, query: &str, limit: u64) -> Result<Vec<AuxMetadata>, Error>;
  // crack_core::music::search
  pub async fn search(data: &Data, guild: GuildId, query: &Query, sources: SourceSet,
                      history_since: Option<DateTime<Utc>>) -> Vec<(Source, SourceResult)>;
  ```

- [ ] **Step 1: Write the failing 🔒 tests** in `crack-types/src/http.rs`'s test module

```rust
    #[test]
    fn search_text_reaches_yt_dlp_only_behind_its_prefix() {
        let client = reqwest::Client::new();
        for (site, prefix) in [(Site::YouTube, "ytsearch5:"), (Site::SoundCloud, "scsearch5:")] {
            for text in ["abba", "-rf --exec rm", "--batch-file=/proc/self/environ"] {
                let dbg = format!("{:?}", ytdl_search(client.clone(), site, text, 5));
                assert!(
                    dbg.contains(&format!("\"{prefix}{text}\"")),
                    "{site:?} {text:?} must be one string starting with {prefix}: {dbg}"
                );
            }
        }
    }

    #[test]
    fn soundcloud_search_sorts_to_mp3_as_its_playback_does() {
        let client = reqwest::Client::new();
        let sc = format!("{:?}", ytdl_search(client.clone(), Site::SoundCloud, "abba", 5));
        assert!(sc.contains(SOUNDCLOUD_FORMAT_SORT), "{sc}");
        let yt = format!("{:?}", ytdl_search(client, Site::YouTube, "abba", 5));
        assert!(!yt.contains(SOUNDCLOUD_FORMAT_SORT), "{yt}");
    }
```

Run: `cargo test -p crack-types search_text_reaches` — expected: compile error (`Site`, `ytdl_search` missing).

- [ ] **Step 2: Implement `ytdl_search`** next to `ytdl_for_url` in `crack-types/src/http.rs`

```rust
/// Which site a yt-dlp search asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    YouTube,
    SoundCloud,
}

/// 🔒 The one way to hand yt-dlp search text: `"{ytsearch|scsearch}{n}:{text}"`
/// as a single argument. The prefix means the string starts with a letter, so
/// yt-dlp can never read the member's text as an option (the v0.17.2 class).
/// songbird's own `new_search` hardcodes `ytsearch`, so SoundCloud needs this.
#[must_use]
pub fn ytdl_search(
    client: Client,
    site: Site,
    query: &str,
    n: usize,
) -> songbird::input::YoutubeDl<'static> {
    let (prefix, args) = match site {
        Site::YouTube => ("ytsearch", Vec::new()),
        Site::SoundCloud => (
            "scsearch",
            vec!["-S".to_owned(), SOUNDCLOUD_FORMAT_SORT.to_owned()],
        ),
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "the second sanctioned call: the string starts with `ytsearchN:` or `scsearchN:`, never `-`"
    )]
    let ytdl = songbird::input::YoutubeDl::new(client, format!("{prefix}{n}:{query}"));
    ytdl.user_args(args)
}
```

In `clippy.toml`, change both reasons at lines 103–104 to end `…; use crack_types::ytdl_for_url for links, YoutubeDl::new_search or crack_types::ytdl_search for search text`.
Run: `cargo test -p crack-types` — expected: pass.

- [ ] **Step 3: Add `search_metadata`** to `impl CrackTrackClient` in `crack-testing/src/lib.rs` (next to `resolve_search`)

```rust
    /// rusty_ytdl's video results for `query`, as metadata, up to `limit`.
    /// One request: a search hit already carries title, channel, duration and
    /// url, so nothing is resolved further.
    pub async fn search_metadata(&self, query: &str, limit: u64) -> Result<Vec<AuxMetadata>, Error> {
        let options = rusty_ytdl::search::SearchOptions {
            limit,
            ..Default::default()
        };
        let results = self.yt_client.search(query, Some(&options)).await?;
        Ok(results
            .into_iter()
            .filter_map(|r| match r {
                SearchResult::Video(video) => {
                    Some(crack_types::metadata::search_video_to_aux_metadata(&video))
                },
                _ => None,
            })
            .collect())
    }
```
(`AuxMetadata`, `SearchResult` and `Error` are already imported in this file; add any that are not. `SearchOptions::limit` is `u64` in rusty_ytdl; if it is another integer type, convert.)
Add an ignored live test beside the existing ones:
```rust
    #[tokio::test]
    #[ignore = "hits live YouTube"]
    async fn search_metadata_finds_videos_with_urls() {
        let hits = CrackTrackClient::new().search_metadata("abba waterloo", 5).await.unwrap();
        assert!(!hits.is_empty() && hits.iter().all(|m| m.source_url.is_some()));
    }
```
Run: `cargo test -p crack-testing` — expected: pass (the new one ignored).

- [ ] **Step 4: Write the failing tests for the source mappers**

`crack-core/src/music/search/youtube.rs` tests:
```rust
#[cfg(test)]
mod test {
    use super::*;
    use songbird::input::AuxMetadata;

    fn m(url: Option<&str>) -> AuxMetadata {
        AuxMetadata { title: Some("t".into()), source_url: url.map(str::to_owned), ..Default::default() }
    }

    #[test]
    fn found_hits_keep_order_drop_non_web_and_cap() {
        let mut metas = vec![m(Some("https://y/a")), m(None), m(Some("https://y/b"))];
        metas.extend((0..20).map(|i| m(Some(&format!("https://y/{i}")))));
        let SourceResult::Hits(hits) = found(metas) else { panic!() };
        assert_eq!(hits.len(), HITS_PER_SOURCE);
        assert_eq!((hits[0].track.url.as_str(), hits[1].track.url.as_str()), ("https://y/a", "https://y/b"));
        assert!(hits.iter().all(|h| h.detail == HitDetail::Found));
    }

    #[tokio::test]
    #[ignore = "hits live YouTube"]
    async fn a_live_youtube_search_finds_something() {
        let data = crate::Data::default();
        let SourceResult::Hits(h) = search_youtube(&data, &Query::new("abba waterloo").unwrap()).await else { panic!() };
        assert!(!h.is_empty());
    }
}
```
`crack-core/src/music/search/soundcloud.rs` tests:
```rust
#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    #[ignore = "hits live SoundCloud and needs yt-dlp on PATH"]
    async fn a_live_soundcloud_search_finds_soundcloud_links() {
        let SourceResult::Hits(h) = search_soundcloud(&Query::new("lofi").unwrap()).await else { panic!() };
        assert!(!h.is_empty() && h.iter().all(|x| x.track.url.contains("soundcloud.com")));
    }
}
```
And in `search/mod.rs`'s test module:
```rust
    #[tokio::test]
    async fn search_runs_only_what_was_asked_and_answers_in_order() {
        let data = crate::Data::default();   // no call, no pool
        let q = Query::new("abba").unwrap();
        let out = search(&data, serenity::all::GuildId::new(1), &q,
                         SourceSet::empty().with(Source::History).with(Source::Queue), None).await;
        assert_eq!(out, vec![
            (Source::Queue, SourceResult::Hits(vec![])),
            (Source::History, SourceResult::NoDatabase),
        ]);
        assert_eq!(search(&data, serenity::all::GuildId::new(1), &q, SourceSet::empty(), None).await, vec![]);
    }
```
Run: `cargo test -p crack-core --lib music::search` — expected: compile errors.

- [ ] **Step 5: Implement the sources and `search()`**

`search/youtube.rs`:
```rust
//! YouTube as a search source: rusty_ytdl's search (one request, metadata
//! included), and yt-dlp's `ytsearch` when rusty_ytdl errors.

use super::{gated, saved_from_metadata, Hit, HitDetail, Query, SourceResult, EXTERNAL, HITS_PER_SOURCE};
use crate::Data;
use songbird::input::AuxMetadata;

/// How many hits the yt-dlp fallback asks for: it extracts each one in full.
pub(crate) const YTDLP_HITS: usize = 5;

pub(crate) async fn search_youtube(data: &Data, q: &Query) -> SourceResult {
    gated(&EXTERNAL, async {
        match data.ct_client.search_metadata(q.as_str(), HITS_PER_SOURCE as u64).await {
            Ok(metas) => found(metas),
            Err(e) => {
                tracing::warn!("search: rusty_ytdl failed, asking yt-dlp: {e}");
                let mut ytdl = crack_types::ytdl_search(
                    crate::http_utils::get_client_old().clone(),
                    crack_types::Site::YouTube,
                    q.as_str(),
                    YTDLP_HITS,
                );
                match ytdl.search(Some(YTDLP_HITS)).await {
                    Ok(metas) => found(metas.collect()),
                    Err(e) => {
                        tracing::warn!("search: yt-dlp YouTube search failed: {e}");
                        SourceResult::Failed
                    },
                }
            },
        }
    })
    .await
}

/// Search metadata as `Found` hits, in order, web URLs only, capped.
pub(crate) fn found(metas: Vec<AuxMetadata>) -> SourceResult {
    SourceResult::Hits(
        metas
            .iter()
            .filter_map(saved_from_metadata)
            .take(HITS_PER_SOURCE)
            .map(|track| Hit { track, detail: HitDetail::Found })
            .collect(),
    )
}
```

`search/soundcloud.rs`:
```rust
//! SoundCloud as a search source: yt-dlp's `scsearch`, through the one
//! sanctioned search constructor.

use super::{gated, youtube::found, Query, SourceResult, EXTERNAL};

/// yt-dlp extracts each hit in full, so SoundCloud asks for five.
pub(crate) const SOUNDCLOUD_HITS: usize = 5;

pub(crate) async fn search_soundcloud(q: &Query) -> SourceResult {
    gated(&EXTERNAL, async {
        let mut ytdl = crack_types::ytdl_search(
            crate::http_utils::get_client_old().clone(),
            crack_types::Site::SoundCloud,
            q.as_str(),
            SOUNDCLOUD_HITS,
        );
        match ytdl.search(Some(SOUNDCLOUD_HITS)).await {
            Ok(metas) => found(metas.collect()),
            Err(e) => {
                tracing::warn!("search: yt-dlp SoundCloud search failed: {e}");
                SourceResult::Failed
            },
        }
    })
    .await
}
```

In `search/mod.rs` add `mod youtube; mod soundcloud;` and:
```rust
/// Search `query` over `sources` in `guild`. Every source runs at once under
/// its own bound; the answer is in [`Source`] order, one entry per source
/// asked. `history_since` is the plan's window (`None`: all of it).
pub async fn search(
    data: &crate::Data,
    guild: serenity::all::GuildId,
    query: &Query,
    sources: SourceSet,
    history_since: Option<DateTime<Utc>>,
) -> Vec<(Source, SourceResult)> {
    let mut jobs: Vec<Job<'_>> = Vec::new();
    if sources.contains(Source::Queue) {
        jobs.push((Source::Queue, LOCAL_TIMEOUT, Box::pin(queue::search_queue(data, guild, query))));
    }
    if sources.contains(Source::History) {
        jobs.push((Source::History, LOCAL_TIMEOUT,
                   Box::pin(history::search_history(data, guild, query, history_since))));
    }
    if sources.contains(Source::YouTube) {
        jobs.push((Source::YouTube, EXTERNAL_TIMEOUT, Box::pin(youtube::search_youtube(data, query))));
    }
    if sources.contains(Source::SoundCloud) {
        jobs.push((Source::SoundCloud, EXTERNAL_TIMEOUT, Box::pin(soundcloud::search_soundcloud(query))));
    }
    gather(jobs).await
}
```
Remove any `dead_code` `cfg_attr`s added in Task 1.

- [ ] **Step 6: Run, sabotage, commit**

Run: `cargo test -p crack-core --lib music::search && cargo test -p crack-types && cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
Expected: pass, clippy clean.
Sabotages: build `ytdl_search`'s string without the prefix; drop SoundCloud's `-S`; let `search` push a job for a source not asked for; drop `.take` in `found`.

```bash
cargo +nightly fmt --all
git add crack-types/src/http.rs clippy.toml crack-testing/src/lib.rs crack-core/src/music/search/youtube.rs crack-core/src/music/search/soundcloud.rs crack-core/src/music/search/mod.rs
git commit -m "feat(search): YouTube and SoundCloud sources behind one sanctioned yt-dlp search constructor; search()

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: `ops::add` and the play-next primitive

**Files:**
- Modify: `crack-core/src/music/queue.rs` (add `enqueue_resolved_track_next`; extend the pinned `SURFACE`; update `enqueue_resolved_tracks_back`'s caller note in `SURFACE`)
- Create: `crack-core/src/music/ops/add.rs`
- Modify: `crack-core/src/music/ops/mod.rs` (`mod add; pub use add::*;`)
- Modify: `crack-core/src/messaging/messages.rs` (`ADD_UNPLAYABLE`)

**Interfaces:**
- Consumes: `ops::{OpCx, begin, Done, Settle, OpRefused}`, `queue::{enqueue_resolved_tracks_back, build_track, enqueue, preload_time, record_add}`, `crack_testing::ResolvedTrack::from_saved`, `crate::db::worker_pool::MetadataMsg`.
- Produces:
  ```rust
  // crack_core::music::queue
  pub async fn enqueue_resolved_track_next(guard: &QueueGuard, call: &Arc<Mutex<Call>>,
      track: ResolvedTrack<'static>, http_client: reqwest::Client) -> Result<TrackHandle, CrackedError>;
  // crack_core::music::ops
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum At { End, Next }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Added { pub title: Option<String> }
  pub async fn add(cx: &OpCx, track: SavedTrack, at: At, member_name: &str) -> Result<Done<Added>, OpRefused>;
  pub(crate) async fn add_on(guard: &QueueGuard, call: &Arc<Mutex<Call>>, track: &SavedTrack, at: At, user: UserId) -> Result<Added, OpRefused>;
  ```

- [ ] **Step 1: Write the failing tests** in `crack-core/src/music/ops/add.rs`

```rust
#[cfg(test)]
mod test {
    use super::super::test_support::{guard, ids_of, queue_of, recorded, GUILD};
    use super::*;
    use crate::music::audit::{Action, AddAt};
    use serenity::all::UserId;

    fn song(n: u32) -> SavedTrack {
        SavedTrack {
            url: format!("https://www.youtube.com/watch?v=song{n}"),
            title: Some(format!("Song {n}")),
            artist: Some("Band".into()),
            duration: Some(std::time::Duration::from_secs(100)),
        }
    }

    async fn requester_of(call: &Arc<Mutex<Call>>, at: usize) -> UserId {
        let h = call.lock().await.queue().current_queue()[at].clone();
        crate::utils::get_requesting_user(&h).await.unwrap()
    }

    async fn url_of(call: &Arc<Mutex<Call>>, at: usize) -> Option<String> {
        let h = call.lock().await.queue().current_queue()[at].clone();
        crate::utils::get_track_handle_metadata(&h).await.ok()?.source_url
    }

    #[tokio::test]
    async fn end_appends_as_the_member_and_records_one_back_add() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        let added = add_on(&g, &call, &song(7), At::End, UserId::new(42)).await.unwrap();
        assert_eq!(added, Added { title: Some("Song 7".into()) });
        let now = ids_of(&call).await;
        assert_eq!((now.len(), &now[..3]), (4, &ids[..]));
        assert_eq!(url_of(&call, 3).await.as_deref(), Some("https://www.youtube.com/watch?v=song7"));
        assert_eq!(requester_of(&call, 3).await, UserId::new(42));
        assert!(matches!(&recorded(&mut rx)[..], [Action::Add { at: AddAt::Back, tracks }] if tracks.len() == 1));
    }

    #[tokio::test]
    async fn next_lands_right_behind_the_playing_track() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        add_on(&g, &call, &song(8), At::Next, UserId::new(42)).await.unwrap();
        let now = ids_of(&call).await;
        assert_eq!((now[0], now[2], now[3]), (ids[0], ids[1], ids[2]));
        assert_eq!(url_of(&call, 1).await.as_deref(), Some("https://www.youtube.com/watch?v=song8"));
        assert!(matches!(&recorded(&mut rx)[..], [Action::Add { at: AddAt::Front, .. }]));
    }

    #[tokio::test]
    async fn next_with_nothing_between_stays_last() {
        for n in [0, 1] {
            let (data, call, ids, _rx) = queue_of(n).await;
            let g = guard(&data).await;
            add_on(&g, &call, &song(9), At::Next, UserId::new(42)).await.unwrap();
            let now = ids_of(&call).await;
            assert_eq!((now.len(), &now[..n]), (n + 1, &ids[..]), "{n} queued before");
            assert_eq!(url_of(&call, n).await.as_deref(), Some("https://www.youtube.com/watch?v=song9"));
        }
    }

    #[tokio::test]
    async fn add_refuses_without_a_call() {
        let cx = super::super::test_support::cx_without_call();
        assert!(matches!(add(&cx, song(1), At::End, "m").await, Err(OpRefused::NotConnected)));
    }

    #[tokio::test]
    async fn add_refuses_during_a_game() {
        let d = crate::Data(Arc::new(crate::DataInner::default()));
        d.claim_playback(GUILD, crate::music::PlaybackOwner::Game).unwrap();
        let cx = OpCx {
            data: Arc::new(d),
            ..super::super::test_support::cx_without_call()
        };
        assert!(matches!(add(&cx, song(1), At::End, "m").await, Err(OpRefused::GameInProgress)));
    }

    #[test]
    fn the_play_log_message_names_the_member_and_the_track() {
        let msg = play_log_message(&song(3), UserId::new(42), "lothrop", GUILD, Some(serenity::all::ChannelId::new(77)));
        assert_eq!(msg.user_id, Some(UserId::new(42)));
        assert_eq!(msg.username.as_deref(), Some("lothrop"));
        assert_eq!(msg.guild_id, GUILD);
        assert_eq!(msg.channel_id.get(), 77);
        assert_eq!(msg.aux_metadata.source_url.as_deref(), Some("https://www.youtube.com/watch?v=song3"));
        assert_eq!(msg.aux_metadata.title.as_deref(), Some("Song 3"));
    }
}
```

Notes for the implementer:
- `test_support::queue_of(n)` builds `n` file tracks on an offline call; `guard(&data)` locks the queue as a web member.
- `crate::utils::get_requesting_user` / `get_track_handle_metadata` are the sanctioned readers (`TrackHandle::data` is banned).

Run: `cargo test -p crack-core --lib music::ops::add` — expected: compile errors.

- [ ] **Step 2: Implement the primitive** in `crack-core/src/music/queue.rs`, after `enqueue_resolved_tracks_back`

```rust
/// Queue one resolved track to play next: behind the playing track, ahead of
/// everything else. Returns its handle.
///
/// The move happens under the same call lock as the enqueue, so nothing can
/// land between them. With nothing after the playing track (or nothing
/// playing) the new track is already next, and stays where it went in, as
/// `/playnext` does.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. Takes no ctx and so cannot log; its caller, `ops::add`, sends its
/// own play-log message.
#[expect(
    clippy::disallowed_methods,
    reason = "an audited primitive: records below; see clippy.toml"
)]
pub async fn enqueue_resolved_track_next(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track: ResolvedTrack<'static>,
    http_client: reqwest::Client,
) -> Result<TrackHandle, CrackedError> {
    let built = build_track(&track, &http_client)?;
    let mut handler = call.lock().await;
    let handle = enqueue(&mut handler, built, preload_time(&track));
    if handler.queue().len() >= 3 {
        handler.queue().modify_queue(|q| {
            if let Some(back) = q.pop_back() {
                q.insert(1, back);
            }
        });
    }
    record_add(guard, &handler, std::slice::from_ref(&handle), AddAt::Front);
    Ok(handle)
}
```

In the `SURFACE` table add:
```rust
        (
            "enqueue_resolved_track_next",
            Primitive("caller: ops::add, which sends its own play-log message"),
        ),
```
and change the `enqueue_resolved_tracks_back` entry's text to `"callers: queue_vec_query_type, queue_resolved_list_back, ops::add (which sends its own play-log message)"`.
Run: `cargo test -p crack-core --lib music::queue` — expected: the surface tests pass.

- [ ] **Step 3: Implement `ops::add`** in `crack-core/src/music/ops/add.rs`

```rust
//! Add one track the member chose (a dashboard search hit) to the end of the
//! queue or to play next. The one op that queues; it closes the gap the ops
//! layer left. Spec §2.
use super::*;
use crate::{
    db::worker_pool::MetadataMsg,
    messaging::messages::ADD_UNPLAYABLE,
    music::queue::{enqueue_resolved_track_next, enqueue_resolved_tracks_back},
};
use crack_testing::ResolvedTrack;
use crack_types::SavedTrack;
use serenity::all::{ChannelId, GenericChannelId, UserId};

/// Where an added track goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    End,
    Next,
}

/// What an add queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub title: Option<String>,
}

/// Add `track` for the acting member. `member_name` is written to the play
/// log with it: `User::insert_or_update_user` overwrites the stored name with
/// whatever it is given, so it must be the member's real one.
pub async fn add(
    cx: &OpCx,
    track: SavedTrack,
    at: At,
    member_name: &str,
) -> Result<Done<Added>, OpRefused> {
    let user = cx.actor.user().ok_or(OpRefused::Invalid(ADD_UNPLAYABLE))?;
    let (g, call) = begin(cx).await?;
    // A member queued something: autoplay's buffered picks no longer apply,
    // as for /play (Ruling 49).
    cx.data.autoplay_buffer.clear(cx.guild_id);
    let added = add_on(&g, &call, &track, at, user).await?;
    let channel = call.lock().await.current_channel().map(|c| ChannelId::new(c.get()));
    if let Some(tx) = &cx.data.db_channel {
        let msg = play_log_message(&track, user, member_name, cx.guild_id, channel);
        // try_send, as the /play paths do: a wedged writer must not block an add.
        if let Err(e) = tx.try_send(msg) {
            tracing::error!("add: could not queue the play log write: {e}");
        }
    }
    Ok(Done {
        outcome: added,
        settle: Settle::QueueMessages,
        call: Some(call),
    })
}

/// The add itself, under `guard`, on `call`. No Discord, no database.
pub(crate) async fn add_on(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track: &SavedTrack,
    at: At,
    user: UserId,
) -> Result<Added, OpRefused> {
    let resolved = ResolvedTrack::from_saved(track).with_user_id(user);
    let http = crate::http_utils::get_client_old().clone();
    match at {
        At::End => {
            let inserted = enqueue_resolved_tracks_back(guard, call, vec![resolved], http)
                .await
                .map_err(|_| OpRefused::Invalid(ADD_UNPLAYABLE))?;
            if inserted.count() == 0 {
                return Err(OpRefused::Invalid(ADD_UNPLAYABLE));
            }
        },
        At::Next => {
            enqueue_resolved_track_next(guard, call, resolved, http)
                .await
                .map_err(|_| OpRefused::Invalid(ADD_UNPLAYABLE))?;
        },
    }
    Ok(Added {
        title: track.title.clone(),
    })
}

/// The play-log write for an add: the track as metadata, the member, and the
/// bot's voice channel (the web has no text channel to name).
pub(crate) fn play_log_message(
    track: &SavedTrack,
    user: UserId,
    member_name: &str,
    guild: GuildId,
    voice: Option<ChannelId>,
) -> MetadataMsg {
    MetadataMsg {
        aux_metadata: track.to_metadata(),
        user_id: Some(user),
        username: Some(member_name.to_owned()),
        guild_id: guild,
        channel_id: GenericChannelId::new(voice.map_or(1, ChannelId::get)),
    }
}
```

Implementer notes:
- `Done`'s fields are private to `ops/mod.rs`; `add.rs` is a child module (`use super::*`), so the struct literal compiles there — other ops construct `Done` the same way; copy their construction if it differs (e.g. a `Done::new(..)` helper).
- `cx.actor.user()` is `Actor`'s existing accessor (`music/audit.rs:143`); a web actor always has one.
- Add to `messages.rs`: `pub const ADD_UNPLAYABLE: &str = "That track could not be queued.";`
- `GuildId` and `Arc`/`Mutex`/`Call`/`QueueGuard` come in through `use super::*`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p crack-core --lib music::ops music::queue`
Expected: all pass.

- [ ] **Step 5: Sabotage, record, commit**

Sabotages: insert at index 2 instead of 1; drop the `len() >= 3` guard (watch the "stays last" test); record `AddAt::Back` for Next; drop `.with_user_id(user)`; send `username: None`; skip `autoplay_buffer.clear` (note if nothing catches it — then add a test: put a pick in the buffer via its public API, add, assert empty).

```bash
cargo +nightly fmt --all
git add crack-core/src/music/queue.rs crack-core/src/music/ops/add.rs crack-core/src/music/ops/mod.rs crack-core/src/messaging/messages.rs
git commit -m "feat(ops): ops::add -- end or play next, as the member, logged to play_log

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: The add echo and `remote::add`

**Files:**
- Modify: `crack-core/src/messaging/cards.rs` (`Echo::Added`, `Echo::AddedNext`, `line`)
- Modify: `crack-core/src/messaging/messages.rs` (`ECHO_ADDED`, `ECHO_ADDED_NEXT`)
- Modify: `crack-core/src/music/remote.rs` (`AddRefused`, `add`)
- Modify: any exhaustive `match` on `Echo` the compiler names

**Interfaces:**
- Consumes: Task 5's `ops::{add, At, Added}`.
- Produces:
  ```rust
  // crack_core::messaging::cards::Echo
  Added { title: Option<String> }, AddedNext { title: Option<String> }
  // crack_core::music::remote
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum AddRefused { NotPlaying, GameInProgress, Failed }
  pub use crate::music::ops::At;
  pub async fn add(data: Arc<Data>, http: Arc<Http>, cache: Arc<Cache>, guild_id: GuildId, user: UserId,
                   member_name: String, via: Via, track: SavedTrack, at: At) -> Result<Option<Echo>, AddRefused>;
  ```

- [ ] **Step 1: Write the failing tests**

In `cards.rs`'s test module (next to the existing echo-line tests; find them with `grep -n "Echo::Skipped" crack-core/src/messaging/cards.rs`):
```rust
    #[test]
    fn add_echoes_name_the_track_where_it_went_and_who() {
        let u = UserId::new(5);
        assert_eq!(
            Echo::Added { title: Some("Waterloo".into()) }.line(u, Via::Dashboard),
            format!("{ECHO_ADDED} **Waterloo** {ECHO_FROM_DASHBOARD} — <@5>")
        );
        assert_eq!(
            Echo::AddedNext { title: Some("Waterloo".into()) }.line(u, Via::Dashboard),
            format!("{ECHO_ADDED_NEXT} **Waterloo** {ECHO_FROM_DASHBOARD} — <@5>")
        );
        let hostile = Echo::Added { title: Some("**x** @everyone".into()) }.line(u, Via::Dashboard);
        assert!(!hostile.contains("**x**"), "the title is escaped: {hostile}");
    }
```
(Import `ECHO_ADDED`, `ECHO_ADDED_NEXT` in the test module. Check how the existing `Removed` echo test asserts escaping and a blank title, and mirror it for `Added`, including the blank-title case the existing tests use.)

In `remote.rs`'s test module:
```rust
    #[test]
    fn add_refusals_map_to_what_the_dashboard_says() {
        use crate::music::ops::OpRefused as R;
        assert_eq!(AddRefused::from(&R::GameInProgress), AddRefused::GameInProgress);
        assert_eq!(AddRefused::from(&R::NotConnected), AddRefused::NotPlaying);
        assert_eq!(AddRefused::from(&R::Invalid("x")), AddRefused::Failed);
    }

    #[test]
    fn the_echo_follows_where_the_track_went() {
        let added = crate::music::ops::Added { title: Some("t".into()) };
        assert_eq!(add_echo(&added, At::End), Echo::Added { title: Some("t".into()) });
        assert_eq!(add_echo(&added, At::Next), Echo::AddedNext { title: Some("t".into()) });
    }
```
Run: `cargo test -p crack-core --lib cards remote` — expected: compile errors.

- [ ] **Step 2: Implement**

`messages.rs` (beside the other `ECHO_*`):
```rust
pub const ECHO_ADDED: &str = "➕ Added";
pub const ECHO_ADDED_NEXT: &str = "⏭ Up next:";
```
`cards.rs`: add the variants to `Echo` and arms to `line`:
```rust
            Self::Added { title } => (ECHO_ADDED, title.as_deref()),
            Self::AddedNext { title } => (ECHO_ADDED_NEXT, title.as_deref()),
```
Fix every other exhaustive `match` on `Echo` the compiler reports.

`remote.rs`:
```rust
pub use crate::music::ops::At;
use crack_types::SavedTrack;

/// Why an add was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddRefused {
    /// The bot is not connected in this guild.
    NotPlaying,
    /// A `/gp` game owns playback.
    GameInProgress,
    Failed,
}

impl From<&ops::OpRefused> for AddRefused {
    fn from(r: &ops::OpRefused) -> Self {
        match r {
            ops::OpRefused::GameInProgress => Self::GameInProgress,
            ops::OpRefused::NotConnected => Self::NotPlaying,
            other => {
                tracing::warn!("dashboard add refused: {other:?}");
                Self::Failed
            },
        }
    }
}

/// The echo for an add that went `at`.
pub(crate) fn add_echo(added: &ops::Added, at: At) -> Echo {
    match at {
        At::End => Echo::Added { title: added.title.clone() },
        At::Next => Echo::AddedNext { title: added.title.clone() },
    }
}

/// Add `track` for `user`, from the dashboard. The echo is returned at once;
/// posting it (if the guild's `control_echoes` is on) and settling run in the
/// background, the settle anchored after the echo, as `control` does.
#[allow(clippy::too_many_arguments)]
pub async fn add(
    data: Arc<Data>,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild_id: GuildId,
    user: UserId,
    member_name: String,
    via: Via,
    track: SavedTrack,
    at: At,
) -> Result<Option<Echo>, AddRefused> {
    let cx = ops::OpCx {
        data,
        http,
        cache,
        guild_id,
        actor: actor_for(via, user, "add"),
    };
    let done = ops::add(&cx, track, at, &member_name)
        .await
        .map_err(|r| AddRefused::from(&r))?;
    let echo = add_echo(done.outcome(), at);
    let (_, settle, call) = done.into_parts();
    let posted = echo.clone();
    tokio::spawn(async move {
        let transport = DiscordTransport {
            http: cx.http.clone(),
            cache: cx.cache.clone(),
        };
        let line = EchoLine { echo: posted, user, via };
        let anchor = courier::post(
            &cx.data,
            &transport,
            Destination::Echo(guild_id),
            &CrackedMessage::Echo(Box::new(line)),
            &RenderCx::now(),
        )
        .await;
        settle.after(&cx, call.as_ref(), anchor).await;
    });
    Ok(Some(echo))
}
```
Replace the `#[allow(clippy::too_many_arguments)]` with `#[expect(clippy::too_many_arguments, reason = "mirrors control(): the surface hands over its whole context")]` if clippy fires; drop it if it does not. Note `ops::add` takes the lease and releases it when `add` returns (the guard lives inside `ops::add`), so the background post runs without it, as `control` intends.

- [ ] **Step 3: Run tests, clippy**

Run: `cargo test -p crack-core --lib && cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
Expected: pass, clean.

- [ ] **Step 4: Sabotage, record, commit**

Sabotages: swap the two echo strings; map `NotConnected` to `Failed`; return `Echo::Added` for `At::Next`.

```bash
cargo +nightly fmt --all
git add crack-core/src/messaging/cards.rs crack-core/src/messaging/messages.rs crack-core/src/music/remote.rs
git commit -m "feat(remote): remote::add and its echo lines

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
(Add by name any other file the compiler made you change.)

---

### Task 7: crack-web — the hit memo, the two routes, the backend

**Files:**
- Create: `crack-web/src/hits.rs` (the memo)
- Create: `crack-web/src/find.rs` (wire types, parsing, both handlers, their tests)
- Modify: `crack-web/src/lib.rs` (`pub mod hits; pub mod find;`, `LiveBackend::{search, add}`, `WebState` fields in `start`)
- Modify: `crack-web/src/backend.rs` (trait methods; re-exports)
- Modify: `crack-web/src/routes.rs` (`WebState` gains `hits`, `search_limiter`; two `.route(...)` lines; make `user_id`, `is_json`, `plan_or_free`, `error_json`, `parse_guild`, `not_found`, `unavailable` `pub(crate)` if they aren't)
- Modify: `crack-web/src/limit.rs` (`SEARCHES_PER_WINDOW`, `SEARCH_WINDOW`)
- Modify: `crack-web/src/test_support.rs` (fake backend fields + methods; `state()` fills the new fields)
- Modify: `crack-core/src/messaging/messages.rs` (the JSON error strings)

**Interfaces:**
- Consumes: `crack_core::music::search::{search, Query, QueryRefused, Source, SourceSet, SourceResult, Hit, HitDetail, SavedTrack}`, `crack_core::music::remote::{add, AddRefused, At}`, `crack_core::guild::plan::Plan`.
- Produces:
  ```rust
  // crack_web::hits
  pub const HIT_TTL: Duration;  // 10 min
  pub struct HitMemo;  // new(ttl), store(user, guild, Vec<SavedTrack>, now) -> Vec<Uuid>, get(user, guild, id, now) -> Option<SavedTrack>
  // crack_web::backend::Backend
  fn search(&self, g: GuildId, q: &Query, sources: SourceSet, since: Option<DateTime<Utc>>)
      -> impl Future<Output = Vec<(Source, SourceResult)>> + Send;
  fn add(&self, user: UserId, name: &str, g: GuildId, track: SavedTrack, at: At)
      -> impl Future<Output = Result<(), AddRefused>> + Send;
  // routes: GET /g/{guild}/search, POST /g/{guild}/add
  ```

- [ ] **Step 1: The memo — failing tests** in `crack-web/src/hits.rs`

```rust
#[cfg(test)]
mod test {
    use super::*;

    fn t(n: u8) -> SavedTrack {
        SavedTrack { url: format!("https://y/{n}"), ..Default::default() }
    }
    const U: UserId = UserId::new(1);
    const G: GuildId = GuildId::new(5);

    #[test]
    fn a_hit_resolves_for_its_user_and_guild_only() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let ids = memo.store(U, G, vec![t(1), t(2)], now);
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(memo.get(U, G, ids[1], now), Some(t(2)));
        assert_eq!(memo.get(UserId::new(2), G, ids[1], now), None, "another user");
        assert_eq!(memo.get(U, GuildId::new(6), ids[1], now), None, "another guild");
        assert_eq!(memo.get(U, G, ids[1], now), Some(t(2)), "a lookup does not consume it");
    }

    #[test]
    fn a_new_search_replaces_the_old_hits_and_they_expire() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let old = memo.store(U, G, vec![t(1)], now);
        let new = memo.store(U, G, vec![t(3)], now);
        assert_eq!(memo.get(U, G, old[0], now), None);
        assert_eq!(memo.get(U, G, new[0], now), Some(t(3)));
        assert_eq!(memo.get(U, G, new[0], now + HIT_TTL), None);
    }

    #[test]
    fn past_the_sweep_size_expired_entries_are_dropped() {
        let memo = HitMemo::with_sweep(HIT_TTL, 2);
        let now = Instant::now();
        for u in 1..=3 {
            memo.store(UserId::new(u), G, vec![t(1)], now);
        }
        memo.store(UserId::new(9), G, vec![t(1)], now + HIT_TTL);
        assert_eq!(memo.len(), 1);
    }
}
```

- [ ] **Step 2: The memo — implementation**

```rust
//! Each member's last search results, per server, for a short while: an add
//! names a hit by id, so the browser never hands the server a URL.

use crack_core::music::search::SavedTrack;
use dashmap::DashMap;
use serenity::all::{GuildId, UserId};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// How long a search's hits can be added.
pub const HIT_TTL: Duration = Duration::from_secs(10 * 60);
/// Past this many entries, a store first drops the expired ones.
const SWEEP_AT: usize = 1024;

pub struct HitMemo {
    ttl: Duration,
    sweep_at: usize,
    entries: DashMap<(UserId, GuildId), (Instant, HashMap<Uuid, SavedTrack>)>,
}

impl HitMemo {
    pub fn new(ttl: Duration) -> Self {
        Self::with_sweep(ttl, SWEEP_AT)
    }

    pub fn with_sweep(ttl: Duration, sweep_at: usize) -> Self {
        Self { ttl, sweep_at, entries: DashMap::new() }
    }

    /// Replace `user`'s hits in `guild` with `tracks`; their ids, in order.
    pub fn store(&self, user: UserId, guild: GuildId, tracks: Vec<SavedTrack>, now: Instant) -> Vec<Uuid> {
        // 🔑 Before this entry is taken: `retain` locks every shard.
        if self.entries.len() > self.sweep_at {
            self.entries
                .retain(|_, (at, _)| now.saturating_duration_since(*at) < self.ttl);
        }
        let mut ids = Vec::with_capacity(tracks.len());
        let mut map = HashMap::with_capacity(tracks.len());
        for t in tracks {
            let id = Uuid::new_v4();
            ids.push(id);
            map.insert(id, t);
        }
        self.entries.insert((user, guild), (now, map));
        ids
    }

    /// The hit `id` from `user`'s last search in `guild`, if it is still fresh.
    pub fn get(&self, user: UserId, guild: GuildId, id: Uuid, now: Instant) -> Option<SavedTrack> {
        let entry = self.entries.get(&(user, guild))?;
        let (at, hits) = &*entry;
        if now.saturating_duration_since(*at) >= self.ttl {
            return None;
        }
        hits.get(&id).cloned()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}
```
Run: `cargo test -p crack-web hits` — expected: pass. (Clippy may want `is_empty` beside `len`: add it, or make `len` `#[cfg(test)]`.)

- [ ] **Step 3: Wire types and parsing — failing tests** in `crack-web/src/find.rs`

```rust
#[cfg(test)]
mod wire_test {
    use super::*;

    #[test]
    fn sources_and_statuses_have_these_wire_names() {
        let names: Vec<String> = [Source::Queue, Source::History, Source::YouTube, Source::SoundCloud]
            .into_iter()
            .map(|s| serde_json::to_string(&SourceName::from(s)).unwrap())
            .collect();
        assert_eq!(names, ["\"queue\"", "\"history\"", "\"youtube\"", "\"soundcloud\""]);
        let s = serde_json::to_string(&StatusView::TimedOut).unwrap();
        assert_eq!(s, "\"timed_out\"");
    }

    #[test]
    fn the_query_string_parses_or_says_why_not() {
        let ok = parse_search(Some("q=abba&src=history,youtube")).unwrap();
        assert_eq!(ok.0.as_str(), "abba");
        assert_eq!(ok.1, SourceSet::empty().with(Source::History).with(Source::YouTube));
        for (raw, why) in [
            (None, SEARCH_TOO_SHORT),
            (Some("q=a&src=queue"), SEARCH_TOO_SHORT),
            (Some("q=abba"), SEARCH_NO_SOURCES),
            (Some("q=abba&src="), SEARCH_NO_SOURCES),
            (Some("q=abba&src=queue,spotify"), SEARCH_UNKNOWN_SOURCE),
        ] {
            assert_eq!(parse_search(raw).unwrap_err().0, why, "{raw:?}");
        }
    }

    #[test]
    fn a_hit_view_falls_back_for_a_missing_title_and_keeps_the_detail() {
        let hit = Hit {
            track: SavedTrack { url: "https://y/1".into(), duration: Some(std::time::Duration::from_secs(61)), ..Default::default() },
            detail: HitDetail::Queue { position: 3 },
        };
        let v = hit_view(Uuid::from_u128(1), &hit);
        assert_eq!(v.title, "Unknown title");
        assert_eq!(v.duration_secs, Some(61));
        assert_eq!(serde_json::to_string(&v.detail).unwrap(), r#"{"kind":"queue","position":3}"#);
    }

    #[test]
    fn the_add_body_is_exact() {
        let ok: AddRequest = serde_json::from_str(r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"next"}"#).unwrap();
        assert_eq!((ok.hit, ok.at), (Uuid::from_u128(1), AtView::Next));
        assert!(serde_json::from_str::<AddRequest>(r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"top"}"#).is_err());
        assert!(serde_json::from_str::<AddRequest>(r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"end","url":"x"}"#).is_err());
    }
}
```

- [ ] **Step 4: Wire types and parsing — implementation** (top of `find.rs`)

Strings, in `crack-core/src/messaging/messages.rs`:
```rust
pub const SEARCH_TOO_SHORT: &str = "Type at least two characters to search.";
pub const SEARCH_TOO_LONG: &str = "That search is too long (100 characters at most).";
pub const SEARCH_CONTROL: &str = "That search has characters it cannot use.";
pub const SEARCH_NO_SOURCES: &str = "Pick at least one place to search.";
pub const SEARCH_UNKNOWN_SOURCE: &str = "That is not a place the dashboard can search.";
```

`find.rs`:
```rust
//! Search and add: `GET /g/{g}/search` and `POST /g/{g}/add`, their wire
//! types and their gates. Spec: docs/superpowers/specs/2026-10-13-dashboard-search-and-add-design.md §3.

use crate::{history::{BadQuery, ErrorBody}, view::QueueView};
use crack_core::messaging::messages::{
    SEARCH_CONTROL, SEARCH_NO_SOURCES, SEARCH_TOO_LONG, SEARCH_TOO_SHORT, SEARCH_UNKNOWN_SOURCE,
};
use crack_core::music::remote::At;
use crack_core::music::search::{Hit, HitDetail, Query, QueryRefused, SavedTrack, Source, SourceResult, SourceSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceName {
    #[serde(rename = "queue")]
    Queue,
    #[serde(rename = "history")]
    History,
    #[serde(rename = "youtube")]
    YouTube,
    #[serde(rename = "soundcloud")]
    SoundCloud,
}

impl From<Source> for SourceName {
    fn from(s: Source) -> Self {
        match s {
            Source::Queue => Self::Queue,
            Source::History => Self::History,
            Source::YouTube => Self::YouTube,
            Source::SoundCloud => Self::SoundCloud,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusView { Ok, Hidden, NoDatabase, Failed, TimedOut }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitDetailView {
    Queue { position: usize },
    History { plays: u32, last_unix: i64 },
    Found,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HitView {
    pub id: Uuid,
    pub title: String,
    pub artist: Option<String>,
    pub duration_secs: Option<u64>,
    pub url: String,
    pub detail: HitDetailView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceView {
    pub source: SourceName,
    pub status: StatusView,
    pub hits: Vec<HitView>,
}

/// The answer to a search.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub sources: Vec<SourceView>,
}

pub(crate) fn hit_view(id: Uuid, h: &Hit) -> HitView {
    HitView {
        id,
        title: h.track.title.clone().unwrap_or_else(|| "Unknown title".to_owned()),
        artist: h.track.artist.clone(),
        duration_secs: h.track.duration.map(|d| d.as_secs()),
        url: h.track.url.clone(),
        detail: match &h.detail {
            HitDetail::Queue { position } => HitDetailView::Queue { position: *position },
            HitDetail::History { plays, last } => HitDetailView::History { plays: *plays, last_unix: last.timestamp() },
            HitDetail::Found => HitDetailView::Found,
        },
    }
}

#[derive(Debug, Deserialize)]
struct RawSearch {
    q: Option<String>,
    src: Option<String>,
}

/// `q` and `src` from the query string.
pub(crate) fn parse_search(raw: Option<&str>) -> Result<(Query, SourceSet), BadQuery> {
    let raw: RawSearch = serde_urlencoded::from_str(raw.unwrap_or(""))
        .map_err(|_| BadQuery("the query string could not be read"))?;
    let q = Query::new(raw.q.as_deref().unwrap_or("")).map_err(|r| {
        BadQuery(match r {
            QueryRefused::TooShort => SEARCH_TOO_SHORT,
            QueryRefused::TooLong => SEARCH_TOO_LONG,
            QueryRefused::ControlCharacters => SEARCH_CONTROL,
        })
    })?;
    let mut set = SourceSet::empty();
    for name in raw.src.as_deref().unwrap_or("").split(',').filter(|s| !s.is_empty()) {
        set = set.with(match name {
            "queue" => Source::Queue,
            "history" => Source::History,
            "youtube" => Source::YouTube,
            "soundcloud" => Source::SoundCloud,
            _ => return Err(BadQuery(SEARCH_UNKNOWN_SOURCE)),
        });
    }
    if set.is_empty() {
        return Err(BadQuery(SEARCH_NO_SOURCES));
    }
    Ok((q, set))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtView { End, Next }

impl From<AtView> for At {
    fn from(a: AtView) -> Self {
        match a {
            AtView::End => At::End,
            AtView::Next => At::Next,
        }
    }
}

/// `POST /g/{id}/add`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRequest {
    pub hit: Uuid,
    pub at: AtView,
}

/// The answer to an add. `Added` carries the view to render.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum AddResult {
    Added { view: QueueView },
    Expired,
    NotAllowed,
    PremiumRequired,
    GameInProgress,
    NotPlaying,
    Failed,
    TooMany,
}
```
(`BadQuery`'s field is `pub`; if `parse_search` needs `BadQuery` to be `Debug`/`PartialEq` for `unwrap_err().0`, compare the `.0` as shown — `unwrap_err` needs `Debug` on the `Ok` type: derive `Debug` on nothing new; `(Query, SourceSet)` is `Debug` already.)
Run: `cargo test -p crack-web wire_test` — expected: pass.

- [ ] **Step 5: Backend + fake — compile-driven**

In `backend.rs`:
```rust
pub use crack_core::music::remote::{AddRefused, At};
pub use crack_core::music::search::{Query, SavedTrack, Source, SourceResult, SourceSet};
use chrono::{DateTime, Utc};
// in trait Backend:
    fn search(
        &self,
        g: GuildId,
        q: &Query,
        sources: SourceSet,
        since: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Vec<(Source, SourceResult)>> + Send;
    fn add(
        &self,
        user: UserId,
        name: &str,
        g: GuildId,
        track: SavedTrack,
        at: At,
    ) -> impl Future<Output = Result<(), AddRefused>> + Send;
```
`LiveBackend` (lib.rs):
```rust
    async fn search(&self, g: GuildId, q: &Query, sources: SourceSet, since: Option<DateTime<Utc>>) -> Vec<(Source, SourceResult)> {
        crack_core::music::search::search(&self.deps.data, g, q, sources, since).await
    }

    async fn add(&self, user: UserId, name: &str, g: GuildId, track: SavedTrack, at: At) -> Result<(), AddRefused> {
        // The echo line is already spawned in core.
        remote::add(
            self.deps.data.clone(), self.deps.http.clone(), self.deps.cache.clone(),
            g, user, name.to_owned(), crack_core::messaging::cards::Via::Dashboard, track, at,
        )
        .await
        .map(|_| ())
    }
```
(`self.deps.data` is `Arc<Data>`; `search` takes `&Data` — pass `&self.deps.data` and let deref coerce, or `self.deps.data.as_ref()`.)

`FakeBackend` (test_support.rs) gains, initialised in `new`:
```rust
    /// What `search` answers.
    pub search_result: Mutex<Vec<(Source, SourceResult)>>,
    /// Every search the routes made: (guild, query text, sources, since).
    pub searches: Mutex<Vec<(GuildId, String, SourceSet, Option<chrono::DateTime<chrono::Utc>>)>>,
    pub add_result: Mutex<Result<(), AddRefused>>,
    /// Every add the routes asked for: (guild, user, name, track, at).
    pub adds: Mutex<Vec<(GuildId, UserId, String, SavedTrack, At)>>,
    /// What `view` becomes once an add succeeds; `None` leaves it as it was.
    pub view_after_add: Mutex<Option<QueueView>>,
```
with `search` pushing to `searches` and returning `search_result.clone()`, and `add` pushing to `adds`, sleeping 1 ms, applying `view_after_add` on `Ok`, returning `add_result`. Defaults: `search_result` empty, `add_result: Ok(())`.

`WebState` (routes.rs) gains:
```rust
    /// Each member's last search results.
    pub hits: Arc<crate::hits::HitMemo>,
    /// Searches, per user; separate from `limiter` so searching never spends a control.
    pub search_limiter: Arc<RateLimit>,
```
(add to `Clone`), filled in `test_support::state` and `lib.rs::start` with `HitMemo::new(hits::HIT_TTL)` and `RateLimit::new(limit::SEARCHES_PER_WINDOW, limit::SEARCH_WINDOW)`. In `limit.rs`:
```rust
/// Searches one user may make in one window, across all servers.
pub const SEARCHES_PER_WINDOW: usize = 5;
pub const SEARCH_WINDOW: Duration = Duration::from_secs(30);
```
Run: `cargo check -p crack-web --all-targets` — expected: clean.

- [ ] **Step 6: Route tests — failing** (first move `playing()` and `controller()` out of `routes.rs`'s test module into `test_support.rs` as `pub fn`s and import them back in `routes.rs`'s tests; then in `find.rs`, `#[cfg(test)] mod route_test`, using `crate::test_support::*`; write `get_search(fake, uri, cookie)` and `post_add(fake, cookie, ctype, origin, body)` helpers modelled on `routes.rs`'s `get` and `post_control`, using the same `app(fake.clone()).oneshot(...)`. **Because the memo lives in `WebState`, a search-then-add test must build the state once and clone the router**: `let app = router(state(fake.clone()));` then `app.clone().oneshot(..)` for each request.)

Tests to write (each one function; exact assertions):

```rust
    const Q: &str = "/g/5/search?q=abba&src=queue,youtube";

    fn found(url: &str) -> Hit {
        Hit { track: SavedTrack { url: url.into(), title: Some("t".into()), ..Default::default() }, detail: HitDetail::Found }
    }

    #[derive(serde::Deserialize)]
    struct SearchAnswer { sources: Vec<SourceAnswer> }
    #[derive(serde::Deserialize)]
    struct SourceAnswer { source: String, status: String, hits: Vec<HitAnswer> }
    #[derive(serde::Deserialize)]
    struct HitAnswer { id: Uuid, url: String }
    #[derive(serde::Deserialize)]
    struct AddAnswer { result: String }

    #[tokio::test]
    async fn a_member_searches_and_gets_ids_per_source() {
        let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
        *fake.search_result.lock().unwrap() = vec![
            (Source::Queue, SourceResult::Hidden),
            (Source::YouTube, SourceResult::Hits(vec![found("https://y/1"), found("https://y/2")])),
        ];
        let r = get_search(fake.clone(), Q, Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let a: SearchAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!((a.sources[0].source.as_str(), a.sources[0].status.as_str()), ("queue", "hidden"));
        assert_eq!((a.sources[1].source.as_str(), a.sources[1].status.as_str()), ("youtube", "ok"));
        assert_eq!(a.sources[1].hits.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), ["https://y/1", "https://y/2"]);
        let (g, text, set, _) = fake.searches.lock().unwrap()[0].clone();
        assert_eq!((g, text.as_str(), set), (GuildId::new(5), "abba", SourceSet::empty().with(Source::Queue).with(Source::YouTube)));
    }

    #[tokio::test]
    async fn search_gates_answer_before_the_backend_searches() {
        // (cookie, membership, uri, want)
        let cases = [
            (None, Membership::Member, Q, StatusCode::UNAUTHORIZED),
            (Some(session(9)), Membership::NotMember, Q, StatusCode::NOT_FOUND),
            (Some(session(9)), Membership::Unknown, Q, StatusCode::SERVICE_UNAVAILABLE),
            (Some(session(9)), Membership::Member, "/g/5/search?q=a&src=queue", StatusCode::BAD_REQUEST),
            (Some(session(9)), Membership::Member, "/g/5/search?q=abba&src=spotify", StatusCode::BAD_REQUEST),
            (Some(session(9)), Membership::Member, "/g/0/search?q=abba&src=queue", StatusCode::NOT_FOUND),
        ];
        for (cookie, m, uri, want) in cases {
            let fake = FakeBackend::new(m, None, QueueView::Idle);
            let r = get_search(fake.clone(), uri, cookie.as_deref()).await;
            assert_eq!(r.status(), want, "{uri} {m:?}");
            assert!(fake.searches.lock().unwrap().is_empty(), "no search for {want}");
        }
    }

    #[tokio::test]
    async fn the_history_window_follows_the_plan() {
        for (plan, windowed) in [(PlanView::Free, true), (PlanView::Premium, false)] {
            let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
            *fake.plan.lock().unwrap() = plan;
            get_search(fake.clone(), Q, Some(&session(9))).await;
            let since = fake.searches.lock().unwrap()[0].3;
            assert_eq!(since.is_some(), windowed, "{plan:?}");
            if let Some(s) = since {
                let age = chrono::Utc::now() - s;
                assert!((age - crack_core::guild::plan::FREE_HISTORY_WINDOW).num_seconds().abs() < 5);
            }
        }
    }

    #[tokio::test]
    async fn searching_is_rate_limited_on_its_own_limiter() {
        let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
        let app = router(state(fake.clone()));
        for _ in 0..crate::limit::SEARCHES_PER_WINDOW {
            assert_eq!(app.clone().oneshot(search_req(Q, &session(9))).await.unwrap().status(), StatusCode::OK);
        }
        assert_eq!(app.clone().oneshot(search_req(Q, &session(9))).await.unwrap().status(), StatusCode::TOO_MANY_REQUESTS);
        // The controls limiter is untouched: a control still goes through.
        let r = app.clone().oneshot(control_req(&session(9))).await.unwrap();
        assert_ne!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn a_hit_found_by_search_is_what_add_queues() {
        let fake = FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing());
        *fake.search_result.lock().unwrap() = vec![(Source::YouTube, SourceResult::Hits(vec![found("https://y/1"), found("https://y/2")]))];
        let app = router(state(fake.clone()));
        let a: SearchAnswer = serde_json::from_str(&body(app.clone().oneshot(search_req(Q, &session(9))).await.unwrap()).await).unwrap();
        let id = a.sources[0].hits[1].id;
        let r = app.clone().oneshot(add_req(&session(9), &format!(r#"{{"hit":"{id}","at":"next"}}"#))).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let ans: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(ans.result, "added");
        let adds = fake.adds.lock().unwrap();
        assert_eq!(adds.len(), 1);
        let (g, user, name, track, at) = &adds[0];
        assert_eq!((*g, *user, name.as_str(), track.url.as_str(), *at), (GuildId::new(5), UserId::new(9), "tester", "https://y/2", At::Next));
    }

    #[tokio::test]
    async fn an_unknown_or_foreign_hit_is_gone() {
        let fake = FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing());
        *fake.search_result.lock().unwrap() = vec![(Source::YouTube, SourceResult::Hits(vec![found("https://y/1")]))];
        let app = router(state(fake.clone()));
        let a: SearchAnswer = serde_json::from_str(&body(app.clone().oneshot(search_req(Q, &session(9))).await.unwrap()).await).unwrap();
        let id = a.sources[0].hits[0].id;
        // Another member replays the id.
        let r = app.clone().oneshot(add_req(&session(10), &format!(r#"{{"hit":"{id}","at":"end"}}"#))).await.unwrap();
        assert_eq!(r.status(), StatusCode::GONE);
        let ans: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(ans.result, "expired");
        assert!(fake.adds.lock().unwrap().is_empty());
    }

    /// A search on `app` as member 9, then the id of its first hit.
    async fn first_hit(app: &axum::Router) -> Uuid {
        let r = app.clone().oneshot(search_req(Q, &session(9))).await.unwrap();
        let a: SearchAnswer = serde_json::from_str(&body(r).await).unwrap();
        a.sources[0].hits[0].id
    }

    fn one_hit(fake: &FakeBackend) {
        *fake.search_result.lock().unwrap() =
            vec![(Source::YouTube, SourceResult::Hits(vec![found("https://y/1")]))];
    }

    fn raw_add(cookie: Option<&str>, ctype: Option<&str>, origin: Option<&str>, text: &str) -> Request<Body> {
        let mut req = Request::post("/g/5/add");
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        if let Some(c) = ctype {
            req = req.header(header::CONTENT_TYPE, c);
        }
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        req.body(Body::from(text.to_owned())).unwrap()
    }

    #[tokio::test]
    async fn add_gates_in_controls_order_and_add_nothing() {
        let json = Some("application/json");
        // (cookie, ctype, origin, body (HIT = the real id), membership, in voice, plan, status, result)
        type Case = (bool, Option<&'static str>, Option<&'static str>, &'static str, Membership, bool, PlanView, StatusCode, Option<&'static str>);
        let ok_body = r#"{"hit":"HIT","at":"end"}"#;
        let cases: Vec<Case> = vec![
            (false, json, Some(ORIGIN), ok_body, Membership::Member, true, PlanView::Premium, StatusCode::UNAUTHORIZED, None),
            (true, Some("text/plain"), Some(ORIGIN), ok_body, Membership::Member, true, PlanView::Premium, StatusCode::UNSUPPORTED_MEDIA_TYPE, None),
            (true, json, Some("https://evil.example"), ok_body, Membership::Member, true, PlanView::Premium, StatusCode::FORBIDDEN, None),
            (true, json, None, ok_body, Membership::Member, true, PlanView::Premium, StatusCode::FORBIDDEN, None),
            (true, json, Some(ORIGIN), r#"{"hit":"HIT","at":"top"}"#, Membership::Member, true, PlanView::Premium, StatusCode::BAD_REQUEST, None),
            (true, json, Some(ORIGIN), r#"{"hit":"HIT","at":"end","url":"https://x"}"#, Membership::Member, true, PlanView::Premium, StatusCode::BAD_REQUEST, None),
            (true, json, Some(ORIGIN), ok_body, Membership::NotMember, true, PlanView::Premium, StatusCode::NOT_FOUND, None),
            (true, json, Some(ORIGIN), ok_body, Membership::Unknown, true, PlanView::Premium, StatusCode::SERVICE_UNAVAILABLE, None),
            (true, json, Some(ORIGIN), ok_body, Membership::Member, false, PlanView::Premium, StatusCode::FORBIDDEN, Some("not_allowed")),
            (true, json, Some(ORIGIN), ok_body, Membership::Member, true, PlanView::Free, StatusCode::FORBIDDEN, Some("premium_required")),
        ];
        for (cookie, ctype, origin, text, membership, in_voice, plan, want, result) in cases {
            let fake = FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing());
            one_hit(&fake);
            let app = router(state(fake.clone()));
            let id = first_hit(&app).await;
            *fake.membership.lock().unwrap() = membership;
            *fake.user_channel.lock().unwrap() = in_voice.then_some(BOT_CHANNEL);
            *fake.plan.lock().unwrap() = plan;
            let s9 = session(9);
            let req = raw_add(cookie.then_some(s9.as_str()), ctype, origin, &text.replace("HIT", &id.to_string()));
            let r = app.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), want, "{ctype:?} {origin:?} {text} {membership:?} {in_voice} {plan:?}");
            if let Some(result) = result {
                let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
                assert_eq!(a.result, result);
            }
            assert!(fake.adds.lock().unwrap().is_empty(), "nothing added for {want}");
        }
    }

    #[tokio::test]
    async fn add_refusals_map_to_their_answers() {
        for (refused, status, result) in [
            (AddRefused::NotPlaying, StatusCode::CONFLICT, "not_playing"),
            (AddRefused::GameInProgress, StatusCode::LOCKED, "game_in_progress"),
            (AddRefused::Failed, StatusCode::INTERNAL_SERVER_ERROR, "failed"),
        ] {
            let fake = FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing());
            one_hit(&fake);
            *fake.add_result.lock().unwrap() = Err(refused);
            let app = router(state(fake.clone()));
            let id = first_hit(&app).await;
            let r = app.clone().oneshot(add_req(&session(9), &format!(r#"{{"hit":"{id}","at":"end"}}"#))).await.unwrap();
            assert_eq!(r.status(), status, "{refused:?}");
            let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, result);
        }
    }
```
Helpers `search_req(uri, cookie) -> Request<Body>`, `add_req(cookie, body) -> Request<Body>` (JSON content type, `Origin: ORIGIN`), `control_req(cookie)` (a valid Skip to `/g/5/control`) live in the same test module.

Run: `cargo test -p crack-web route_test` — expected: fail (routes 404 / handlers missing).

- [ ] **Step 7: The handlers** (in `find.rs`)

```rust
use crate::{
    access::{decide, Access},
    backend::Backend,
    hits::HitMemo,
    routes::{error_json, is_json, not_found, parse_guild, plan_or_free, unavailable, user_id, Session, WebState},
    view::PlanView,
};
use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use crack_core::guild::plan::Plan;
use std::time::Instant;

fn history_since(plan: PlanView) -> Option<chrono::DateTime<chrono::Utc>> {
    let plan = match plan {
        PlanView::Free => Plan::Free,
        PlanView::Premium => Plan::Premium,
    };
    plan.history_floor(chrono::Utc::now())
}

/// Store each source's hits in the memo and build the answer.
fn answer_search(memo: &HitMemo, user: UserId, g: GuildId, results: Vec<(Source, SourceResult)>) -> SearchResult {
    let tracks: Vec<SavedTrack> = results
        .iter()
        .flat_map(|(_, r)| match r {
            SourceResult::Hits(h) => h.iter().map(|x| x.track.clone()).collect(),
            _ => Vec::new(),
        })
        .collect();
    let mut ids = memo.store(user, g, tracks, Instant::now()).into_iter();
    let sources = results
        .into_iter()
        .map(|(source, r)| {
            let (status, hits) = match r {
                SourceResult::Hits(h) => (StatusView::Ok, h.iter().map(|x| hit_view(ids.next().expect("one id per hit"), x)).collect()),
                SourceResult::Hidden => (StatusView::Hidden, Vec::new()),
                SourceResult::NoDatabase => (StatusView::NoDatabase, Vec::new()),
                SourceResult::Failed => (StatusView::Failed, Vec::new()),
                SourceResult::TimedOut => (StatusView::TimedOut, Vec::new()),
            };
            SourceView { source: source.into(), status, hits }
        })
        .collect();
    SearchResult { sources }
}

/// Session, membership, query, the search limiter, the plan's window.
pub(crate) async fn search<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return StatusCode::NOT_FOUND.into_response(),
        Access::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Access::View | Access::Control => {},
    }
    let (q, sources) = match parse_search(query.as_deref()) {
        Ok(p) => p,
        Err(bad) => return error_json(StatusCode::BAD_REQUEST, bad.0),
    };
    if !s.search_limiter.allow(user, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let since = history_since(plan_or_free(s.backend.as_ref(), g).await);
    let results = s.backend.search(g, &q, sources, since).await;
    Json(answer_search(&s.hits, user, g, results)).into_response()
}

fn add_answer(status: StatusCode, result: AddResult) -> Response {
    (status, Json(result)).into_response()
}

/// The same gates as a control, then the memo, then the add.
pub(crate) async fn add<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, name)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<AddRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return add_answer(StatusCode::FORBIDDEN, AddResult::NotAllowed),
        Access::Control => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return add_answer(StatusCode::TOO_MANY_REQUESTS, AddResult::TooMany);
    }
    if s.backend.plan(g).await != PlanView::Premium {
        return add_answer(StatusCode::FORBIDDEN, AddResult::PremiumRequired);
    }
    let Some(track) = s.hits.get(user, g, req.hit, Instant::now()) else {
        return add_answer(StatusCode::GONE, AddResult::Expired);
    };
    let at = At::from(req.at);
    match s.backend.add(user, &name, g, track, at).await {
        Ok(()) => {
            tracing::info!(guild = %g, user = %user, at = ?at, "dashboard add");
            s.hub.refresh(g).await;
            let view = s.backend.view(g).await;
            add_answer(StatusCode::OK, AddResult::Added { view })
        },
        Err(AddRefused::NotPlaying) => add_answer(StatusCode::CONFLICT, AddResult::NotPlaying),
        Err(AddRefused::GameInProgress) => add_answer(StatusCode::LOCKED, AddResult::GameInProgress),
        Err(AddRefused::Failed) => add_answer(StatusCode::INTERNAL_SERVER_ERROR, AddResult::Failed),
    }
}
```
In `routes.rs`: make `Session`, `user_id`, `is_json`, `error_json`, `plan_or_free` `pub(crate)` (`parse_guild`, `not_found`, `unavailable` already are), and register in the `per_user` router:
```rust
        .route("/g/{guild}/search", get(crate::find::search::<B>))
        .route("/g/{guild}/add", post(crate::find::add::<B>))
```
Add `pub mod find; pub mod hits;` to `lib.rs`.

- [ ] **Step 8: Run all crack-web tests and clippy**

Run: `cargo test -p crack-web && cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
Expected: pass, clean.

- [ ] **Step 9: Sabotage, record, commit**

Sabotages: memo `get` ignores the guild; `store` appends instead of replacing; search uses `s.limiter` instead of `s.search_limiter`; drop the premium check in `add`; skip the Origin check; `history_since` returns `None` for Free; `answer_search` assigns ids out of order (e.g. reversed).

```bash
cargo +nightly fmt --all
git add crack-web/src/hits.rs crack-web/src/find.rs crack-web/src/lib.rs crack-web/src/backend.rs crack-web/src/routes.rs crack-web/src/limit.rs crack-web/src/test_support.rs crack-core/src/messaging/messages.rs
git commit -m "feat(web): GET /g/{g}/search and POST /g/{g}/add over a per-member hit memo

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 8: The page — Find a track

**Files:**
- Modify: `crack-web/src/page.rs` (`queue_page` markup; `QUEUE_SCRIPTS`; a page test)
- Modify: `crack-web/src/routes.rs` (`asset()` serves `find.js`)
- Create: `crack-web/assets/find.js`
- Modify: `crack-web/assets/app.js` (expose the live state)
- Modify: `crack-web/assets/app.css`

**Interfaces:**
- Consumes: Task 7's routes and wire format (`sources[].source|status|hits[]`, `hits[].id|title|artist|duration_secs|url|detail{kind,…}`, `AddResult.result`).
- Produces: `window.crackDash = { state(), onState(fn), settle(body), say(text) }` from `app.js`.

- [ ] **Step 1: Failing page test** in `page.rs`'s tests

```rust
    #[test]
    fn the_queue_page_carries_the_find_panel_and_its_script() {
        let view = QueueView::Idle;
        let state = PageState { view: &view, can_control: false, plan: PlanView::Free };
        let html = queue_page("G", GuildId::new(5), &state, false);
        for needle in [
            "<form id=\"find\"",
            "<input type=\"search\" id=\"find-q\" name=\"q\" minlength=\"2\" maxlength=\"100\"",
            "id=\"find-src\"",
            "value=\"queue\"", "value=\"history\"", "value=\"youtube\"", "value=\"soundcloud\"",
            "<div id=\"results\"",
            "<script src=\"/assets/find.js\"></script>",
        ] {
            assert!(html.contains(needle), "missing {needle}: {html}");
        }
        // find.js runs after app.js, which it reads the live state from.
        assert!(html.find("app.js").unwrap() < html.find("find.js").unwrap());
    }
```
And in `routes.rs`'s tests, extend the existing asset test (find it with `grep -n '"/assets/' crack-web/src/routes.rs`) or add:
```rust
    #[tokio::test]
    async fn find_js_is_served_as_javascript() {
        let r = get(member_viewing(), "/assets/find.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    }
```
Run: `cargo test -p crack-web find_panel find_js` — expected: fail.

- [ ] **Step 2: Markup, asset route, script list**

`page.rs`: `const QUEUE_SCRIPTS: &[&str] = &["sortable.min.js", "app.js", "find.js"];` — the picker page also loads these; `find.js` must return early when `#find` is absent. In `queue_page`, insert after the `voice-controls` paragraph and before `<div id="now">`:
```rust
<form id=\"find\" role=\"search\" autocomplete=\"off\">\
<input type=\"search\" id=\"find-q\" name=\"q\" minlength=\"2\" maxlength=\"100\" \
placeholder=\"Find a track\" aria-label=\"Find a track\">\
<button type=\"submit\" id=\"find-go\">Search</button>\
<fieldset id=\"find-src\"><legend>Search in</legend>\
<label class=\"chip\"><input type=\"checkbox\" value=\"queue\" checked> Queue</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"history\" checked> History</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"youtube\" checked> YouTube</label>\
<label class=\"chip\"><input type=\"checkbox\" value=\"soundcloud\" checked> SoundCloud</label>\
</fieldset></form><div id=\"results\" aria-live=\"polite\"></div>\
```
`routes.rs` `asset()`: add
```rust
        "find.js" => (
            include_bytes!("../assets/find.js"),
            "text/javascript; charset=utf-8",
        ),
```

- [ ] **Step 3: `app.js` exposes the live state**

Inside the IIFE, after `let state = JSON.parse(...)` and the `settle` function are defined (place it just before the final `render();`):
```js
  // find.js reads the live state (can_control, plan, view) and hands add
  // answers back to be drawn: one renderer, one state.
  const listeners = [];
  window.crackDash = {
    state: () => state,
    onState: (fn) => { listeners.push(fn); },
    settle: (body) => settle(body),
    say,
  };
```
and at the end of `render()` add `for (const fn of listeners) fn(state);`. (`listeners` must be declared before the first `render()` call; the final `render();` at the bottom of the file comes after this block.)

- [ ] **Step 4: `find.js`** (`crack-web/assets/find.js`)

```js
// Find a track: search the queue, this server's history, YouTube and
// SoundCloud, then add a hit or play it next. Third-party text (titles,
// artists) only ever reaches the page through textContent and setAttribute.
(() => {
  "use strict";
  const form = document.getElementById("find");
  const dash = window.crackDash;
  if (!form || !dash) return; // not the queue page
  const root = document.getElementById("dash");
  const guild = root.dataset.guild;
  const input = document.getElementById("find-q");
  const go = document.getElementById("find-go");
  const results = document.getElementById("results");
  const boxes = Array.from(document.querySelectorAll("#find-src input[type=checkbox]"));
  const KEY = "crack.find.sources";
  const NAMES = { queue: "Queue", history: "History", youtube: "YouTube", soundcloud: "SoundCloud" };
  const STATUS = {
    hidden: "Hidden while a Guilty Pleasure game is on.",
    no_database: "This bot keeps no history.",
    failed: "did not answer.",
    timed_out: "took too long.",
  };

  try {
    const saved = JSON.parse(localStorage.getItem(KEY) || "null");
    if (Array.isArray(saved)) for (const b of boxes) b.checked = saved.includes(b.value);
  } catch (_) { /* private window or blocked storage: keep the defaults */ }
  for (const b of boxes) {
    b.addEventListener("change", () => {
      try { localStorage.setItem(KEY, JSON.stringify(boxes.filter((x) => x.checked).map((x) => x.value))); } catch (_) {}
    });
  }

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function duration(secs) {
    if (secs === null || secs === undefined) return "";
    const m = Math.floor(secs / 60);
    return `${m}:${String(secs % 60).padStart(2, "0")}`;
  }

  function ago(unix) {
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
    if (s < 3600) return `${Math.max(1, Math.floor(s / 60))} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return `${Math.floor(s / 86400)} d ago`;
  }

  function detail(d) {
    if (d.kind === "queue") return d.position === 0 ? "now playing" : `#${d.position} in queue`;
    if (d.kind === "history") return `queued ${d.plays}× · ${ago(d.last_unix)}`;
    return "";
  }

  // Why the add buttons are off, from the live state; null when they are on.
  function addBlocked() {
    const s = dash.state();
    if (s.view && s.view.state === "hidden") return "A Guilty Pleasure game is on — the queue is locked.";
    if (s.plan !== "premium") return "Adding from the dashboard is a premium feature.";
    if (!s.can_control) return "Join the bot's voice channel to add tracks.";
    return null;
  }

  function refreshButtons() {
    const blocked = addBlocked();
    for (const b of results.querySelectorAll("button.add")) {
      if (b.dataset.busy !== "1") b.disabled = !!blocked;
      if (blocked) b.title = blocked; else b.removeAttribute("title");
    }
  }
  dash.onState(refreshButtons);

  async function add(row, id, at, title) {
    const buttons = row.querySelectorAll("button.add");
    for (const b of buttons) { b.disabled = true; b.dataset.busy = "1"; }
    let body = null;
    let status = 0;
    try {
      const res = await fetch(`/g/${guild}/add`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ hit: id, at }),
      });
      status = res.status;
      body = await res.json().catch(() => null);
    } catch (_) {
      dash.say("Could not reach the server. Nothing was added.");
    } finally {
      for (const b of buttons) delete b.dataset.busy;
    }
    const result = body && body.result;
    if (result === "added") dash.say(at === "next" ? `Up next: ${title}.` : `Added ${title}.`);
    else if (result === "expired") { dash.say("Those results expired — search again."); results.replaceChildren(); }
    else if (result === "not_allowed") dash.say("Join the bot's voice channel to add tracks.");
    else if (result === "premium_required") dash.say("Adding from the dashboard is a premium feature.");
    else if (result === "game_in_progress") dash.say("A Guilty Pleasure game is on — the queue is locked.");
    else if (result === "not_playing") dash.say("The bot is not in a voice channel.");
    else if (result === "too_many") dash.say("Slow down a little.");
    else if (result === "failed") dash.say("That track could not be queued.");
    else if (status) dash.say(`That did not work (${status}).`);
    if (body && body.view) dash.settle(body);
    refreshButtons();
  }

  function hitRow(h) {
    const li = el("li", "track hit");
    const title = el("a", "title", h.title);
    title.href = h.url; // the server sends http(s) only
    title.rel = "noopener noreferrer";
    title.target = "_blank";
    li.appendChild(title);
    li.appendChild(el("span", "meta", [h.artist, duration(h.duration_secs), detail(h.detail)].filter(Boolean).join(" · ")));
    for (const [at, label, aria] of [["end", "＋ Add", "Add"], ["next", "⏭ Next", "Play next:"]]) {
      const b = el("button", "add", label);
      b.type = "button";
      b.setAttribute("aria-label", `${aria} ${h.title}`);
      b.addEventListener("click", () => add(li, h.id, at, h.title));
      li.appendChild(b);
    }
    return li;
  }

  function draw(answer) {
    results.replaceChildren();
    const free = dash.state().plan !== "premium";
    for (const s of answer.sources) {
      const block = el("section", "source");
      block.appendChild(el("h2", null, NAMES[s.source] || s.source));
      if (s.status === "ok" && s.hits.length === 0) block.appendChild(el("p", "empty", "No matches."));
      else if (s.status === "failed" || s.status === "timed_out") block.appendChild(el("p", "empty", `${NAMES[s.source]} ${STATUS[s.status]}`));
      else if (s.status !== "ok") block.appendChild(el("p", "empty", STATUS[s.status] || s.status));
      if (s.source === "history" && free) block.appendChild(el("p", "empty", "Free servers search the last 24 hours."));
      if (s.hits.length) {
        const ol = el("ol", "hits");
        for (const h of s.hits) ol.appendChild(hitRow(h));
        block.appendChild(ol);
      }
      results.appendChild(block);
    }
    refreshButtons();
  }

  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const q = input.value.trim();
    const src = boxes.filter((b) => b.checked).map((b) => b.value);
    if (q.length < 2) { dash.say("Type at least two characters to search."); return; }
    if (!src.length) { dash.say("Pick at least one place to search."); return; }
    go.disabled = true;
    results.replaceChildren(el("p", "empty", "Searching…"));
    try {
      const params = new URLSearchParams({ q, src: src.join(",") });
      const res = await fetch(`/g/${guild}/search?${params}`, { credentials: "same-origin" });
      const body = await res.json().catch(() => null);
      if (res.ok && body) { dash.say(""); draw(body); }
      else if (res.status === 429) { results.replaceChildren(); dash.say("Slow down a little."); }
      else { results.replaceChildren(); dash.say((body && body.error) || `That did not work (${res.status}).`); }
    } catch (_) {
      results.replaceChildren();
      dash.say("Could not reach the server.");
    } finally {
      go.disabled = false;
    }
  });
})();
```

- [ ] **Step 5: CSS** (append to `app.css`)

```css
#find { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 0 0 12px; }
#find-q { flex: 1 1 14rem; min-width: 0; padding: 6px 10px; font: inherit; color: var(--fg);
          background: var(--card); border: 1px solid var(--line); border-radius: 6px; }
#find-go, .hit .add { background: none; border: 1px solid var(--line); color: var(--muted);
                      border-radius: 6px; padding: 4px 10px; cursor: pointer; font: inherit; }
#find-go[disabled], .hit .add[disabled] { opacity: 0.45; cursor: not-allowed; }
#find-src { display: flex; flex-wrap: wrap; gap: 6px; border: 0; padding: 0; margin: 0; flex-basis: 100%; }
#find-src legend { color: var(--muted); font-size: 0.85rem; padding: 0; margin-right: 6px; float: left; }
#results .source h2 { margin: 12px 0 6px; }
#results ol.hits { list-style: none; padding: 0; margin: 0; }
.hit .add { padding: 0 8px; font-size: 0.85rem; line-height: 1.6; }
.hit .add:first-of-type { margin-left: auto; }
```

- [ ] **Step 6: Run tests; drive it in a browser**

Run: `cargo test -p crack-web`
Expected: pass.
Then, if a TuneTitan or local run is available to you, load the queue page and check by hand: search each source; buttons disabled on a free server and outside voice; a double click on Add sends one request (watch the network panel); "expired" after a second search clears the old results. If no run is available, say so in your report; the owner checks it on TuneTitan.

- [ ] **Step 7: Sabotage, record, commit**

Sabotages: drop `find.js` from `QUEUE_SCRIPTS`; serve `find.js` with `text/plain`; reorder scripts so `find.js` precedes `app.js`.

```bash
cargo +nightly fmt --all
git add crack-web/src/page.rs crack-web/src/routes.rs crack-web/assets/find.js crack-web/assets/app.js crack-web/assets/app.css
git commit -m "feat(web): the Find a track panel -- search, add and play next on the queue page

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 9: Release preparation

**Files:**
- Modify: `Cargo.toml` (`[workspace.package] version = "0.27.0"`), `Cargo.lock` (via `cargo check`)
- Modify: `CHANGELOG.md` (new `0.27.0` entry at the top, in the file's existing style)
- Modify: `docs/web-dashboard.md` (a "Search and add" section: sources, who may add, the 10-minute results, the free 24-hour history window, the echo)

**Interfaces:** none.

- [ ] **Step 1: Bump and document**

Edit the version line; run `cargo check --workspace` so `Cargo.lock` follows. Write the CHANGELOG entry (features: dashboard search over queue/history/YouTube/SoundCloud; Add and Play next for premium servers from the bot's voice channel; `ops::add`; dashboard adds now reach `play_log`). Write the docs section.

- [ ] **Step 2: Full verification**

```bash
cargo +nightly fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
cargo check -p crack-core --features crack-bf,crack-osint --locked
SQLX_OFFLINE=true cargo check --workspace --all-targets
DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres \
  cargo test --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests
```
Expected: all clean; tests pass except the known #523 live-network flakes (if one fails, read it; rerun once; report it). Record the pass/fail/ignored counts.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md docs/web-dashboard.md
git commit -m "v0.27.0: search the queue, history, YouTube and SoundCloud from the dashboard, and add or play next

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
