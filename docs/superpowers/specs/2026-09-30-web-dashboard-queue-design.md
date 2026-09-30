# Web dashboard, arc 1: view the queue and reorder it

**Date:** 2026-09-30
**Target version:** 0.15.0 (cracktunes), 0.1.0 (catacombs)
**Status:** approved in conversation (sections 1–4 by the owner); ready for planning

## Goal

Guild members open a web page, sign in with Discord, and see what the bot is
playing in their server and what is queued behind it, updating live. Members
who are **in the voice channel the bot is playing in** can reorder the upcoming
queue by dragging. That is the whole arc: the first control is reordering, and
nothing else.

This is the first arc of a wider web dashboard. Its choices (an embedded
server, a typed JSON/SSE boundary, catacombs for auth) are meant to carry later
arcs — more controls, settings, playlists — without rework.

## Owner decisions

1. **Audience: guild members**, not an operator console. Public URL, Discord
   login.
2. **Viewing:** any member of the guild. **Controlling:** only a member whose
   voice state is in the bot's voice channel in that guild.
3. **Controls in this arc: reorder only.** `/movesong <at> <to>` already exists;
   the dashboard reuses its queue operation.
4. **Approach A:** embedded in the bot process, server-rendered HTML, SSE for
   live updates, vendored SortableJS for drag-and-drop. No Node toolchain.
5. **Auth through catacombs** (`cycle-five/catacombs`), the owner's Discord OAuth
   library, extended first to support a browser redirect flow and cookie
   sessions. Consumed as a **tag-pinned git dependency**, the same way
   `crack-osint` and `crack-bf` are; publishing catacombs to crates.io is out of
   scope for this arc.
6. **Hostname: `dash.cracktun.es`**, the product's own domain.
7. **A web reorder is silent in Discord but refreshes the queue messages**, as
   `/movesong` does. No "X moved Y" post.

## Architecture

### A new workspace crate: `crack-web`

Depends on `crack-core`. `crack-cli` starts it beside poise on the same tokio
runtime, handing it the same `Data`, serenity `Cache` and `Http`, and songbird
manager. It sits behind a `crack-cli` cargo feature, `web`, **on by default**, so
the bot still builds and runs without it.

| Unit | Does | Depends on |
|---|---|---|
| `config` | Reads the web env vars into a typed config, or reports which are missing | env |
| `access` | `can_view(user, guild)`: a member (see Auth and access). `can_control(user, guild)`: the user's cached voice state is in the bot's voice channel for that guild | serenity `Cache` + `Http`, through a pure decision function |
| `view` | The wire types below, `rev`, and the conversion from crack-core's `QueueState` | `crack_core::music::remote` |
| `watch` | One `tokio::sync::watch` channel per guild, alive only while subscribed; snapshots about once a second, publishes only on change | `crack_core::music::remote` |
| `routes` | Pages, `GET /g/{id}/events` (SSE), `POST /g/{id}/move` | all of the above |
| `assets` | HTML templates, CSS, SortableJS, our JS — compiled in with `include_str!` | — |

`crack-web` mounts catacombs' router at `/auth` and uses its
`AuthenticatedUser` extractor. It builds catacombs' `Config` itself rather than
calling `Config::from_env`, whose variable names do not match cracktunes'
(`DISCORD_BOT_TOKEN` vs `DISCORD_TOKEN`).

### One addition to `crack-core`: `music::remote`

`music::queue` and `connected_call` are `pub(crate)`, and every songbird access
in crack-core sits behind `clippy.toml` bans (`Songbird::get`,
`TrackHandle::data`). So the queue operations the dashboard needs live in
crack-core, in a new public module `music::remote` — "queue operations for
callers with no poise `Context`" — and crack-web never touches songbird:

- `queue_state(&Data, GuildId) -> QueueState` — `Idle` | `Hidden` (a game owns
  playback; checked before the call is touched) | `Playing { bot_channel,
  tracks: Vec<TrackSummary> }`, where `tracks[0]` is now playing. Reads through
  `connected_call`, `get_track_handle_metadata` and `get_requesting_user`.
- `active_guilds(&Data) -> Vec<(GuildId, ChannelId)>` — guilds with a connected
  call and the channel it is in.
- `move_by_id(&Data, &Http, GuildId, Uuid, to) -> Result<(), MoveRefused>` —
  takes the lease, moves, drops the lease, refreshes the queue messages.

Underneath it, `move_track_by_id(guard: &QueueGuard, handler: &Call, id: Uuid, to: usize) ->
Result<(), MoveRefused>` beside `move_track` in `music/queue.rs`. It locates the
track by `TrackHandle::uuid()`, refuses if the id is absent or is the
now-playing entry (index 0), and clamps `to` into the upcoming range (queue
indices `1..len`). It takes a `QueueGuard` like every other queue mutation, so
`/gp` exclusion is enforced by the type. `/movesong` keeps its index API.

### Wire types

Typed serde, one enum per direction, per the owner's serialization rules; no
`serde_json::Value` or `json!`.

```rust
// outbound
#[serde(tag = "state", rename_all = "snake_case")]
enum QueueView {
    Idle,                                             // bot not in voice, or queue empty
    Hidden,                                           // a /gp game owns playback
    Playing { now: TrackView, upcoming: Vec<TrackView>, rev: u64 },
}
struct TrackView {
    id: Uuid, title: String, url: Option<String>,
    duration_secs: Option<u64>, requester: Option<String>,
}
#[serde(tag = "result", rename_all = "snake_case")]
enum MoveResult { Moved { view: QueueView }, Conflict { view: QueueView },
                  NotAllowed, GameInProgress }

// inbound
struct MoveRequest { id: Uuid, to: usize, rev: u64 }
```

The server-rendered HTML and each SSE event are produced from the same
`QueueView`, so the first paint and the live updates cannot disagree. `rev` is a
hash of the ordered track-id list (current first); "has it changed?" is one
comparison.

### Configuration

| Variable | Meaning |
|---|---|
| `DISCORD_CLIENT_ID` | OAuth client id (defaults to `DISCORD_APP_ID` when unset) |
| `DISCORD_CLIENT_SECRET` | OAuth client secret |
| `WEB_PUBLIC_ORIGIN` | e.g. `https://dash.cracktun.es`; builds the redirect URI and is the only accepted `Origin` on POSTs |
| `WEB_JWT_SECRET` | signs session JWTs |
| `WEB_ENCRYPTION_KEY` | catacombs' refresh-token encryption key (required by its config even with memory storage) |
| `WEB_BIND` | listen address; default `0.0.0.0:8090` |

If any required variable is missing, the web server logs one WARN naming the
missing keys (names only, never values) and does not start. The bot runs
regardless. The port is not 8080: `edge/Caddyfile` already routes
`192.168.30.30:8080` to crack-voting.

## catacombs: the website flow (prerequisite PR)

catacombs was built for a Discord Activity: client JS obtains a code through the
Discord SDK, POSTs it to `/exchange`, receives a JWT in the body and sends it as
`Authorization: Bearer`. A browser tab needs the redirect flow and cookies.
Additions, all additive — the existing SDK flow keeps working unchanged:

- **`GET /login?return_to=<path>`** — generates a random `state`, stores it with
  `return_to` in a short-lived (10 min) HttpOnly cookie, and redirects to
  Discord's authorize URL with the configured scopes (default `identify`).
- **`GET /callback?code&state`** — verifies `state` against the cookie (constant
  time), exchanges the code (the existing exchange code path), upserts the user,
  sets the session cookie, clears the state cookie, and redirects to
  `return_to`. `return_to` must be a same-site relative path: it starts with a
  single `/`, not `//` or `/\`, else it falls back to `/`. A missing or
  mismatched `state` is a 400 and nothing is exchanged.
- **Cookie session:** the JWT in a cookie (name configurable, default
  `catacombs_session`; HttpOnly, Secure, SameSite=Lax, Path=/, Max-Age = JWT
  lifetime). The `AuthenticatedUser` extractor tries the cookie first, then the
  header, then the query parameter.
- **`POST /logout`** also clears the session cookie; **`GET /logout`** is not
  added (a GET must not change state).
- `DiscordConfig.api_base` (default `https://discord.com/api/v10`) so tests
  can point the token and user calls at a local mock that records what was
  sent.
- New `WebConfig` (public origin / redirect URI, scopes, cookie name,
  `Secure` flag for local development) — optional, so existing consumers
  compile.
- README: the website flow, and the crates.io badge/install lines corrected to
  the git dependency form they actually need today.

Storage: cracktunes uses `memory-storage`. `SqlxStorage` is on sqlx 0.8 and
migrates into the default `_sqlx_migrations` table, which would collide with
cracktunes' own migration history; neither matters with memory storage, and no
user needs persisting in this arc.

**Baseline first.** catacombs' only CI run on master is red (rustfmt, clippy
on dead `StoredEntitlement` fields under `memory-storage`, an MSRV of 1.75 its
dependencies cannot meet — they need 1.88 — and the audit job, which lacks
`checks: write` and reports an advisory). The PR makes it green first, and
drops dependencies nothing uses (`oauth2`, `dotenvy`, `tower`, `tower-http`,
`tracing-subscriber`, axum's `ws`) so cracktunes does not compile them.

**Release.** `release.yml` runs `cargo publish` on every tag, and the repo has
no `CRATES_IO_TOKEN`, so a tag today fails and the GitHub-release job (which
`needs: publish`) is skipped. The publish step becomes conditional on the
token being present: a tag then produces a GitHub release and no crates.io
publish, which keeps publishing the owner's later decision. Released as
catacombs **v0.1.0** by that tag push (case A of the release rule).

## Auth and access

**Login.** "Log in with Discord" → `/auth/login?return_to=/g/<id>` → Discord
(`identify` only) → `/auth/callback` → cookie set → back where the user started.
The dashboard never asks for the `guilds` scope: membership comes from the
bot's cache, which is fresher and asks less.

**Every request re-checks against the cache**; nothing about guilds or voice is
stored in the session.

- **Membership** is: in the cached member list, **or** has a cached voice state
  in the guild, **or** `Http::get_member` succeeds. The bot never requests
  member chunks, so for large guilds `GUILD_CREATE` leaves the cached member
  list partial and a cache-only check would 404 real members. The HTTP answer
  is remembered for 5 minutes: a 404 as "not a member", success as "member";
  any other error is not remembered and the request answers 503 ("Discord did
  not answer, try again"). Controllers never reach the HTTP path — being in
  voice puts them in the cache.
- **Guild picker (`GET /`):** guilds where the bot is in a voice channel right
  now **and** the user is a member. A stranger therefore cannot list the
  bot's guilds.
- **`can_view` fails → 404**, not 403, so probing a guild id does not confirm the
  bot is there.
- **`can_control`:** the user's cached voice state channel equals the bot's
  channel in that guild. The page shows drag handles only when true (the view
  is rendered per-user); every move re-checks it.

**POST protection.** Cookie auth means a cross-site POST would carry the cookie.
Three independent defences: `SameSite=Lax` withholds the cookie from
cross-site POSTs; `POST /g/{id}/move` requires `Content-Type: application/json`
(415 otherwise), which no plain HTML form can send; and it requires `Origin` to
equal `WEB_PUBLIC_ORIGIN` (403 otherwise).

**Data held.** The bot gains an OAuth client secret and a JWT key. Nothing is
written to the database. Personal data: a user id and username inside a signed
cookie. The cracktun.es Privacy Policy gets a paragraph saying so.

## Data flow

**Page load.** `GET /g/{id}` returns the page with the current `QueueView`
inlined as JSON (`<script type="application/json">`, with every `<` written as
`\u003c` so a track title containing `</script>` cannot end the element). One
renderer — the page's JS — draws both the inlined view and every SSE event, so
the first paint and the live updates cannot disagree, and there is no extra
round trip. Titles are inserted with `textContent`, never `innerHTML`: track
titles are arbitrary third-party text.

**Snapshot** (`remote::queue_state`). Lock the `Call`, clone the `current_queue()` handles, release the
lock, then read each handle's metadata (the same typemap read `/queue` does).
No formatting under the lock. Playback position is deliberately absent — it
changes every second and this arc does not need it.

**Live updates.** The page opens an `EventSource` on `GET /g/{id}/events`. The
first event is the current view. `watch` creates a guild's channel on first
subscribe and stops its task about 30 s after the last subscriber leaves. The
task snapshots about once a second and publishes only when `rev` (or the
Idle/Hidden state) changes. Each event is the whole view, never a diff, so a
client that missed events is correct again after the next one. The stream
re-checks `can_view` every 30 s and closes if it fails. SSE keep-alive comments
every 15 s keep proxies from idling it out.

Polling is chosen over instrumenting mutation sites: the queue changes from a
dozen places (track end, `/play`, `/remove`, autoplay, `/gp`), and "remember to
notify" at each is the bug class #434 removed. It costs nothing while nobody is
watching.

**A move.** Drop → `POST /g/{id}/move` with `MoveRequest { id, to, rev }`:

1. `can_control`, else `NotAllowed`.
2. `lock_queue(guild, PlaybackOwner::Free)`; `GameInProgress` during `/gp`.
3. Lock the `Call`, `move_track_by_id`. Absent id or now-playing → `Conflict`,
   nothing moves. `to` is a position in *upcoming* and is clamped. A stale `rev`
   does not refuse the move — the id makes it safe — it only tells the client to
   re-render.
4. Drop the guard. Push a fresh snapshot into the guild's watch channel so every
   open tab updates immediately.
5. Refresh Discord's queue messages (`update_queue_messages`), no reply posted.
   Log the move at INFO with user id, guild id, track id and positions.
6. Respond `Moved { view }`; the page renders from it.

## Failure behaviour

- **Web server cannot start** (missing env, bind failure): logged, bot keeps
  running. The dashboard must never be able to take the bot down.
- **Bot leaves voice / queue empties:** view becomes `Idle`; handles disappear;
  the page revives when playback resumes.
- **`/gp` starts:** view becomes `Hidden` — no titles anywhere in the page or
  stream, since the queue would reveal the song being guessed.
- **SSE drops:** `EventSource` reconnects on its own; a "reconnecting…" badge
  shows until the next event.
- **A refused move:** a short inline message, then a re-render from the returned
  view. The page never keeps an optimistic order the server rejected.
- A request timeout layer on every route except the SSE stream.

## Testing

The owner's standing rules apply: **sabotage every test** (each PR body carries
a mutation → caught-by table; an uncaught mutation is reported), and **assert on
what is sent** and on request counts.

- **catacombs:** callback rejects missing/wrong `state` and exchanges nothing
  (request count 0); `return_to` rejects `https://x`, `//x`, `/\x`; extractor
  prefers cookie over header; logout clears the cookie; a mock token endpoint
  asserts on basic auth, `redirect_uri`, `grant_type` and `code`.
- **`move_track_by_id`:** moves by id; refuses an absent id and the now-playing
  id, leaving the queue untouched; clamps `to` at both ends.
- **`access`:** the decision is a pure function over a `Presence` (membership,
  user's channel, bot's channel), the pattern `music::perms` already uses —
  member / non-member / unknown; in the bot's channel / another / not in voice /
  bot not in voice. The membership memo: a 404 is remembered, a 500 is not,
  entries expire.
- **The page:** a title containing `</script>` does not terminate the inlined
  JSON.
- **`snapshot` / `rev`:** order change changes `rev`; identical queue keeps it;
  game → `Hidden`.
- **routes** (`tower::ServiceExt::oneshot`): unauthenticated → redirect to login;
  non-member → 404; not in voice → `NotAllowed` **and queue unchanged**; wrong
  `Origin` → 403, non-JSON → 415, both with queue unchanged; `/gp` →
  `GameInProgress`, and `GET /g/{id}` contains no track titles.
- **Manual on TuneTitan:** two tabs plus Discord. Drag in one, the other follows;
  a `/skip` in Discord reaches the page within about a second; leaving voice
  removes the handles.

## Rollout

Each step ships on its own.

1. **catacombs** PR → merge → tag `v0.1.0`.
2. **cracktunes** PR → v0.15.0. The dashboard is dormant until its env is set, so
   it can deploy to production with no visible change.
3. **TuneTitan:** secrets in `.env.tunetitan`; register
   `http://localhost:8090/auth/callback` on TuneTitan's own Discord application;
   test through `ssh -L 8090:localhost:8090`. No public beta hostname: TuneTitan
   sits on the LAN, not VLAN 30, and a cross-VLAN path just for testing is not
   worth opening. (`Secure` cookies are accepted on `http://localhost`.)
4. **Production** (homelab PR): env and port on the bots VM, a Caddy site block
   for `dash.cracktun.es`, a tunnel ingress rule and CNAME, a `verify.sh` check.
   The owner adds `https://dash.cracktun.es/auth/callback` in the Discord
   developer portal.
5. **cracktun.es:** Privacy Policy paragraph.

## Out of scope

Skip / pause / remove / add; playback position and progress; an operator view;
persisted web users; publishing catacombs; a "moved by" note in Discord;
catacombs' sqlx 0.9 bump and configurable migrations table.
