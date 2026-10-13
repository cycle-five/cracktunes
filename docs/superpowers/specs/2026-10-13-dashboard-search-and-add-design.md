# Dashboard search and add — design

**Date:** 2026-10-13 · **Status:** approved in conversation (sections 1–3 reviewed one by one; the owner then said "continue on through to code review", so sections 4–5 were written without a separate stop)

## Goal

A member on the dashboard can find a track and put it in the queue without
typing a slash command. Search is general: one query runs over every source
the bot implements, and every hit, whatever its source, is a track that can be
added.

## Owner decisions (brainstorm, 2026-10-13)

| Question | Decision |
|---|---|
| What is searched | Everything implemented: "a track search returns a track", over the queue, history, YouTube and SoundCloud |
| Surfaces | Dashboard now. The seam lives in crack-core so Discord `/search` and `/play` autocomplete can adopt it later |
| Access | Search: any member who can view the guild (rate-limited). Add: like the controls — in the bot's voice channel **and** premium. Free servers see Add disabled with the Patreon note |
| Idle bot | Add works only while the bot is connected in the member's voice channel. Summoning from the web is a later arc |
| History | Tracks only (no member names), any viewer, one row per track with play count and last-queued time. Window follows the plan: free = `FREE_HISTORY_WINDOW` (24 h), premium = all |
| Sources this arc | Queue, History, YouTube, SoundCloud. **Not** pasted links, saved playlists or Spotify (sleevenote resolves links only; it has no search) |
| Position | Two actions per hit: Add (end) and Play next |
| How add names a track | **Server-held hits**: search stores what it found; add takes an opaque hit id. The client never supplies a URL |

## 1. The search seam (crack-core)

New module `crack-core/src/music/search/`: `mod.rs` plus one file per source
(`queue.rs`, `history.rs`, `youtube.rs`, `soundcloud.rs`).

```rust
pub enum Source { Queue, History, YouTube, SoundCloud }   // fixed display order
pub struct SourceSet(..);                                 // a set of Source

pub struct Query(String);   // the one constructor validates: trimmed, 2..=100 chars, no control chars

pub struct Hit {
    pub track: SavedTrack,  // url, title, artist, duration: exactly what add enqueues
    pub detail: HitDetail,
}
pub enum HitDetail {
    Queue { position: usize },                    // 0 = playing
    History { plays: u32, last: DateTime<Utc> },
    Found,                                        // YouTube, SoundCloud
}
pub enum SourceResult { Hits(Vec<Hit>), Hidden, NoDatabase, Failed, TimedOut }

pub async fn search(
    data: &Data,
    guild: GuildId,
    query: &Query,
    sources: SourceSet,
    history_since: Option<DateTime<Utc>>,
) -> Vec<(Source, SourceResult)>;
```

- **Every hit is a `SavedTrack`**, the type queue resume (#595) already plays
  from through `ResolvedTrack::from_saved`. One representation, every source.
- **Grouped, never merged.** Results come back per source in the fixed order;
  nothing ranks a YouTube relevance against a history recency.
- **Concurrent and isolated.** Sources run together; each has its own timeout
  (`LOCAL_TIMEOUT` = 2 s for queue/history, `EXTERNAL_TIMEOUT` = 8 s for
  YouTube/SoundCloud) and a cap of `HITS_PER_SOURCE` = 10. One source failing
  or timing out never fails the others; each failure is logged at `warn` with
  its reason (degrade gracefully; do not chase intermittents).
- **Only http(s) URLs become hits.** A hit with any other URL is dropped: it
  could not be shown as a link nor safely played.
- **External text only behind a search prefix.** YouTube via
  `YoutubeDl::new_search` (`ytsearchN:`), SoundCloud via
  `YoutubeDl::new(…, "scsearchN:{q}")`. The prefix means the member's text can
  never start yt-dlp's argument, so it can never be an option (the v0.17.2
  injection class). A 🔒 test pins both forms.
- **A bot-wide semaphore** (`EXTERNAL_SEARCHES` = 4 permits) caps concurrent
  external searches, so web search cannot hammer YouTube from the residential
  IP. Waiting for a permit counts against the source's timeout.

Per source:
- **Queue** — `remote::queue_state`, filtered case-insensitively on title and
  artist. `Hidden` while a `/gp` game owns playback (the queue would give the
  answers away). Needs no network.
- **History** — `play_log ⨝ metadata` for this guild, `created_at ≥ since`
  when `since` is given, title or artist `ILIKE` the query with `\`, `%` and
  `_` escaped, grouped by `source_url`, `count(*)` and `max(created_at)`,
  newest first, `LIMIT 10`. No member columns are read. `NoDatabase` when the
  bot runs without a pool. `play_log` holds only `/play`-path adds (and, after
  this arc, dashboard adds); `/gp` never writes it, so history search cannot
  reveal a running game.
- **YouTube** — rusty_ytdl `search` (limit 10, video results only), whose
  results already carry title, duration and URL; on a rusty_ytdl error, yt-dlp
  `ytsearch10:`.
- **SoundCloud** — yt-dlp `scsearch10:`. Playback already sorts SoundCloud to
  MP3 (`SOUNDCLOUD_FORMAT_SORT`, v0.20.1).

**Not in this arc:** an index on `play_log (guild_id, created_at)`. The query
is bounded by `LOCAL_TIMEOUT`; measure it on production after release and add
the index (a migration, which also raises the migrate image's rollback floor)
only if it is slow.

## 2. The add op (crack-core)

`ops::add` closes the one gap the ops layer left ("every mutator except add").

```rust
// crack-core/src/music/ops/add.rs
pub enum At { End, Next }
pub struct Added { pub title: Option<String> }
pub async fn add(cx: &OpCx, track: SavedTrack, at: At) -> Result<Done<Added>, OpRefused>;
```

- **Lease through `ops::begin`**, like every op: a running game is
  `GameInProgress`, no call is `NotConnected`. A bot connected in the member's
  channel with an empty queue is still "in your voice channel": add works and
  starts playback.
- **The requester is the member**: `ResolvedTrack::from_saved(&t)` (which
  deliberately carries no requester, for `/gp`) then `.with_user_id(user)`.
- **End** goes through `enqueue_resolved_tracks_back`. **Next** is the same
  enqueue, then the new track is moved to index 1 under the same call lock,
  with `/playnext`'s rule (only when something sits between). Audited
  `AddAt::Back` / `AddAt::Front`, as `/play` and `/playnext` are.
- **Clears the guild's autoplay buffer**, as `/play` does, so a pre-picked
  autoplay track cannot jump ahead of the member's pick.
- **Writes `play_log`**: a `MetadataMsg` on `data.db_channel` (requester =
  the member), so a dashboard add shows up in later history searches. Today
  only the poise-context `/play` paths do this.
- **Settles `QueueMessages`**, as the queue-changing ops do. When the add
  starts playback from an empty queue, the now-playing status follows from
  the track-start event, exactly as it does for `/play`; the op adds nothing
  for it.

`remote::add(data, http, cache, guild, user, via, track, at) ->
Result<Option<Echo>, AddRefused>`, modelled on `remote::control`: the lease
covers the op only; the echo and the settle run in the background, the settle
anchored after the echo. New `Echo` variants `Added { title }` and
`AddedNext { title }` render "➕ @member added *Title* (dashboard)" and
"⏭ … to play next"; gated by `control_echoes` like every echo; strings in
`messages.rs`; titles clipped and escaped as the others are.
`AddRefused { NotPlaying, GameInProgress, Failed }`.

`ops::add` is the one new caller of the enqueue primitives. No new
`pub async fn enqueue_*`, so the pinned-surface test and `clippy.toml` stand.
`/play` does not move onto `ops::add` in this arc (that is the "Discord later"
step).

## 3. Web routes and the hit memo (crack-web)

**`GET /g/{g}/search?q=…&src=queue,history,youtube,soundcloud`**
1. Session, else 401.
2. Membership via `decide(presence)`: `Hidden` 404, `Unavailable` 503. Any
   viewer may search.
3. Parse `q` (as `Query`) and `src` (comma-separated, at least one, no
   unknowns), else 400 with an `ErrorBody`.
4. `search_limiter`: a second `RateLimit` (`SEARCHES_PER_WINDOW` = 5 per
   `SEARCH_WINDOW` = 30 s, per user), separate from the controls limiter so
   searching never spends Skip. Else 429.
5. The plan via `plan_or_free`, for the history window
   (`Plan::history_floor`).
6. `backend.search(...)`, then the hits are stored in the memo and returned:

```rust
SearchResult { sources: Vec<SourceView> }
SourceView   { source: Source, status: Ok|Hidden|NoDatabase|Failed|TimedOut, hits: Vec<HitView> }
HitView      { id: Uuid, title: String, artist: Option<String>, duration_secs: Option<u64>,
               url: String, detail: HitDetailView }
HitDetailView = tag "kind": queue { position } | history { plays, last } | found
```

A GET, as `history.json` is: a forged cross-site GET can at most spend one of
the victim's rate-limited searches, and it reads nothing back.

**`HitMemo`** (`crack-web/src/hits.rs`):
`DashMap<(UserId, GuildId), (Instant, HashMap<Uuid, SavedTrack>)>`.
- Each search **replaces** that user's hits for that guild. Memory is bounded
  by active searchers × 40 hits, and stale ids stop resolving as soon as a new
  search lands.
- `HIT_TTL` = 10 minutes. Swept like `RateLimit`: past a size threshold, an
  insert first drops expired entries.
- Hit ids are random v4 UUIDs, resolvable only by the same user **and** guild:
  an id leaked or replayed from another tab, user or server finds nothing.
- A lookup does not consume the hit: adding the same track twice is allowed.

**`POST /g/{g}/add`**, body `{ "hit": Uuid, "at": "end" | "next" }`
(`deny_unknown_fields`)
1. The same gates as `/control`, in its order: session (401), JSON content
   type (415), `Origin` (403), body (400), `Access::Control` (403
   `not_allowed`), the controls limiter (429 `too_many`), premium (403
   `premium_required`).
2. The memo lookup: a miss is **410 Gone** `expired`.
3. `backend.add(...)` → `remote::add`.
4. `AddResult::{ Added { view }, Expired, NotAllowed, PremiumRequired,
   GameInProgress, NotPlaying, Failed, TooMany }`; on `Added`, a hub refresh
   so open tabs follow over SSE.

`Backend` gains `search` and `add`; `LiveBackend` calls the crack-core seam,
`FakeBackend` drives the route tests. `routes.rs` is already ~1,900 lines, so
the two handlers and their wire types live in a new `crack-web/src/find.rs`
(as `history.rs` holds its own), registered from `routes::router`.

## 4. The page

- A **Find a track** panel on the queue page, below the controls and above
  Now playing: a search `input` with a Search button inside a `<form>`
  (submit only — Enter or the button; never search-as-you-type), and four
  source checkboxes styled as the history page's chips.
- Source choice is remembered per browser in `localStorage`, every access in
  try/catch; default all four on.
- Results: one block per source, in the fixed order, each with a heading and
  either its hits or a status line ("No matches", "Hidden while a Guilty
  Pleasure game is on", "No history on this bot", "YouTube did not answer",
  "SoundCloud took too long"). On a free server the History block notes "Free
  servers search the last 24 hours."
- A hit row reuses the queue's `.track` look: title (a link, http(s) only,
  new tab, `noopener noreferrer`), meta (artist · duration · detail: "#3 in
  queue" / "now playing" / "queued 4× · 3 d ago"), then **＋ Add** and
  **⏭ Next** buttons.
- The buttons are enabled from the **live** page state (the SSE stream's
  `can_control` and `plan`), not from the search answer, so joining or
  leaving voice updates them without searching again. Disabled reasons reuse
  the existing notes: the Patreon line on free servers, "Join the bot's voice
  channel…" otherwise, and the game note while the queue is hidden.
- On `added`: the note says "Added *title*." (or "…to play next"), the
  queue redraws from the answer's view. On `expired`: "Those results
  expired — search again." and the results are cleared. Other answers map to
  the same notes `control` uses.
- New script `crack-web/assets/find.js`, served by `asset()` and loaded only
  on the queue page. Every node via `createElement` + `textContent`; the CSP
  (Trusted Types, no inline script) is unchanged. It reads the live state
  through a small hook `app.js` exposes (`window.crackDash.state()` and an
  `onState` listener), not by re-parsing the page.

## 5. Errors and testing

**Errors.** Each source reports its own status; the page always renders
whatever answered. A search with every source failed is still a 200 with
statuses, not a 5xx. Add refusals map one-to-one to `AddResult`. Logs: one
`info` per add (`guild`, `user`, `source`, `at`), one `warn` per failed or
timed-out source with its reason. No title or query text at `info`.

**Tests** (TDD; every test watched failing first, and its claim sabotaged
once — the owner's standing rule):
- `Query`: bounds, trimming, control characters.
- Queue source: case-insensitive title/artist match, positions, `Hidden`
  during a game, cap of 10, non-http URLs dropped.
- History source (`db-tests`): guild scoping (another guild's plays never
  appear), the `since` window, `%`/`_`/`\` escaping, grouping with play count
  and last time, `LIMIT`, no database → `NoDatabase`.
- 🔒 External search arguments: the YouTube and SoundCloud yt-dlp invocations
  carry the query only after `ytsearch10:` / `scsearch10:`, including a query
  starting with `-`.
- `search`: one source timing out or failing leaves the others' hits; fixed
  order; the semaphore bounds concurrency.
- `ops::add` on an offline `standalone_call`: lease refusals (game,
  not connected), End appends, Next lands at index 1 (and stays last when
  nothing is between), requester is the member, audit row `Add` with the
  right `AddAt`, autoplay buffer cleared, a `MetadataMsg` sent.
- `Echo::Added`/`AddedNext` lines: clipped, escaped, `(untitled)` for blank.
- `HitMemo`: TTL, replace-on-new-search, user **and** guild binding, sweep.
- Routes (fake backend): every gate in order for both routes, status codes
  and result tags, the 410, the memo filled by search and read by add, the
  search limiter independent of the controls limiter, the history window
  passed for free vs premium.
- Live-network tests (a real YouTube or SoundCloud search) are `#[ignore]`d,
  per the standing rule.

**Manual (TuneTitan, before merge):** search each source; add and play-next
from YouTube, SoundCloud, history and queue hits; free server sees Add
disabled; outside voice sees Add disabled; a `/gp` game hides queue results
and refuses add; results expire after 10 minutes; the echo line posts and
respects `/echoes`.

## Shipping

One PR, `feat/dashboard-search-add`, **v0.27.0** (minor: a feature). The
brainstorm floated three stacked PRs; one PR with a commit per plan task was
chosen at spec time instead: the crack-core half ships nothing visible on its
own, the owner tests one branch image on TuneTitan, and stacking has bitten
merges before (a base merged with `--delete-branch` closed its child). No migration, so rollback to
v0.26.0 is the bot image alone.

## Follow-ups (not this arc)

- Discord `/search` and `/play` autocomplete on the seam.
- Pasted links (single tracks, then playlists), saved playlists, Spotify
  search (needs sleevenote work), likes.
- Summoning the bot from the web.
- Streaming results per source as each answers, instead of one answer bounded
  by the slowest source.
- The `play_log (guild_id, created_at)` index, if production says so.
