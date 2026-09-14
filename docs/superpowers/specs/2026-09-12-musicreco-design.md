# `crack-musicreco` — a provider-agnostic music recommendation crate

**Status:** approved in chat 2026-09-12. Supersedes
`2026-09-12-musicatlas-autoplay-design.md`, which designed a single-provider
musicatlas integration. That spec's measured API facts are carried forward here
and it should be deleted when this lands.

**Goal:** one crate that turns "this track just finished" into "here is the next
track to play", backed by several interchangeable providers, so the source can
be swapped or extended without touching the bot.

## 1. Why more than one provider

Autoplay is dead on production: `get_recommended_track_query` checks Spotify
auth before reading history, there are no client credentials, and Spotify
blocked new Web API app creation around 2025-12. The replacement should not
recreate the same single point of failure -- hence a crate that holds several
providers behind one interface, with fallback.

🪤 **An earlier note claimed empty `play_log` broke autoplay. It did not.** The
auth check fails first. Fixing play history neither fixed nor will fix autoplay.

## 2. The providers are NOT interchangeable

All three were probed live on 2026-09-12. The differences below are the whole
reason this crate needs a real design rather than a trait with three impls.

| | musicatlas | ReccoBeats | MusicBrainz |
|---|---|---|---|
| role | recommender | recommender | **not a recommender** |
| auth | Bearer key (rbw `musicatlas-api-key`) | **none** | none (UA required) |
| seed accepted | `{artist, track}` strings | its **own UUID** | free-text search |
| results/call | 20 | configurable (`size`) |  n/a |
| **YouTube id** | ✅ every match | ❌ never | ❌ |
| other ids | spotify, apple, deezer | isrc, spotify href | MBID, isrc |
| quota | **100/day**, no headers | **undisclosed**, 429 + `Retry-After` | 1 req/sec, hard |

🔴 **Superseded 2026-09-13 (v0.12.0): ReccoBeats is removed.** On production
v0.11.x it answered nothing on-genre for any seed. Its recommendations are
nearest neighbours in audio-feature space (tempo, energy, danceability,
valence), with no listening data: "Hit That" (131 BPM, energy 0.81) gave Calvin
Harris (128, 0.88) and Banda Toro (120, 0.77), and five Offspring seeds gave
Lambada and Gunna. More seeds made it worse. YouTube's Mix (§5.4) and Deezer's
artist radio (§5.5) replace it; both come from listening data.

### 2.1 What each one is actually for

- **YouTube's Mix** — the primary recommender since v0.12.0. It works from the
  ended video's id alone, so it needs no seed at all, and returns playable ids.
- **Deezer** — the fallback. Artist radio from listening data; needs a seed
  artist, and its results need a search step to become playable.
- **musicatlas** — the metered last resort. Returns playable YouTube ids, but
  spends a 100/day budget, so it is asked only when both free providers had
  nothing.
- **MusicBrainz** — **not a recommender at all.** It is a *seed canonicalizer*.

### 2.2 MusicBrainz earns its place by fixing the seed problem

Measured: yt-dlp returns `artist: NA` and `track: NA` for ordinary music
videos. The information is only in the title:

| `artist` | `track` | `title` |
|---|---|---|
| NA | NA | `Guns N' Roses - Sweet Child O' Mine (Official Music Video)` |
| NA | NA | `Queen – Bohemian Rhapsody (Official Video Remastered)` |

So a seed must be parsed out of a title, and parsing is guesswork. MusicBrainz
turns the guess into a checked fact: a recording search for
`artist:Queen AND recording:"Bohemian Rhapsody"` returns **score 100** matches
with MBIDs. A low score means "do not spend a metered musicatlas call on this".

🪤 **ISRC is not a usable bridge between providers.** A real ReccoBeats ISRC
(`USA2P2511772`) returned **404** from MusicBrainz's `/isrc/` lookup. Do not
design a cross-provider join on ISRC; it is a nice-to-have field, not a key.

## 3. Architecture

Two traits, because the providers do two different jobs. Collapsing them into
one `Provider` trait would force MusicBrainz to pretend it recommends.

```rust
/// Turn messy input into a seed worth spending a metered call on.
#[async_trait]
pub trait SeedResolver: Send + Sync {
    fn name(&self) -> &'static str;
    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>, Error>;
}

/// Turn a seed into candidate next-tracks.
#[async_trait]
pub trait Recommender: Send + Sync {
    fn name(&self) -> &'static str;
    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>, Error>;
}
```

### 3.1 Core types

```rust
/// What the bot knows about the track that just ended.
pub struct RawTrack {
    pub title: String,            // always present
    pub artist: Option<String>,   // usually None from yt-dlp
    pub uploader: Option<String>, // often the artist, with noise
}

pub struct Seed {
    pub artist: String,
    pub title: String,
    pub mbid: Option<String>,
    /// 0-100. MusicBrainz's score, or 100 for a seed the caller supplied
    /// directly. Below `min_seed_confidence` no metered call is made.
    pub confidence: u8,
}

pub struct Recommendation {
    pub artist: String,
    pub title: String,
    pub playable: Playable,
    pub isrc: Option<String>,
    pub source: &'static str,   // which provider produced it
}

/// 🔑 Providers differ in how playable their results are, and the type says so
/// rather than hiding it behind a lowest common denominator.
pub enum Playable {
    /// musicatlas: play this video id directly, no search.
    YouTubeId(String),
    /// Deezer: no video id exists; the bot must search for this string.
    SearchQuery(String),
}
```

### 3.2 The orchestrator

```rust
pub struct MusicReco {
    resolvers: Vec<Box<dyn SeedResolver>>,     // tried in order, first Some wins
    recommenders: Vec<Box<dyn Recommender>>,   // tried in order, first non-empty wins
    policy: Policy,
}

impl MusicReco {
    pub async fn next_tracks(&self, raw: &RawTrack, want: usize)
        -> Result<Vec<Recommendation>, Error>;
}
```

Default ordering:

- resolvers: `TitleParse` (offline, free) → `MusicBrainz` (confirms/corrects it)
- recommenders (v0.12.0): `YouTubeMix` (free, no seed, playable ids) →
  `Deezer` (free, needs a seed, needs search) → `MusicAtlas` (metered, only with
  a key and a database)

🔑 **The seed is resolved lazily.** `Recommender::needs_seed` is `false` for
YouTube's Mix, and the orchestrator runs the resolvers only when it first
reaches a provider that needs a seed. A Mix that answers spends no MusicBrainz
call, and a title nothing can be parsed from still gets a Mix.

🔑 **Fallback is per-provider, not global.** A Mix with nothing falls through to
Deezer rather than disabling autoplay. That is the entire point of the crate.

## 4. Per-provider policy

Each provider owns its own limit discipline; the orchestrator does not
generalize them, because they are genuinely different failures.

| provider | discipline |
|---|---|
| musicatlas | local daily counter (no headers exist to read). Default budget **90** of 100, leaving headroom for manual probes. Exhaustion = skip to next recommender, not an error. |
| YouTube's Mix | one yt-dlp run per refill, killed after **30s**. No quota. A yt-dlp that fails or prints something other than a playlist is an error for that refill, never an empty Mix. |
| Deezer | no key. Errors arrive in **HTTP 200** bodies; the documented quota error (code 4) is a rate limit, any other error body is an error. 429 and 5xx are rate limits. |
| ReccoBeats | *(removed in v0.12.0)* reactive: honor **429 + `Retry-After`**, back off for that long, skip provider meanwhile. Limits are undisclosed so nothing can be pre-computed. |
| MusicBrainz | proactive: **hard 1 req/sec** throttle, plus the mandatory descriptive User-Agent. Exceeding it gets the bot's IP blocked, which is not a per-guild failure but an estate-wide one. |

## 5. Measured API contracts

### 5.1 musicatlas

`POST https://musicatlas.ai/api/similar_tracks`, `Authorization: Bearer <key>`,
body `{"artist","track"}`. Returns `{success, matches[20], hashtags,
source_track}`; every match carried `platform_ids.youtube`.

🪤 **Logical failures are HTTP 200.**

| condition | HTTP | body |
|---|---|---|
| ok | 200 | `{"success":true,"matches":[…]}` |
| unknown track | **200** | `{"success":false,"error":"That doesn't appear to be a released track."}` |
| missing field | **200** | `{"success":false,"error":"Missing artist or track"}` |
| bad key | 403 | `{"error":"Invalid or unconfirmed API key"}` |

**Key the taxonomy on `success`, never on the status.**

🪤 **It gates on User-Agent.** `Python-urllib` is refused **403 with the same
body as a bad key**. An explicit UA is mandatory, or a first integration looks
exactly like a bad credential.

### 5.2 ReccoBeats (removed in v0.12.0)

Kept for the record. See §2: its recommendations are audio-feature neighbours
with no listening data, measured useless on production.

Base `https://api.reccobeats.com/v1`, no auth.

**Two calls, not one.** Recommendation seeds are ReccoBeats UUIDs; Spotify ids
are rejected (`4002 Cannot find any track for given seed ids`), and
`/v1/track?ids=<spotify id>` returns `{"content":[]}`.

1. `GET /v1/track/search?searchText=<title>&size=50` → the `id` of the first
   `content[]` entry whose `artists[].name` matches the seed artist (normalized)
2. `GET /v1/track/recommendation?size=<n>&seeds=<uuid>` → `content[]`

🪤 **`searchText` matches track titles only.** This spec originally said
`<artist title>&size=1`; measured on production v0.11.0 (2026-09-13), that
finds nothing for any seed ("Queen Bohemian Rhapsody" → `[]`, "Bohemian
Rhapsody" → 50 results). A title search returns every song of that name, hence
the artist match; `size` is capped at 50 (`size=100` → 400 "must be less than
or equal to 50"). Coverage is partial: The Offspring's "Hit That" and Guns N'
Roses' "Sweet Child O' Mine" match at index 0, but Queen's "Bohemian Rhapsody",
Daft Punk's "Get Lucky" and Kendrick Lamar's "Love" are not in the first 50.

Each result: `{id, trackTitle, artists[{name,href}], durationMs, isrc, href}`.
`href` is a Spotify URL. **No YouTube id**, hence `Playable::SearchQuery`,
built as `"{artist} - {title}"`.

🪤 The two-call shape doubles ReccoBeats' request count per recommendation and
its limits are undisclosed. Cache the search result (`seed → uuid`) separately
from the recommendations so a repeat seed costs one call, not two.

### 5.3 MusicBrainz

`GET https://musicbrainz.org/ws/2/recording?query=…&fmt=json&limit=N`, no key,
**descriptive User-Agent mandatory** (contact address included), **1 req/sec**.

Query `artist:<a> AND recording:"<t>"`; take `recordings[0]`'s `score`,
`artist-credit[0].name` and `title` as the canonical seed.

### 5.4 YouTube's Mix (v0.12.0)

Every video has an auto-generated Mix playlist, `watch?v=<id>&list=RD<id>`.

```
yt-dlp --flat-playlist --no-warnings -J --playlist-items 1:<want+1> \
  'https://www.youtube.com/watch?v=<id>&list=RD<id>'
```

Measured 2026-09-13 with yt-dlp 2026.08.19 (the production container runs the
same version): 1-2 seconds. The output is `{"_type":"playlist", "entries":[…]}`,
each entry `{id, title, channel, uploader, duration, url}`, and **the first
entry is the video itself**. An unplayable id exits 1 with
`ERROR: [youtube] <id>: This video is unavailable` on stderr and `null` on
stdout.

Results, for the fan upload "The Offspring ~ Hit That": more Offspring, then
blink-182, Sum 41, Linkin Park, Papa Roach, System Of A Down. For Queen's
"Bohemian Rhapsody", which ReccoBeats' title search could not even find:
Queen, Aerosmith, Kansas.

🪤 The video id reaches yt-dlp inside a URL. It is validated as exactly 11 of
`[A-Za-z0-9_-]` first, so an `&` or `#` cannot rewrite that URL.

🪤 rusty_ytdl's `VideoInfo.related_videos` is empty on the pinned fork: its
parser no longer finds YouTube's related list, so it is not an alternative.

### 5.5 Deezer (v0.12.0)

Base `https://api.deezer.com`, no auth.

1. `GET /search/artist?q=<artist>&limit=10` → of the entries whose `name`
   matches the seed artist (normalized), the one with the most fans (`nb_fan`);
   never simply the first

🪤 **Many artists share a name, and the order is not popularity.** Measured:
`q=Queen` returns four exact "Queen"s with 131, 344, 7 and 12,800,469 fans, the
band fifth overall; `q=Genesis` returns seven, the band (1,190,757 fans) eighth.
The first exact "Queen" has a radio of one track, and it is not Queen's.
2. `GET /artist/<id>/radio?limit=<want+1>` → `data[]`, each
   `{id, title, title_short, duration, artist{id,name}, …}`, no ISRC

Measured for The Offspring (id 882): radio plays The Offspring, Rage Against
the Machine, blink-182, Green Day, Foo Fighters; `related` lists Green Day,
Sum 41, Bad Religion, blink-182, Rise Against, NOFX. `limit` is honoured
(5, 25, 100).

🪤 **Errors arrive as HTTP 200**, and `code` is not always present:

| request | response |
|---|---|
| `artist/999999999999/radio` | `200 {"error":{"type":"DataException","message":"no data","code":800}}` |
| `artist/0/radio` | `200 {"error":{"type":"Exception","message":"An error has occured"}}` |
| `search/artist?q=` | `200 {"error":{"type":"ParameterException","message":"empty parameter","code":500}}` |
| `search/artist?q=<nonsense>` | `200 {"data":[],"total":0}` |

The quota error, code 4 "Quota limit exceeded", is documented but not measured.

## 6. Seed derivation

Order: `AuxMetadata.artist` if present → else parse the title.

Split on the first of `" - "`, `" – "` (**EN DASH**), `" — "` (EM DASH),
`" | "`. Left = artist, right = title. Strip from the title:
- bracketed noise matching `(official|video|audio|lyric|remaster|hd|4k|mv|visualizer)`
- a trailing `ft.` / `feat.` clause

🪤 **The en dash is not a hyphen.** The Queen title uses `–`; a splitter that
only knows `-` silently yields no seed.

**No separator ⇒ no seed ⇒ no metered call.** Measured: 3/3 real music videos
parsed and returned 20 musicatlas matches; `lofi hip hop radio - beats to
relax/study to` parsed but returned `success:false` (1 call wasted);
`Never Gonna Give You Up` was skipped for free.

MusicBrainz then scores the parsed pair. `confidence < min_seed_confidence`
(default 80) ⇒ skip metered providers; YouTube's Mix and Deezer may still be tried.

## 7. Caching

One table, provider-tagged, **negative results included**:

```sql
CREATE TABLE musicreco_cache (
    provider   TEXT        NOT NULL,
    artist     TEXT        NOT NULL,   -- normalized
    track      TEXT        NOT NULL,   -- normalized
    results    JSONB       NOT NULL,   -- serialized Vec<Recommendation>; [] when none
    found      BOOLEAN     NOT NULL,
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, artist, track)
);

CREATE TABLE musicreco_budget (
    provider TEXT    NOT NULL,
    day      DATE    NOT NULL,
    calls    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (provider, day)
);
```

Normalization for keys only: trim, collapse internal whitespace, lowercase. The
un-normalized values are what get sent to the provider.

🔑 **`found = false` is cached.** The lofi seed cost a call and returned
nothing; without a negative row it costs one every time that stream ends.
Negative caching is a quota feature.

Budget is incremented in the same transaction as the cache write, so a crash
between the call and the bookkeeping cannot under-count a quota that no header
reports back.

`results` is `JSONB` because the database never queries into it -- it is
written and read only as a serialized `Vec<Recommendation>`. The workspace's
typed-serde rule governs the Rust side; there is no `serde_json::Value` in the
domain types.

## 8. Bot integration

`track_end.rs` replaces `get_recommended_track_query` with a call to
`MusicReco::next_tracks`, keeping the existing per-guild buffer idea:

1. Track ends, autoplay on, queue empty.
2. Buffer non-empty ⇒ pop → step 6.
3. Build `RawTrack` from the ended track's `AuxMetadata` (in-process, no DB).
4. `next_tracks(raw, 20)`; empty ⇒ announce autoplay off, stop.
5. Fill the guild buffer with the result.
6. `Playable::YouTubeId` ⇒ a direct `QueryType`; `Playable::SearchQuery` ⇒ the
   existing search path. Hand to `queue_query` unchanged.

Every path that stops autoplay announces, via the existing
`announce_autoplay_off`. Silent stopping is the behaviour being fixed.

## 9. Error handling

| condition | behaviour |
|---|---|
| no seed derivable | autoplay off + announce; no calls |
| seed confidence below floor | skip metered providers; try free ones |
| musicatlas `success:false` | negative-cache; next recommender |
| musicatlas 403 | log ERROR, disable that provider for the process, next recommender |
| Deezer 429, 5xx, or quota error (code 4) | rate limited: fall through to the next provider |
| yt-dlp missing, failing, hung, or not a playlist | error for this refill: fall through to Deezer |
| MusicBrainz any failure | fall back to the unverified parsed seed |
| all recommenders exhausted | autoplay off + announce |
| result lacks a playable form | skip it, take the next from the buffer |

## 10. Testing

- **Per provider:** taxonomy tests against a local `TcpListener` serving canned
  bodies -- including musicatlas' **HTTP 200 + `success:false`**, which is the
  case a status-only client gets wrong, and Deezer's error bodies inside **HTTP 200**.
  Assert on **request counts**, not just returned values: a cache or budget that
  silently still calls returns the right answer.
- **Parsing:** table-driven over §6's measured titles, including the en dash and
  the no-separator skip.
- **Orchestration:** a fake provider pair proves fallback order, that a failing
  first recommender does not disable autoplay, and that an exhausted budget
  skips rather than errors.
- **Live:** one manual end-to-end run on TuneTitan before production. Seed
  quality is empirical and cannot be settled offline.

## 11. Success criteria

1. A track ends with autoplay on and a related track plays, on TuneTitan.
2. A second track-end in the same chain plays with **no** new API call.
3. With musicatlas' budget forced to 0, autoplay still works via YouTube's Mix
   and Deezer.
4. A no-separator title stops autoplay with a message and spends no quota.
5. `musicreco_cache` shows both `found=true` and `found=false` rows.
6. MusicBrainz calls never exceed 1/sec.

## 12. Out of scope

- `recommendPlaylists` (musicatlas): returns playlist metadata only -- no
  tracks, no platform ids. It is the right endpoint for a future mood/tag
  command, not for autoplay.
- Reviving Spotify recommendations.
- Backfilling the cache from existing play history.
- Any cross-provider join on ISRC (§2.2: measured 404).
