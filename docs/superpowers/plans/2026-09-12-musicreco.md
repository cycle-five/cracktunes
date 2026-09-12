# crack-musicreco Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace cracktunes' dead Spotify-recommendation autoplay with a
provider-agnostic `crack-musicreco` crate backed by musicatlas, ReccoBeats and
MusicBrainz, with fallback between them.

**Architecture:** Two traits — `SeedResolver` (messy title → checked seed) and
`Recommender` (seed → candidate tracks) — behind a `MusicReco` orchestrator that
tries providers in order. Providers are NOT interchangeable: only musicatlas
returns a directly playable YouTube id, so playability is modelled explicitly as
`Playable::{YouTubeId, SearchQuery}`. A Postgres cache with negative entries and
a per-provider budget table keeps the metered provider inside its free tier.

**Tech Stack:** Rust 2021, reqwest, serde, thiserror, sqlx 0.8 (Postgres),
tokio. Structural precedent: `crack-sleevenote`.

**Spec:** `docs/superpowers/specs/2026-09-12-musicreco-design.md` — read it
first; every API contract in it was measured against the live services and
several facts contradict the vendors' own docs.

## Global Constraints

- Workspace members are all at version `0.9.7`; the new crate joins at `0.9.7`.
- Every commit ends with exactly this trailer and no other `Co-Authored-By`:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- **Typed serde only.** No `serde_json::json!` or `serde_json::Value` for data
  we own. `Value` is acceptable only for opaque passthrough.
- **Never build container images locally** — container DNS is broken. Build on
  `pve-staging` if an image is needed.
- **musicatlas: key the error taxonomy on the `success` field, never on the HTTP
  status.** Logical failures arrive as HTTP 200.
- **Every HTTP client must set an explicit User-Agent.** musicatlas returns 403
  with a bad-key body for `Python-urllib`; MusicBrainz requires a descriptive UA
  with a contact address.
- **MusicBrainz: hard 1 request/second.** Exceeding it blocks the estate's IP,
  not just one guild.
- Tests that assert caching, budgets or retries must assert on **request
  counts**, not only returned values — a cache that silently still calls returns
  the right answer.
- Run `SQLX_OFFLINE=true cargo check --workspace` before any commit that adds a
  `query!` macro; CI's `Docker` job is skipped on PRs and will not catch it.

---

### Task 1: Scaffold the crate and its domain types

**Files:**
- Create: `crack-musicreco/Cargo.toml`
- Create: `crack-musicreco/src/lib.rs`
- Create: `crack-musicreco/src/model.rs`
- Create: `crack-musicreco/src/error.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: `RawTrack`, `Seed`, `Recommendation`, `Playable`, `Error`, `Result<T>`
  — every later task consumes these.

- [ ] **Step 1: Add the member to the workspace**

In the root `Cargo.toml`, add `"crack-musicreco",` to `[workspace] members`,
after `"crack-sleevenote",`.

- [ ] **Step 2: Write `crack-musicreco/Cargo.toml`**

```toml
[package]
name = "crack-musicreco"
version = "0.9.7"
edition = "2021"
authors = ["Cycle Five <cycle.five@proton.me>"]
publish = true
license = "MIT"
description = "Provider-agnostic music recommendation: musicatlas, ReccoBeats, MusicBrainz."
keywords = ["music", "discord", "bot", "crack", "tunes"]
categories = ["multimedia::audio"]
homepage = "https://cracktun.es/"
repository = "https://github.com/cycle-five/cracktunes"
workspace = "../"

[dependencies]
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "1.0.69"
url = "2.5"
reqwest = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
tokio = { workspace = true }
```

- [ ] **Step 3: Write the failing test for `Playable`**

Create `crack-musicreco/src/model.rs` with only this test module at first:

```rust
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
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p crack-musicreco`
Expected: FAIL — `Recommendation` not defined.

- [ ] **Step 5: Implement the domain types**

Prepend to `crack-musicreco/src/model.rs`:

```rust
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
```

- [ ] **Step 6: Write the error taxonomy**

Create `crack-musicreco/src/error.rs`:

```rust
//! One variant per distinct failure. They do not collapse: "this seed is not a
//! real track" is permanent, "we are rate limited" is temporary, and "the key
//! is bad" needs an operator. A caller that cannot tell them apart retries the
//! unretryable and gives up on the recoverable.

use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// The provider says this seed is not a released track. Permanent for this
    /// seed; negative-cache it rather than retrying.
    #[error("`{artist} - {title}` is not a track {provider} knows: {message}")]
    NotATrack { provider: &'static str, artist: String, title: String, message: String },

    /// Credential rejected. An operator must act; do not retry.
    #[error("{provider} rejected the credential: {message}")]
    InvalidKey { provider: &'static str, message: String },

    /// Rate limited. `retry_after` is the provider's own figure when it gave one.
    #[error("{provider} rate limited us{}", .retry_after.map(|d| format!(", retry after {}s", d.as_secs())).unwrap_or_default())]
    RateLimited { provider: &'static str, retry_after: Option<std::time::Duration> },

    /// The locally-counted daily budget for a metered provider is spent.
    #[error("{provider} daily budget of {budget} calls is spent")]
    BudgetExhausted { provider: &'static str, budget: u32 },

    #[error("{provider} transport error: {source}")]
    Transport { provider: &'static str, #[source] source: reqwest::Error },

    #[error("{provider} sent a body this client could not read: {message}")]
    UnexpectedBody { provider: &'static str, message: String },

    #[error("configuration: {0}")]
    Config(String),
}

impl Error {
    /// Whether trying the SAME provider again could plausibly succeed.
    ///
    /// 🪤 Deliberately NOT a general `is_retryable()` covering every variant —
    /// it answers one narrow question the orchestrator asks. `NotATrack` is
    /// false: the seed is wrong, not the moment.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Error::Transport { .. } | Error::RateLimited { .. })
    }
}
```

- [ ] **Step 7: Write `lib.rs`**

```rust
//! Provider-agnostic music recommendation.
//!
//! Autoplay needs "given the track that just ended, what plays next". No single
//! service answers that reliably, so this crate holds several behind two traits
//! and falls back between them.
//!
//! * [`SeedResolver`] — a messy YouTube title becomes a checked `(artist, title)`.
//! * [`Recommender`] — a seed becomes candidate tracks.
//!
//! 🔑 The providers are not interchangeable. Only musicatlas returns a directly
//! playable YouTube id, so [`model::Playable`] makes that difference explicit
//! instead of hiding it behind a lowest common denominator.

pub mod error;
pub mod model;

pub use error::{Error, Result};
pub use model::{Playable, RawTrack, Recommendation, Seed};
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p crack-musicreco`
Expected: PASS, 2 tests.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crack-musicreco
git commit -m "feat(musicreco): scaffold the crate and its domain types

Playable is an enum rather than an optional url because the providers genuinely
differ: musicatlas returns a YouTube id and ReccoBeats never does. Flattening
them would let a caller treat a ReccoBeats result as directly playable.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: Title parsing — the offline seed resolver

**Files:**
- Create: `crack-musicreco/src/resolver/mod.rs`
- Create: `crack-musicreco/src/resolver/title_parse.rs`
- Modify: `crack-musicreco/src/lib.rs`

**Interfaces:**
- Consumes: `RawTrack`, `Seed`, `Result` from Task 1.
- Produces: `trait SeedResolver`, `TitleParseResolver::new()`.

- [ ] **Step 1: Write the failing table-driven test**

In `crack-musicreco/src/resolver/title_parse.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn raw(title: &str) -> RawTrack {
        RawTrack { title: title.into(), artist: None, uploader: None }
    }

    #[tokio::test]
    async fn parses_the_titles_we_measured() {
        // Every row here came from running yt-dlp against a real video.
        let cases = [
            ("Guns N' Roses - Sweet Child O' Mine (Official Music Video)",
             Some(("Guns N' Roses", "Sweet Child O' Mine"))),
            // 🪤 EN DASH, not a hyphen. A splitter that only knows '-' silently
            // yields no seed for this one.
            ("Queen – Bohemian Rhapsody (Official Video Remastered)",
             Some(("Queen", "Bohemian Rhapsody"))),
            ("Daft Punk - Get Lucky (Official Audio) ft. Pharrell Williams",
             Some(("Daft Punk", "Get Lucky"))),
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
        let seed = r.resolve(&RawTrack {
            title: "Anything At All".into(),
            artist: Some("Real Artist".into()),
            uploader: None,
        }).await.unwrap().expect("explicit artist is a seed");
        assert_eq!(seed.artist, "Real Artist");
        assert_eq!(seed.title, "Anything At All");
        assert_eq!(seed.confidence, 100, "a supplied artist is not a guess");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-musicreco`
Expected: FAIL — `TitleParseResolver` not defined.

- [ ] **Step 3: Define the trait**

Create `crack-musicreco/src/resolver/mod.rs`:

```rust
use crate::{RawTrack, Result, Seed};
use async_trait::async_trait;

pub mod title_parse;
pub use title_parse::TitleParseResolver;

/// Turn what the bot knows about a finished track into a seed worth spending a
/// metered call on. `Ok(None)` means "no usable seed" and is NOT an error —
/// it is the correct, free outcome for a title with no artist in it.
#[async_trait]
pub trait SeedResolver: Send + Sync {
    fn name(&self) -> &'static str;
    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>>;
}
```

- [ ] **Step 4: Implement the parser**

Prepend to `crack-musicreco/src/resolver/title_parse.rs`:

```rust
use crate::{RawTrack, Result, Seed};
use async_trait::async_trait;

/// Separators seen in real titles, longest-first so " — " is tried before " - ".
///
/// 🪤 The second and third are EN DASH and EM DASH, not hyphens. A measured
/// Queen title uses the en dash; matching only '-' loses it silently.
const SEPARATORS: &[&str] = &[" — ", " – ", " - ", " | "];

/// Bracketed suffixes that are packaging, not part of a track name.
const NOISE_WORDS: &[&str] = &[
    "official", "video", "audio", "lyric", "remaster", "hd", "4k", "mv", "visualizer",
];

/// Strip "(Official Music Video)" / "[HD]" style suffixes, then a trailing
/// "ft. …" / "feat. …" clause.
fn clean_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut buf = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' => { depth += 1; buf.clear(); },
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
    pub fn new() -> Self { Self }
}

#[async_trait]
impl crate::resolver::SeedResolver for TitleParseResolver {
    fn name(&self) -> &'static str { "title-parse" }

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
```

- [ ] **Step 5: Export from lib.rs**

Add to `crack-musicreco/src/lib.rs`:

```rust
pub mod resolver;
pub use resolver::{SeedResolver, TitleParseResolver};
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p crack-musicreco`
Expected: PASS, 4 tests.

- [ ] **Step 7: Commit**

```bash
git add crack-musicreco
git commit -m "feat(musicreco): parse a seed out of a YouTube title

yt-dlp supplies neither artist nor track for ordinary music videos -- measured
NA/NA on three real ones -- so parsing the title is the primary path, not a
fallback. Separators include EN and EM dashes: a measured Queen title uses the
en dash, and matching only a hyphen loses it silently.

No separator means no seed, which means no metered call. Guessing costs quota on
a request already known not to match.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: musicatlas recommender

**Files:**
- Create: `crack-musicreco/src/provider/mod.rs`
- Create: `crack-musicreco/src/provider/musicatlas.rs`
- Modify: `crack-musicreco/src/lib.rs`

**Interfaces:**
- Consumes: `Seed`, `Recommendation`, `Playable`, `Error` (Task 1).
- Produces: `trait Recommender`, `MusicAtlas::new(api_key, http)`,
  `MusicAtlas::with_base_url(..)` for tests.

**Measured contract (do not re-derive):** `POST
https://musicatlas.ai/api/similar_tracks`, `Authorization: Bearer <key>`, body
`{"artist","track"}`. Success is `{"success":true,"matches":[20 items]}`; each
match has `artist`, `title`, `platform_ids.youtube`. **Logical failures arrive
as HTTP 200 with `success:false`.** A bad key is 403. **A missing or default
User-Agent is 403 with the same body as a bad key.**

- [ ] **Step 1: Write the failing tests**

In `crack-musicreco/src/provider/musicatlas.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves canned bodies in order; returns base url and a request counter.
    /// Counting REQUESTS is the point: a provider that silently skips or
    /// double-calls still returns a plausible value.
    async fn serve(bodies: Vec<(u16, &'static str)>) -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let n = served.fetch_add(1, Ordering::SeqCst);
                let (code, body) = bodies.get(n).copied().unwrap_or((200, r#"{"success":true,"matches":[]}"#));
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let r = format!("HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        (format!("http://{addr}"), hits)
    }

    fn seed() -> Seed {
        Seed { artist: "Queen".into(), title: "Bohemian Rhapsody".into(), mbid: None, confidence: 100 }
    }

    const OK: (u16, &str) = (200, r#"{"success":true,"matches":[
        {"artist":"A","title":"T","platform_ids":{"youtube":"vid1","spotify":"sp1"}},
        {"artist":"B","title":"U","platform_ids":{"spotify":"sp2"}}]}"#);

    #[tokio::test]
    async fn maps_matches_and_keeps_only_playable_ones() {
        let (base, _) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let out = p.recommend(&seed(), 10).await.unwrap();
        assert_eq!(out.len(), 1, "the match with no youtube id is dropped");
        assert_eq!(out[0].youtube_id(), Some("vid1"));
        assert_eq!(out[0].source, "musicatlas");
    }

    /// 🪤 THE TRAP. A status-only client reads this as success.
    #[tokio::test]
    async fn success_false_on_http_200_is_a_not_a_track_error() {
        let (base, _) = serve(vec![(200, r#"{"success":false,"error":"That doesn't appear to be a released track."}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let err = p.recommend(&seed(), 10).await.expect_err("must not read as success");
        assert!(matches!(err, Error::NotATrack { .. }), "got {err}");
        assert!(!err.is_transient(), "a bad seed is permanent, not a flake");
    }

    #[tokio::test]
    async fn a_403_is_an_invalid_key_and_is_not_transient() {
        let (base, _) = serve(vec![(403, r#"{"error":"Invalid or unconfirmed API key"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let err = p.recommend(&seed(), 10).await.expect_err("403");
        assert!(matches!(err, Error::InvalidKey { .. }), "got {err}");
        assert!(!err.is_transient());
    }

    #[tokio::test]
    async fn want_caps_the_returned_count() {
        let (base, _) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base);
        assert_eq!(p.recommend(&seed(), 0).await.unwrap().len(), 0);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-musicreco musicatlas`
Expected: FAIL — `MusicAtlas` not defined.

- [ ] **Step 3: Define the `Recommender` trait**

Create `crack-musicreco/src/provider/mod.rs`:

```rust
use crate::{Recommendation, Result, Seed};
use async_trait::async_trait;

pub mod musicatlas;
pub mod reccobeats;
pub use musicatlas::MusicAtlas;
pub use reccobeats::ReccoBeats;

/// Turn a seed into candidate next-tracks. An empty `Ok(vec![])` means "this
/// provider had nothing", which the orchestrator treats as "try the next one" —
/// distinct from an `Err`, which may disable the provider.
#[async_trait]
pub trait Recommender: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether calls cost a finite, purchased quota.
    ///
    /// 🔑 Only metered providers are gated by the seed-confidence floor. A free
    /// provider must still be tried on a shaky seed -- otherwise a MusicBrainz
    /// outage leaves every seed at the bare-parse score of 50 and autoplay is
    /// dead, which is exactly the single-point-of-failure this crate exists to
    /// remove.
    fn is_metered(&self) -> bool { false }

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>>;
}
```

- [ ] **Step 4: Implement musicatlas**

Prepend to `crack-musicreco/src/provider/musicatlas.rs`:

```rust
use crate::{Error, Playable, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const NAME: &str = "musicatlas";
pub const DEFAULT_BASE_URL: &str = "https://musicatlas.ai";
/// 🪤 MANDATORY. The API answers a default/absent User-Agent with 403 and the
/// SAME body as a bad key, so omitting this looks exactly like a bad credential.
pub const USER_AGENT: &str = concat!("cracktunes/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Serialize)]
struct Request<'a> { artist: &'a str, track: &'a str }

/// 🔑 `success` is the discriminator, NOT the HTTP status: logical failures
/// arrive as 200.
#[derive(Debug, Deserialize)]
struct Response {
    success: bool,
    #[serde(default)]
    matches: Vec<Match>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Match { artist: String, title: String, #[serde(default)] platform_ids: PlatformIds }

#[derive(Debug, Default, Deserialize)]
struct PlatformIds { #[serde(default)] youtube: Option<String> }

#[derive(Debug, Clone)]
pub struct MusicAtlas { api_key: String, base_url: String, http: reqwest::Client }

impl MusicAtlas {
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Ok(Self::with_base_url(api_key, DEFAULT_BASE_URL))
    }

    #[must_use]
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl crate::provider::Recommender for MusicAtlas {
    fn name(&self) -> &'static str { NAME }
    /// 100 calls/day on the free tier, and no header reports the remainder.
    fn is_metered(&self) -> bool { true }

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>> {
        let url = format!("{}/api/similar_tracks", self.base_url.trim_end_matches('/'));
        let resp = self.http.post(&url)
            .bearer_auth(&self.api_key)
            .json(&Request { artist: &seed.artist, track: &seed.title })
            .send().await
            .map_err(|source| Error::Transport { provider: NAME, source })?;

        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(|source| Error::Transport { provider: NAME, source })?;

        if status == 403 {
            return Err(Error::InvalidKey { provider: NAME, message: body });
        }
        if status == 429 {
            return Err(Error::RateLimited { provider: NAME, retry_after: None });
        }

        let parsed: Response = serde_json::from_str(&body)
            .map_err(|e| Error::UnexpectedBody { provider: NAME, message: format!("{e}: {body}") })?;

        if !parsed.success {
            return Err(Error::NotATrack {
                provider: NAME,
                artist: seed.artist.clone(),
                title: seed.title.clone(),
                message: parsed.error.unwrap_or_else(|| "no reason given".into()),
            });
        }

        Ok(parsed.matches.into_iter()
            .filter_map(|m| m.platform_ids.youtube.map(|id| Recommendation {
                artist: m.artist,
                title: m.title,
                playable: Playable::YouTubeId(id),
                isrc: None,
                source: NAME,
            }))
            .take(want)
            .collect())
    }
}
```

- [ ] **Step 5: Export and run**

Add `pub mod provider; pub use provider::{MusicAtlas, ReccoBeats, Recommender};` to `lib.rs`
(create a stub `reccobeats.rs` with `pub struct ReccoBeats;` so it compiles; Task 4 fills it).

Run: `cargo test -p crack-musicreco musicatlas`
Expected: PASS, 4 tests.

- [ ] **Step 6: Commit**

```bash
git add crack-musicreco
git commit -m "feat(musicreco): musicatlas recommender

🪤 The taxonomy keys on the \`success\` field, not the HTTP status: measured,
an unknown track returns HTTP 200 with success:false, so a status-only client
reads 'not a released track' as a success and then finds matches empty.

🪤 An explicit User-Agent is mandatory. Measured: Python-urllib is refused 403
with the SAME body as a bad key, so omitting it looks exactly like a bad
credential.

Matches without a youtube id are dropped rather than returned unplayable.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: ReccoBeats recommender

**Files:**
- Modify: `crack-musicreco/src/provider/reccobeats.rs`

**Interfaces:**
- Consumes: `Seed`, `Recommendation`, `Playable`, `Error`, `Recommender`.
- Produces: `ReccoBeats::new()`, `ReccoBeats::with_base_url(..)`.

**Measured contract:** base `https://api.reccobeats.com/v1`, **no auth**.
**Two calls.** Seeds are ReccoBeats UUIDs; Spotify ids are rejected with
`4002`. `GET /v1/track/search?searchText=<artist title>&size=1` →
`content[0].id`; then `GET /v1/track/recommendation?size=<n>&seeds=<uuid>` →
`content[]` of `{id,trackTitle,artists[{name}],isrc,href}`. **No YouTube id**,
so every result is `Playable::SearchQuery`. Limits are undisclosed; 429 carries
`Retry-After`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // (reuse the same `serve` helper shape as Task 3 — copy it into this module)

    const SEARCH: (u16, &str) = (200, r#"{"content":[{"id":"uuid-1","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Queen"}],"isrc":"X","href":"https://open.spotify.com/track/z"}]}"#);
    const RECO: (u16, &str) = (200, r#"{"content":[{"id":"uuid-2","trackTitle":"Mulla","artists":[{"name":"Kashcoming"}],"isrc":"USA2P2511772","href":"https://open.spotify.com/track/7x"}]}"#);

    #[tokio::test]
    async fn search_then_recommend_takes_two_calls_and_yields_search_queries() {
        let (base, hits) = serve(vec![SEARCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base);
        let out = p.recommend(&seed(), 5).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2, "search + recommend");
        assert_eq!(out.len(), 1);
        // 🔑 ReccoBeats never returns a video id.
        assert_eq!(out[0].youtube_id(), None);
        assert_eq!(out[0].search_text(), "Kashcoming - Mulla");
        assert_eq!(out[0].isrc.as_deref(), Some("USA2P2511772"));
    }

    #[tokio::test]
    async fn an_empty_search_makes_no_recommendation_call() {
        let (base, hits) = serve(vec![(200, r#"{"content":[]}"#), RECO]).await;
        let p = ReccoBeats::with_base_url(base);
        assert!(p.recommend(&seed(), 5).await.unwrap().is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 1,
            "must NOT reach the canned recommendation response");
    }

    #[tokio::test]
    async fn a_429_carries_retry_after() {
        let (base, _) = serve(vec![(429, r#"{"error":"slow down"}"#)]).await;
        let p = ReccoBeats::with_base_url(base);
        let err = p.recommend(&seed(), 5).await.expect_err("429");
        assert!(matches!(err, Error::RateLimited { .. }), "got {err}");
        assert!(err.is_transient(), "rate limiting passes");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-musicreco reccobeats`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use crate::{Error, Playable, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

const NAME: &str = "reccobeats";
pub const DEFAULT_BASE_URL: &str = "https://api.reccobeats.com/v1";
pub const USER_AGENT: &str = concat!("cracktunes/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Deserialize)]
struct Page { #[serde(default)] content: Vec<Track> }

#[derive(Debug, Deserialize)]
struct Track {
    id: String,
    #[serde(rename = "trackTitle")] track_title: String,
    #[serde(default)] artists: Vec<Artist>,
    #[serde(default)] isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Artist { name: String }

#[derive(Debug, Clone)]
pub struct ReccoBeats { base_url: String, http: reqwest::Client }

impl ReccoBeats {
    #[must_use]
    pub fn new() -> Self { Self::with_base_url(DEFAULT_BASE_URL) }

    #[must_use]
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder().user_agent(USER_AGENT).build().unwrap_or_default(),
        }
    }

    async fn get(&self, url: &str) -> Result<Page> {
        let resp = self.http.get(url).send().await
            .map_err(|source| Error::Transport { provider: NAME, source })?;
        if resp.status().as_u16() == 429 {
            let retry_after = resp.headers().get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(Error::RateLimited { provider: NAME, retry_after });
        }
        let body = resp.text().await.map_err(|source| Error::Transport { provider: NAME, source })?;
        serde_json::from_str(&body)
            .map_err(|e| Error::UnexpectedBody { provider: NAME, message: format!("{e}: {body}") })
    }
}

impl Default for ReccoBeats { fn default() -> Self { Self::new() } }

#[async_trait]
impl crate::provider::Recommender for ReccoBeats {
    fn name(&self) -> &'static str { NAME }

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>> {
        let base = self.base_url.trim_end_matches('/');
        // 🪤 TWO calls. Recommendation seeds are ReccoBeats UUIDs; Spotify ids
        // are rejected outright (measured: 4002 "Cannot find any track").
        let q = encode_query(&format!("{} {}", seed.artist, seed.title));
        let found = self.get(&format!("{base}/track/search?searchText={q}&size=1")).await?;
        let Some(first) = found.content.into_iter().next() else {
            // No seed track: do NOT spend the second call.
            return Ok(Vec::new());
        };
        let page = self.get(&format!("{base}/track/recommendation?size={want}&seeds={}", first.id)).await?;
        Ok(page.content.into_iter().map(|t| {
            let artist = t.artists.first().map_or_else(String::new, |a| a.name.clone());
            Recommendation {
                // ReccoBeats gives no video id, ever.
                playable: Playable::SearchQuery(format!("{artist} - {}", t.track_title)),
                artist,
                title: t.track_title,
                isrc: t.isrc,
                source: NAME,
            }
        }).take(want).collect())
    }
}

/// Minimal percent-encoding for a query value; avoids a dependency for one use.
/// `pub(crate)` because the MusicBrainz resolver in Task 5 uses the same one
/// rather than carrying a second copy.
pub(crate) fn encode_query(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "%20".to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p crack-musicreco reccobeats`
Expected: PASS, 3 tests.

- [ ] **Step 5: Commit**

```bash
git add crack-musicreco
git commit -m "feat(musicreco): ReccoBeats recommender

Two calls, not one: recommendation seeds are ReccoBeats UUIDs and Spotify ids
are rejected outright (measured 4002), so a search resolves the seed first. An
empty search short-circuits rather than spending the second call.

Every result is Playable::SearchQuery -- ReccoBeats returns no YouTube id, so
pretending otherwise would hand the bot something it cannot play.

Limits are undisclosed, so 429 + Retry-After is honored reactively rather than
budgeted ahead.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: MusicBrainz seed canonicalizer

**Files:**
- Create: `crack-musicreco/src/resolver/musicbrainz.rs`
- Modify: `crack-musicreco/src/resolver/mod.rs`

**Interfaces:**
- Consumes: `RawTrack`, `Seed`, `SeedResolver`.
- Produces: `MusicBrainz::new(contact_email)`, `MusicBrainz::with_base_url(..)`.

**Measured contract:** `GET
https://musicbrainz.org/ws/2/recording?query=artist:<a> AND recording:"<t>"&fmt=json&limit=1`,
no key. Returns `{recordings:[{id,score,title,artist-credit:[{name}]}]}`; a
good match scores 100. **Hard 1 request/second** and a descriptive User-Agent
with a contact address are both mandatory — exceeding the rate blocks the whole
estate's IP, not one guild.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // (copy the `serve` helper from Task 3)

    const HIT: (u16, &str) = (200, r#"{"recordings":[{"id":"mbid-1","score":100,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"}]}]}"#);

    #[tokio::test]
    async fn a_confident_match_raises_confidence_and_carries_the_mbid() {
        let (base, _) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base);
        let seed = mb.resolve(&RawTrack {
            title: "Queen - Bohemian Rhapsody".into(), artist: None, uploader: None,
        }).await.unwrap().expect("a seed");
        assert_eq!(seed.artist, "Queen");
        assert_eq!(seed.confidence, 100);
        assert_eq!(seed.mbid.as_deref(), Some("mbid-1"));
    }

    #[tokio::test]
    async fn no_recordings_yields_no_seed_rather_than_an_error() {
        let (base, _) = serve(vec![(200, r#"{"recordings":[]}"#)]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base);
        assert!(mb.resolve(&RawTrack { title: "Queen - Nope".into(), artist: None, uploader: None })
            .await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_title_with_no_separator_makes_no_request_at_all() {
        let (base, hits) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base);
        let got = mb.resolve(&RawTrack { title: "Never Gonna Give You Up".into(), artist: None, uploader: None }).await.unwrap();
        assert!(got.is_none());
        assert_eq!(hits.load(Ordering::SeqCst), 0,
            "nothing to query with, so the 1/sec budget is not spent");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-musicreco musicbrainz`
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use crate::resolver::{title_parse::TitleParseResolver, SeedResolver};
use crate::{Error, RawTrack, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

const NAME: &str = "musicbrainz";
pub const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2";
/// 🪤 HARD limit, and it is enforced against the IP. Exceeding it blocks every
/// service on this host, not just one guild's autoplay.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

#[derive(Debug, Deserialize)]
struct SearchResponse { #[serde(default)] recordings: Vec<Recording> }

#[derive(Debug, Deserialize)]
struct Recording {
    id: String,
    #[serde(default)] score: u8,
    title: String,
    #[serde(rename = "artist-credit", default)] artist_credit: Vec<Credit>,
}

#[derive(Debug, Deserialize)]
struct Credit { name: String }

pub struct MusicBrainz {
    base_url: String,
    http: reqwest::Client,
    /// Serializes every call and enforces the 1/sec floor.
    gate: Mutex<Option<Instant>>,
}

impl MusicBrainz {
    #[must_use]
    pub fn new(contact: &str) -> Self { Self::with_base_url(contact, DEFAULT_BASE_URL) }

    #[must_use]
    pub fn with_base_url(contact: &str, base_url: impl Into<String>) -> Self {
        // The UA must identify the app AND carry a contact address.
        let ua = format!("cracktunes/{} ( {contact} )", env!("CARGO_PKG_VERSION"));
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder().user_agent(ua).build().unwrap_or_default(),
            gate: Mutex::new(None),
        }
    }

    async fn throttle(&self) {
        let mut last = self.gate.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < MIN_INTERVAL {
                tokio::time::sleep(MIN_INTERVAL - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }
}

#[async_trait]
impl SeedResolver for MusicBrainz {
    fn name(&self) -> &'static str { NAME }

    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>> {
        // Reuse the offline parse to get something to ask ABOUT. With no
        // artist there is no query, and the rate budget is not spent.
        let Some(guess) = TitleParseResolver::new().resolve(raw).await? else {
            return Ok(None);
        };
        let q = format!(r#"artist:{} AND recording:"{}""#, guess.artist, guess.title);
        let url = format!("{}/recording?query={}&fmt=json&limit=1",
            self.base_url.trim_end_matches('/'), crate::provider::reccobeats::encode_query(&q));

        self.throttle().await;
        let resp = self.http.get(&url).send().await
            .map_err(|source| Error::Transport { provider: NAME, source })?;
        let body = resp.text().await.map_err(|source| Error::Transport { provider: NAME, source })?;
        let parsed: SearchResponse = serde_json::from_str(&body)
            .map_err(|e| Error::UnexpectedBody { provider: NAME, message: format!("{e}: {body}") })?;

        Ok(parsed.recordings.into_iter().next().map(|r| Seed {
            artist: r.artist_credit.first().map_or(guess.artist, |c| c.name.clone()),
            title: r.title,
            mbid: Some(r.id),
            confidence: r.score,
        }))
    }
}
```

Note: `encode_query` is defined once in `reccobeats.rs` (Task 4) as
`pub(crate)` and reused here rather than duplicated.

- [ ] **Step 4: Export it**

In `crack-musicreco/src/resolver/mod.rs` add `pub mod musicbrainz;` and
`pub use musicbrainz::MusicBrainz;`, and re-export from `lib.rs`:
`pub use resolver::{MusicBrainz, SeedResolver, TitleParseResolver};`

- [ ] **Step 5: Run tests**

Run: `cargo test -p crack-musicreco musicbrainz`
Expected: PASS, 3 tests.

- [ ] **Step 5: Commit**

```bash
git add crack-musicreco
git commit -m "feat(musicreco): MusicBrainz as a seed canonicalizer, not a recommender

MusicBrainz has no similar-track capability at all -- it is metadata lookup. It
earns its place by fixing the weakest part of the design: seeds are parsed out
of YouTube titles because yt-dlp supplies no artist, and a parse is a guess.
MusicBrainz scores the guess so a metered call is never spent on a bad one.

🪤 The 1 req/sec limit is enforced against the IP, so exceeding it blocks every
service on the host rather than one guild's autoplay. A mutex-guarded gate
serializes calls; the User-Agent carries a contact address as required.

A title with no separator makes no request: there is nothing to ask about.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: The `MusicReco` orchestrator

**Files:**
- Create: `crack-musicreco/src/reco.rs`
- Modify: `crack-musicreco/src/lib.rs`

**Interfaces:**
- Consumes: `SeedResolver`, `Recommender`, `RawTrack`, `Recommendation`, `Error`.
- Produces: `MusicReco`, `MusicRecoBuilder`, `Policy { min_seed_confidence }`,
  `MusicReco::next_tracks(&RawTrack, usize) -> Result<Vec<Recommendation>>`.

- [ ] **Step 1: Write the failing tests**

In `crack-musicreco/src/reco.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeReco {
        name: &'static str,
        calls: Arc<AtomicUsize>,
        result: fn() -> Result<Vec<Recommendation>>,
    }

    #[async_trait::async_trait]
    impl Recommender for FakeReco {
        fn name(&self) -> &'static str { self.name }
        async fn recommend(&self, _s: &Seed, _w: usize) -> Result<Vec<Recommendation>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.result)()
        }
    }

    fn one(src: &'static str) -> Result<Vec<Recommendation>> {
        Ok(vec![Recommendation {
            artist: "A".into(), title: "T".into(),
            playable: Playable::YouTubeId("v".into()), isrc: None, source: src,
        }])
    }
    fn boom() -> Result<Vec<Recommendation>> {
        Err(Error::BudgetExhausted { provider: "first", budget: 90 })
    }
    fn empty() -> Result<Vec<Recommendation>> { Ok(vec![]) }

    fn raw() -> RawTrack {
        RawTrack { title: "Queen - Bohemian Rhapsody".into(), artist: None, uploader: None }
    }

    #[tokio::test]
    async fn the_first_recommender_that_answers_wins_and_the_rest_are_not_called() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco { name: "first", calls: Arc::clone(&a), result: || one("first") }))
            .recommender(Box::new(FakeReco { name: "second", calls: Arc::clone(&b), result: || one("second") }))
            .policy(Policy { min_seed_confidence: 0 })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "first");
        assert_eq!((a.load(Ordering::SeqCst), b.load(Ordering::SeqCst)), (1, 0));
    }

    /// 🔑 The entire point of the crate: one provider failing must not end
    /// autoplay.
    #[tokio::test]
    async fn a_failing_first_recommender_falls_through_to_the_second() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco { name: "first", calls: Arc::clone(&a), result: boom }))
            .recommender(Box::new(FakeReco { name: "second", calls: Arc::clone(&b), result: || one("second") }))
            .policy(Policy { min_seed_confidence: 0 })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "second");
        assert_eq!(b.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_empty_answer_also_falls_through() {
        let b = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco { name: "first", calls: Arc::new(AtomicUsize::new(0)), result: empty }))
            .recommender(Box::new(FakeReco { name: "second", calls: Arc::clone(&b), result: || one("second") }))
            .policy(Policy { min_seed_confidence: 0 })
            .build();
        assert_eq!(r.next_tracks(&raw(), 5).await.unwrap()[0].source, "second");
    }

    #[tokio::test]
    async fn no_derivable_seed_calls_nothing() {
        let a = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco { name: "first", calls: Arc::clone(&a), result: || one("first") }))
            .policy(Policy { min_seed_confidence: 0 })
            .build();
        let raw = RawTrack { title: "Never Gonna Give You Up".into(), artist: None, uploader: None };
        assert!(r.next_tracks(&raw, 5).await.unwrap().is_empty());
        assert_eq!(a.load(Ordering::SeqCst), 0, "no seed, no calls, no quota");
    }

    struct MeteredFake { calls: Arc<AtomicUsize> }

    #[async_trait::async_trait]
    impl Recommender for MeteredFake {
        fn name(&self) -> &'static str { "metered" }
        fn is_metered(&self) -> bool { true }
        async fn recommend(&self, _s: &Seed, _w: usize) -> Result<Vec<Recommendation>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            one("metered")
        }
    }

    /// A bare parse scores 50. With a floor of 80 the METERED provider is
    /// skipped -- but the free one must still run, or a MusicBrainz outage
    /// would kill autoplay outright.
    #[tokio::test]
    async fn a_low_confidence_seed_skips_metered_providers_but_not_free_ones() {
        let (m, f) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(MeteredFake { calls: Arc::clone(&m) }))
            .recommender(Box::new(FakeReco { name: "free", calls: Arc::clone(&f), result: || one("free") }))
            .policy(Policy { min_seed_confidence: 80 })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "free");
        assert_eq!(m.load(Ordering::SeqCst), 0, "no quota spent on a guess");
        assert_eq!(f.load(Ordering::SeqCst), 1, "the free provider still ran");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-musicreco reco`
Expected: FAIL — `MusicReco` not defined.

- [ ] **Step 3: Implement**

```rust
use crate::provider::Recommender;
use crate::resolver::SeedResolver;
use crate::{Error, RawTrack, Recommendation, Result, Seed};

/// Tunables that are policy rather than protocol.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Below this, metered providers are skipped instead of guessed at. A bare
    /// title parse scores 50; a MusicBrainz-confirmed seed scores its match.
    pub min_seed_confidence: u8,
}

impl Default for Policy {
    fn default() -> Self { Self { min_seed_confidence: 80 } }
}

#[derive(Default)]
pub struct MusicRecoBuilder {
    resolvers: Vec<Box<dyn SeedResolver>>,
    recommenders: Vec<Box<dyn Recommender>>,
    policy: Policy,
}

impl MusicRecoBuilder {
    #[must_use] pub fn resolver(mut self, r: Box<dyn SeedResolver>) -> Self { self.resolvers.push(r); self }
    #[must_use] pub fn recommender(mut self, r: Box<dyn Recommender>) -> Self { self.recommenders.push(r); self }
    #[must_use] pub fn policy(mut self, p: Policy) -> Self { self.policy = p; self }
    #[must_use] pub fn build(self) -> MusicReco {
        MusicReco { resolvers: self.resolvers, recommenders: self.recommenders, policy: self.policy }
    }
}

pub struct MusicReco {
    resolvers: Vec<Box<dyn SeedResolver>>,
    recommenders: Vec<Box<dyn Recommender>>,
    policy: Policy,
}

impl MusicReco {
    #[must_use] pub fn builder() -> MusicRecoBuilder { MusicRecoBuilder::default() }

    /// The best seed any resolver produced. Later resolvers may only RAISE
    /// confidence -- MusicBrainz confirming a parse, never replacing a
    /// caller-supplied artist with a worse guess.
    async fn best_seed(&self, raw: &RawTrack) -> Option<Seed> {
        let mut best: Option<Seed> = None;
        for r in &self.resolvers {
            match r.resolve(raw).await {
                Ok(Some(s)) => {
                    if best.as_ref().is_none_or(|b| s.confidence > b.confidence) {
                        best = Some(s);
                    }
                },
                Ok(None) => {},
                // A resolver failing is not fatal: the offline parse still stands.
                Err(e) => tracing::warn!("seed resolver {} failed: {e}", r.name()),
            }
        }
        best
    }

    /// Candidate next-tracks, or an empty vec when nothing could be produced.
    ///
    /// 🔑 Never returns `Err` for a provider failure: a provider is allowed to
    /// fail, and falling through to the next is the reason this crate exists.
    /// An empty result means every avenue was tried.
    ///
    /// # Errors
    /// Currently infallible; the signature keeps room for a future fatal case.
    pub async fn next_tracks(&self, raw: &RawTrack, want: usize) -> Result<Vec<Recommendation>> {
        let Some(seed) = self.best_seed(raw).await else {
            tracing::debug!("no seed derivable from {:?}; spending nothing", raw.title);
            return Ok(Vec::new());
        };
        let shaky = seed.confidence < self.policy.min_seed_confidence;
        for r in &self.recommenders {
            // 🔑 The floor gates METERED providers only. Skipping free ones too
            // would make a MusicBrainz outage fatal to autoplay.
            if shaky && r.is_metered() {
                tracing::debug!(
                    "seed `{} - {}` scored {} (< {}); skipping metered {}",
                    seed.artist, seed.title, seed.confidence,
                    self.policy.min_seed_confidence, r.name()
                );
                continue;
            }
            match r.recommend(&seed, want).await {
                Ok(v) if !v.is_empty() => return Ok(v),
                Ok(_) => tracing::debug!("{} had nothing for `{}`", r.name(), seed.title),
                Err(e) => tracing::warn!("{} failed ({e}); trying the next provider", r.name()),
            }
        }
        Ok(Vec::new())
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p crack-musicreco`
Expected: PASS, all tests.

- [ ] **Step 5: Commit**

```bash
git add crack-musicreco
git commit -m "feat(musicreco): the orchestrator, with per-provider fallback

next_tracks never returns Err for a provider failure -- falling through to the
next provider is the reason the crate exists, so a musicatlas budget exhaustion
degrades to ReccoBeats instead of ending autoplay. An empty vec means every
avenue was tried.

Resolvers may only RAISE confidence, so MusicBrainz can confirm a parse but
never replaces a caller-supplied artist with a worse guess.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 7: Postgres cache and budget

**Files:**
- Create: `migrations/20260912120000_musicreco_cache.sql`
- Create: `crack-core/src/db/musicreco.rs`
- Modify: `crack-core/src/db/mod.rs`

**Interfaces:**
- Consumes: `crack_musicreco::{Recommendation, Seed}`.
- Produces: `MusicRecoCache::get(pool, provider, seed)`,
  `MusicRecoCache::put(pool, provider, seed, &[Recommendation], found)`,
  `MusicRecoCache::try_spend(pool, provider, budget) -> Result<bool>`.

**Note:** the cache lives in `crack-core` (which already owns the pool and the
migrations), not in `crack-musicreco`, so the client crate stays free of a
database dependency — the same separation `crack-sleevenote` keeps.

- [ ] **Step 1: Write the migration**

`migrations/20260912120000_musicreco_cache.sql`:

```sql
-- Keys are NORMALIZED (trim, collapse whitespace, lowercase) so `Queen ` and
-- `queen` are one entry rather than two metered calls. The values sent to the
-- provider are the un-normalized originals.
CREATE TABLE IF NOT EXISTS musicreco_cache (
    provider   TEXT        NOT NULL,
    artist     TEXT        NOT NULL,
    track      TEXT        NOT NULL,
    results    JSONB       NOT NULL,
    -- 🔑 false rows are the point: a seed that returned nothing cost a call,
    -- and without a negative entry it costs one every time that track ends.
    found      BOOLEAN     NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, artist, track)
);

-- No rate-limit header exists to read, so usage is counted locally. Rows for
-- past days are retained: they are the only record of actual quota usage.
CREATE TABLE IF NOT EXISTS musicreco_budget (
    provider TEXT    NOT NULL,
    day      DATE    NOT NULL,
    calls    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider, day)
);
```

- [ ] **Step 2: Apply it and regenerate the sqlx cache**

```bash
sqlx migrate run --source migrations/
cargo sqlx prepare --workspace -- --tests --all
```

- [ ] **Step 3: Write the failing test**

In `crack-core/src/db/musicreco.rs`, behind the existing `db-tests` feature
gate used elsewhere in this module (follow `crack-core/src/db/play_log.rs`):

```rust
#[cfg(all(test, feature = "db-tests"))]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn a_negative_result_is_remembered(pool: sqlx::PgPool) {
        let seed = Seed { artist: "Lofi Radio".into(), title: "Beats".into(), mbid: None, confidence: 100 };
        MusicRecoCache::put(&pool, "musicatlas", &seed, &[], false).await.unwrap();
        let got = MusicRecoCache::get(&pool, "musicatlas", &seed).await.unwrap();
        let entry = got.expect("a negative entry is still an entry");
        assert!(!entry.found, "the caller must be able to skip the call entirely");
        assert!(entry.results.is_empty());
    }

    #[sqlx::test]
    async fn keys_are_normalized_so_case_and_padding_do_not_double_spend(pool: sqlx::PgPool) {
        let a = Seed { artist: "Queen".into(), title: "Bohemian Rhapsody".into(), mbid: None, confidence: 100 };
        let b = Seed { artist: "  QUEEN ".into(), title: "bohemian   rhapsody".into(), mbid: None, confidence: 100 };
        MusicRecoCache::put(&pool, "musicatlas", &a, &[], true).await.unwrap();
        assert!(MusicRecoCache::get(&pool, "musicatlas", &b).await.unwrap().is_some());
    }

    #[sqlx::test]
    async fn the_budget_stops_at_the_limit(pool: sqlx::PgPool) {
        for i in 0..3 {
            assert!(MusicRecoCache::try_spend(&pool, "musicatlas", 3).await.unwrap(), "call {i}");
        }
        assert!(!MusicRecoCache::try_spend(&pool, "musicatlas", 3).await.unwrap(),
            "the fourth call must be refused, not merely logged");
    }
}
```

- [ ] **Step 4: Run to verify it fails**

Run: `cargo test -p crack-core --features db-tests musicreco`
Expected: FAIL — `MusicRecoCache` not defined.

- [ ] **Step 5: Implement**

```rust
use crack_musicreco::{Recommendation, Seed};
use sqlx::PgPool;

/// Cache keys only. The provider receives the un-normalized values.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub results: Vec<Recommendation>,
    pub found: bool,
}

pub struct MusicRecoCache;

impl MusicRecoCache {
    /// # Errors
    /// Any sqlx failure.
    pub async fn get(pool: &PgPool, provider: &str, seed: &Seed) -> Result<Option<CacheEntry>, sqlx::Error> {
        let row = sqlx::query!(
            "SELECT results, found FROM musicreco_cache WHERE provider = $1 AND artist = $2 AND track = $3",
            provider, normalize(&seed.artist), normalize(&seed.title),
        ).fetch_optional(pool).await?;

        Ok(row.map(|r| CacheEntry {
            // The blob is opaque to the database; a decode failure means a
            // stale shape, which is a miss rather than an error.
            results: serde_json::from_value(r.results).unwrap_or_default(),
            found: r.found,
        }))
    }

    /// # Errors
    /// Any sqlx failure, or a serialization failure of the results.
    pub async fn put(pool: &PgPool, provider: &str, seed: &Seed, results: &[Recommendation], found: bool)
        -> Result<(), sqlx::Error>
    {
        let blob = serde_json::to_value(results).unwrap_or(serde_json::Value::Array(vec![]));
        sqlx::query!(
            "INSERT INTO musicreco_cache (provider, artist, track, results, found)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (provider, artist, track)
             DO UPDATE SET results = EXCLUDED.results, found = EXCLUDED.found, fetched_at = now()",
            provider, normalize(&seed.artist), normalize(&seed.title), blob, found,
        ).execute(pool).await?;
        Ok(())
    }

    /// Reserve one call against today's budget. `Ok(false)` means refuse.
    ///
    /// 🔑 Increments BEFORE the call and in one statement, so a crash between
    /// the call and the bookkeeping cannot under-count a quota that no response
    /// header reports back.
    ///
    /// # Errors
    /// Any sqlx failure.
    pub async fn try_spend(pool: &PgPool, provider: &str, budget: i32) -> Result<bool, sqlx::Error> {
        let row = sqlx::query!(
            "INSERT INTO musicreco_budget (provider, day, calls) VALUES ($1, CURRENT_DATE, 1)
             ON CONFLICT (provider, day) DO UPDATE
               SET calls = musicreco_budget.calls + 1
               WHERE musicreco_budget.calls < $2
             RETURNING calls",
            provider, budget,
        ).fetch_optional(pool).await?;
        Ok(row.is_some())
    }
}
```

- [ ] **Step 6: Export and verify offline compilation**

Add `pub mod musicreco;` to `crack-core/src/db/mod.rs`, then:

```bash
cargo sqlx prepare --workspace -- --tests --all
SQLX_OFFLINE=true cargo check --workspace
```

Expected: clean. **This step is not optional** — CI's Docker job is skipped on
PRs, so a missing `.sqlx` entry reaches master otherwise.

- [ ] **Step 7: Commit**

```bash
git add migrations crack-core .sqlx
git commit -m "feat(musicreco): Postgres cache with negative entries, and a budget

found=false rows are the point: a seed that returned nothing still cost a call,
and without a negative entry it costs one every time that track ends. Negative
caching is a quota feature.

try_spend increments before the call in a single statement, so a crash between
the call and the bookkeeping cannot under-count a quota no response header
reports back.

Keys are normalized (trim, collapse whitespace, lowercase) so 'Queen ' and
'queen' are one entry rather than two metered calls; providers still receive the
original values.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 8: Wire autoplay into `track_end.rs`

**Files:**
- Modify: `crack-core/src/handlers/track_end.rs` (replace `get_recommended_track_query`)
- Modify: `crack-core/src/lib.rs` (add the per-guild buffer to `Data`)
- Modify: `crack-core/Cargo.toml` (depend on `crack-musicreco`)
- Modify: `crack-core/src/config.rs` (build `MusicReco` at startup)

**Interfaces:**
- Consumes: `MusicReco`, `RawTrack`, `Recommendation`, `Playable`, `MusicRecoCache`.
- Produces: `Data::musicreco: Option<Arc<MusicReco>>`,
  `Data::autoplay_buffer: Arc<RwLock<HashMap<GuildId, Vec<Recommendation>>>>`.

- [ ] **Step 1: Add the dependency**

In `crack-core/Cargo.toml`, beside the existing `crack-sleevenote` line:

```toml
crack-musicreco = { path = "../crack-musicreco" }
```

- [ ] **Step 2: Add buffer and client to `Data`**

In `crack-core/src/lib.rs`, beside the other per-guild maps:

```rust
/// Pending autoplay recommendations per guild.
///
/// 🔑 ONE API call yields ~20 tracks. Draining a buffer is what keeps the
/// metered provider inside a 100/day free tier; calling per track-end would be
/// ~20x the requests for no benefit.
pub autoplay_buffer: Arc<RwLock<HashMap<GuildId, Vec<crack_musicreco::Recommendation>>>>,
pub musicreco: Option<Arc<crack_musicreco::MusicReco>>,
```

- [ ] **Step 3: Build it at startup**

In `crack-core/src/config.rs`, where the other optional services are built:

```rust
// Autoplay works without a key: ReccoBeats needs none. The metered provider is
// added only when a key is present, so a deployment with no key degrades to the
// free provider rather than losing autoplay.
let musicreco = {
    let mut b = crack_musicreco::MusicReco::builder()
        .resolver(Box::new(crack_musicreco::TitleParseResolver::new()))
        .resolver(Box::new(crack_musicreco::MusicBrainz::new("cycle.five@proton.me")));
    if let Ok(key) = std::env::var("MUSICATLAS_API_KEY") {
        match crack_musicreco::MusicAtlas::new(key) {
            Ok(p) => b = b.recommender(Box::new(p)),
            Err(e) => tracing::warn!("musicatlas disabled: {e}"),
        }
    } else {
        tracing::info!("MUSICATLAS_API_KEY unset; autoplay uses ReccoBeats only");
    }
    b = b.recommender(Box::new(crack_musicreco::ReccoBeats::new()));
    Some(Arc::new(b.build()))
};
```

- [ ] **Step 4: Replace `get_recommended_track_query`**

Delete the Spotify-based function and replace its call site (around
`track_end.rs:215`) with:

```rust
// 🔴 The Spotify path this replaces was unreachable: it checked Spotify auth
// BEFORE reading history, production has no client credentials, and Spotify
// blocked new Web API app creation around 2025-12. Fixing play history did not
// and could not revive it.
let next = match self.next_autoplay_track().await {
    Some(rec) => rec,
    None => {
        self.data.set_autoplay(self.guild_id, false).await;
        tracing::warn!("autoplay disabled for {}: no recommendation", self.guild_id);
        announce_autoplay_off(channel, self.http.clone(), &CrackedError::Other(
            "no recommendation available",
        )).await;
        return None;
    },
};
let query = match &next.playable {
    crack_musicreco::Playable::YouTubeId(id) =>
        QueryType::VideoLink(format!("https://www.youtube.com/watch?v={id}")),
    crack_musicreco::Playable::SearchQuery(q) => QueryType::Keywords(q.clone()),
};
```

And add the buffer-draining helper to `TrackEndHandler`:

```rust
/// Pop a pending recommendation, refilling from the providers when empty.
async fn next_autoplay_track(&self) -> Option<crack_musicreco::Recommendation> {
    {
        let mut buf = self.data.autoplay_buffer.write().await;
        if let Some(v) = buf.get_mut(&self.guild_id) {
            if let Some(next) = v.pop() {
                return Some(next);
            }
        }
    }
    let reco = self.data.musicreco.as_ref()?;
    let raw = self.ended_raw_track()?;
    let mut fresh = reco.next_tracks(&raw, 20).await.ok()?;
    let first = fresh.pop()?;
    self.data.autoplay_buffer.write().await.insert(self.guild_id, fresh);
    Some(first)
}
```

`ended_raw_track` builds a `RawTrack` from the ended `TrackHandle`'s
`AuxMetadata` (`title`, `artist`), which is already in-process at this point —
no database read.

- [ ] **Step 5: Verify the whole workspace**

```bash
cargo fmt --check
SQLX_OFFLINE=true cargo check --workspace
cargo clippy --workspace --all-targets
cargo test --workspace
```

Expected: clean. `crack-testing::tests::test_enqueue_query` fails on master too
(a deleted YouTube video id) — that one is pre-existing, not a regression.

- [ ] **Step 6: Commit**

```bash
git add crack-core Cargo.lock
git commit -m "feat(autoplay): recommend through crack-musicreco

Replaces a path that could never run: it checked Spotify auth before reading
play history, production has no client credentials, and Spotify blocked new Web
API app creation around 2025-12.

One call fills a ~20-track per-guild buffer that later track-ends drain, which
is what keeps the metered provider inside a 100/day free tier -- calling per
track-end would be ~20x the requests for no benefit.

Autoplay now works with NO key at all: ReccoBeats needs none, so a deployment
without MUSICATLAS_API_KEY degrades to the free provider rather than losing the
feature.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 9: Version bump, secret, and deployment notes

**Files:**
- Modify: all ten `*/Cargo.toml` version lines
- Modify: `docs/superpowers/specs/2026-09-12-musicatlas-autoplay-design.md` (delete)
- Modify: `README.md` (env var table, if one exists)

- [ ] **Step 1: Bump every member to 0.9.8**

```bash
for f in */Cargo.toml; do sed -i '0,/^version = "0.9.7"/s//version = "0.9.8"/' "$f"; done
grep -c '^version = "0.9.8"' */Cargo.toml | grep -v ':0' | wc -l   # expect 10
```

- [ ] **Step 2: Delete the superseded spec**

```bash
git rm docs/superpowers/specs/2026-09-12-musicatlas-autoplay-design.md
```

Its measured API facts were carried into the musicreco spec; keeping both
invites someone to implement the single-provider design by mistake.

- [ ] **Step 3: Record the secret**

`MUSICATLAS_API_KEY` goes in the gitignored `.env.bots` on the workstation,
sourced from Vaultwarden item `musicatlas-api-key` via
`./secrets/rbw-get.sh musicatlas-api-key` in the homelab repo, and is passed to
the container by `homelab.sh`'s `--env-file`. **It must not be written to the
docker host.** Add the compose `environment:` line in the homelab repo, not here.

- [ ] **Step 4: Full gate and commit**

```bash
cargo fmt --check && SQLX_OFFLINE=true cargo check --workspace && cargo test --workspace
git add -A
git commit -m "chore: v0.9.8 -- autoplay on crack-musicreco

Deletes the superseded single-provider musicatlas spec; its measured API facts
live on in the musicreco spec, and keeping both invites someone to implement the
wrong one.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

- [ ] **Step 5: Live verification before production**

Deploy to **tunetitan first**, then confirm on a real play:

1. Autoplay on, let a track end → a related track plays.
2. A second track-end in the same chain plays with **no new API call**
   (`select calls from musicreco_budget where day = CURRENT_DATE`).
3. `select found, count(*) from musicreco_cache group by found` shows both.
4. Force `MUSICATLAS_API_KEY` empty → autoplay still works via ReccoBeats.

🪤 **The deploy succeeding is not the verification.** A v0.9.3 fix for play
history passed its tests, deployed cleanly, verified green — and wrote history
for single tracks only, silently writing nothing for playlists, because the
test asserted a hard-coded count of the three enqueue sites its author knew
about rather than the property. Playlists went through a fourth. v0.9.5 fixed
it by pinning the whole enqueue surface, which immediately turned up two more
entry points. Confirm against the database, on a real play — and on a real
playlist, since the difference between those two paths IS what was missed.
