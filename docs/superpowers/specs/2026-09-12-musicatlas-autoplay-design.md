# Autoplay on musicatlas.ai — design

**Status:** approved in chat 2026-09-12, not yet implemented.
**Replaces:** the Spotify-recommendations path in `track_end.rs`, which is dead.

## 1. Why

Autoplay has been broken on production for some time, and not for the reason
the code suggests. `get_recommended_track_query` checks Spotify auth *before*
it reads play history:

```rust
let spotify = verify(spotify.as_ref(), CrackedError::SpotifyAuth)?;   // fails here
let last_played = pool.get_last_played_by_guild(guild_id, 5).await?;  // never reached
```

Production has no Spotify client credentials, and `SPOTIFY_DISABLED_LOG`
records that Spotify blocked new Web API app creation around 2025-12 — so this
is very likely unfixable by supplying credentials. Autoplay needs a different
recommendation source, not a repair.

🪤 An earlier note in ct#486 claimed empty `play_log` was the cause. That was
wrong, and the correction matters here: fixing play history did **not** and
will not revive autoplay on its own.

## 2. The source: `getSimilarTracks`

`POST https://musicatlas.ai/api/similar_tracks`, `Authorization: Bearer <key>`,
body `{"artist": "...", "track": "..."}`.

**Everything below was measured against the live API on 2026-09-12**, not read
off the docs.

Returns `{ success, matches[], hashtags[], source_track }`. **20 matches per
call**, and in the observed responses **every match carried
`platform_ids.youtube`** — a video id the bot can play directly, with no search
step.

### 🪤 Logical failures are HTTP 200

| condition | HTTP | body |
|---|---|---|
| success | 200 | `{"success":true,"matches":[...20...]}` |
| unknown track | **200** | `{"success":false,"error":"That doesn't appear to be a released track."}` |
| missing field | **200** | `{"success":false,"error":"Missing artist or track"}` |
| bad/unconfirmed key | 403 | `{"error":"Invalid or unconfirmed API key"}` |

**The client MUST key its taxonomy on the `success` field, not on the HTTP
status.** A client that checks only the status treats "not a released track" as
a success and then finds `matches` empty — the exact silent-failure shape that
`crack-sleevenote`'s taxonomy exists to prevent.

### 🪤 The API gates on User-Agent

A request with Python's default `User-Agent: Python-urllib/3.x` is rejected
**403**, with the same body as a bad key. The identical request with
`User-Agent: cracktunes/0.9.5` succeeds. The client must send an explicit
User-Agent; `crack-sleevenote::default_user_agent()` is the precedent to copy.
Without this, a first integration looks exactly like a bad API key.

### Quota

Free tier: **100 requests/day**, rolling 24h. **No rate-limit headers are
returned**, so remaining budget cannot be read from a response — it must be
counted locally. 100 calls × 20 tracks = 2,000 autoplay tracks/day, which is
ample *only* because of §4's one-call-per-chain rule.

## 3. The seed problem, measured

The API needs `artist` and `track` separately. **yt-dlp supplies neither.**
Measured on three typical music videos:

| `artist` | `track` | `uploader` | `title` |
|---|---|---|---|
| NA | NA | Guns N' Roses | `Guns N' Roses - Sweet Child O' Mine (Official Music Video)` |
| NA | NA | Stone Temple Pilots | `Stone Temple Pilots - Reds & Blues (Official Art Video)` |
| NA | NA | Queen Official | `Queen – Bohemian Rhapsody (Official Video Remastered)` |

So **title parsing is the primary path, not a fallback.** `AuxMetadata.artist`
is used when present and parsing is the rule otherwise.

### Parsing rules

Split on the first of `" - "`, `" – "` (EN DASH), `" — "` (EM DASH), `" | "`.
Left is artist, right is track. Then strip from the track:

- parenthetical/bracketed noise matching
  `(official|video|audio|lyric|remaster|hd|4k|mv|visualizer)`
- a trailing `ft.` / `feat.` clause

🪤 **The en dash is not a hyphen.** The Queen title above uses `–`; a splitter
that only knows `-` fails on it and silently produces no seed.

**No separator means no call.** A title like `Never Gonna Give You Up` yields no
artist, and guessing burns quota on a request we already know cannot match.

Measured end-to-end with these rules: 3/3 real music videos parsed and returned
20 matches; `lofi hip hop radio - beats to relax/study to` parsed but correctly
returned `success:false`; `Never Gonna Give You Up` was skipped with no call.

## 4. Architecture

### 4.1 `crack-musicatlas`, a new crate

Mirrors `crack-sleevenote`: typed client, typed error taxonomy one variant per
failure condition, `ClientBuilder`, env-configured base URL/key/timeout,
explicit User-Agent. The user's stated reason for a separate crate is that the
recommendation source is expected to change — a crate boundary makes swapping
it a dependency change rather than a refactor.

Typed structs per the workspace rule; no `serde_json::Value` for data we own.

```rust
pub struct SimilarTracks { pub matches: Vec<Match>, pub hashtags: Vec<String> }
pub struct Match { pub artist: String, pub title: String, pub platform_ids: PlatformIds }
pub struct PlatformIds { pub youtube: Option<String>, pub spotify: Option<String>, /* … */ }
```

Error variants: `NotAReleasedTrack`, `MissingField`, `InvalidKey`,
`Transport`, `Decode`, `UnexpectedBody`. `NotAReleasedTrack` is a permanent
fact about the seed and must never be retried; `Transport` may be.

### 4.2 One call per chain, not per track

A call yields 20 playable tracks. Autoplay holds a **per-guild buffer** of
pending recommendations, pops one per track-end, and only calls the API when
the buffer empties. This is what makes the free tier viable: ~20× fewer calls
than the naive per-track design.

The buffer lives in `Data` beside the existing per-guild maps, keyed by
`GuildId`, and is dropped when autoplay is turned off or the bot leaves.

### 4.3 Cache, including negative results

Postgres table `musicatlas_similar`, keyed on the **normalized** `(artist,
track)` seed:

```sql
CREATE TABLE musicatlas_similar (
    artist      TEXT        NOT NULL,
    track       TEXT        NOT NULL,
    matches     JSONB       NOT NULL,   -- serialized Vec<Match>; [] when not found
    found       BOOLEAN     NOT NULL,
    fetched_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (artist, track)
);
```

🔑 **`found = false` rows are cached too.** `lofi hip hop radio` cost a call and
returned nothing; without a negative entry the same seed costs a call every
time that stream ends. Negative caching is a quota feature, not a nicety.

`matches` is `JSONB` because it is an opaque blob to the database — it is
written and read only as a serialized `Vec<Match>`, never queried into. The
typed-struct rule still governs the Rust side.

### 4.4 Budget

A daily counter of calls made, persisted so a restart cannot reset it into an
overspend. When the budget is exhausted, autoplay does **not** silently stop:
it turns off for that guild and announces why, reusing the existing
`announce_autoplay_off` path, which already exists precisely because silent
disabling was a past complaint.

Default budget 90, below the 100 limit, leaving headroom for manual probes.
Configurable by env var.

## 5. Data flow

1. Track ends; autoplay is on; the queue is empty.
2. If the guild's buffer is non-empty, pop a recommendation → step 6.
3. Derive `(artist, track)` from the ended track's `AuxMetadata` (§3). No seed
   derivable → announce autoplay off, stop. No call.
4. Cache hit → fill buffer from it (or stop, if `found = false`) → step 6.
5. Cache miss → if budget remains, call the API; write the result to the cache
   either way; increment the counter. Budget exhausted → announce and stop.
6. Turn the chosen match into a `QueryType` from `platform_ids.youtube`, and
   hand it to the existing `queue_query` path unchanged.

## 6. Error handling

| condition | behaviour |
|---|---|
| no seed derivable | autoplay off + announce; **no API call** |
| `NotAReleasedTrack` | negative-cache; autoplay off + announce |
| `InvalidKey` | log at ERROR; autoplay off + announce; do not retry |
| `Transport` | one retry, then autoplay off + announce |
| budget exhausted | autoplay off + announce; no call |
| match has no `youtube` id | skip that match, take the next from the buffer |

Every path that stops autoplay announces. Silent stopping is the behaviour this
design exists to avoid.

## 7. Testing

- **Crate:** taxonomy tests against a local `TcpListener` serving canned
  bodies, including the **HTTP 200 + `success:false`** case, which is the one a
  status-only client gets wrong. Same technique as `crack-sleevenote`'s retry
  tests: assert on request counts, not just return values.
- **Parsing:** table-driven over the measured titles in §3, including the en
  dash and the no-separator skip.
- **Budget/cache:** a call with a warm cache makes no request; a `found=false`
  entry makes no request; an exhausted budget makes no request. Each asserted
  by request count.
- **Live:** one manual end-to-end run on TuneTitan before production, since the
  seed quality question is empirical and cannot be settled offline.

## 8. Success criteria

1. A track ends with autoplay on, and a related track plays, on TuneTitan.
2. A second track-end in the same chain plays without a new API call.
3. A seed with no separator stops autoplay with a message and spends no quota.
4. `musicatlas_similar` shows both a `found=true` and a `found=false` row.
5. Daily calls stay under the configured budget across a real session.

## 9. Deliberately out of scope

- `recommendPlaylists`. Considered and rejected for autoplay: it returns
  playlist metadata only — no tracks, no `platform_ids` — so it needs a second
  resolution mechanism. It is the right endpoint for a future "play me
  something jazzy" tag/mood command, which this design does not build.
- Reviving Spotify recommendations.
- Backfilling `musicatlas_similar` from existing play history.
