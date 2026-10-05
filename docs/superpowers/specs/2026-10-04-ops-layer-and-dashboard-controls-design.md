# Ops layer and premium dashboard controls — design

**Date:** 2026-10-04. **Status:** approved in conversation (four sections), owner said
"take it to deploy on TuneTitan".

## Goal

Premium servers get playback controls on the web dashboard: skip, pause/resume,
remove, shuffle and repeat. Free servers keep what they have, which is viewing the
queue and drag-reordering it, and see the new controls disabled with a premium note.

To get there, every user-initiated playback or queue change moves onto one shared
**ops layer** in crack-core. Slash commands, the dashboard and (in the next arc) the
embed buttons become thin surfaces over it. The owner chose the full refactor now
(approach C) because the button controls for the now-playing embed come next and
will use the same layer.

Two PRs and two releases:

1. **v0.20.1, the refactor.** Every mutating music command moves onto `ops`. Visible
   behavior is unchanged except for the fixes listed in §3.
2. **v0.21.0, the dashboard controls,** built on `ops`.

## Decisions (owner)

| Question | Decision |
|---|---|
| First sub-project | Transport controls. Add and likes are later arcs. |
| Controls in this arc | skip, pause/resume, remove, shuffle, repeat |
| Free servers | Controls rendered **disabled**, with a premium note and the Patreon link |
| Discord echo | A **short line in the channel** for each dashboard control. Reorder stays silent. |
| Structure | **Approach C**: a shared ops layer, every mutating command migrated |
| Refactor scope | **All playback/queue mutators except add**: skip, voteskip, pause, resume, repeat, remove, shuffle, movesong, stop, clear, seek, volume, leave, plus the dashboard move |

Assumed and not contradicted: premium widens *what* a controller can do, not *who*
controls. A controller is still a member in the bot's voice channel. A running `/gp`
game refuses every control, the same as move.

## 1. The ops layer

`crack-core/src/music/ops.rs` (a directory module if it grows) is the only place a
user-initiated playback or queue change is orchestrated: look up the call, take the
lease, check preconditions, mutate, and hand back a typed outcome plus how to settle.

### Inputs

```rust
pub struct OpCx {
    pub data: Data,              // cheap clone (Arc inside)
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
    pub guild_id: GuildId,
    pub actor: Actor,
}
```

- `Actor::from_ctx` serves commands.
- `Actor::web(user)` currently hard-codes the command name `"dashboard move"`. It
  becomes `Actor::web(user, op)`, recorded as `"dashboard <op>"` (for example,
  `"dashboard skip"`), so the audit log's `command` column says what was clicked.
- The buttons arc adds an interaction constructor. That is out of scope here.

### Functions

There is one function per op, each with its own outcome type, so a surface can't
mistake one op's result for another's:

| fn | outcome |
|---|---|
| `skip(cx, count: usize, expect: Option<Uuid>)` | `Skipped { now: Option<TrackSummary>, count }` |
| `voteskip(cx, voter: UserId)` | `Voted { missing }` or `Skipped(..)` |
| `pause(cx)` / `resume(cx)` | `Paused` / `Resumed` |
| `repeat(cx, on: Option<bool>)` | `Repeat { on: bool }`. `None` toggles. |
| `remove(cx, Target)` | `Removed { first: TrackSummary, count }` |
| `shuffle(cx)` | `Shuffled { count }` |
| `move_track(cx, Target, to)` | `Moved { to }` |
| `stop(cx)` | `Stopped { removed }` |
| `clear(cx)` | `Cleared { removed }` |
| `seek(cx, Duration)` | `Sought { to }` |
| `volume(cx, percent)` / `volume_now(cx)` | `VolumeSet { old, new }` / `VolumeNow { setting, track }` |
| `leave(cx)` | `Left` |

`Target` is `Index(usize)`, `Range(usize, usize)` or `Id(Uuid)`. Not every op accepts
every target: `move_track` takes `Index` or `Id`, and `remove` takes all three.

There is also a dispatcher for surfaces that receive ops as data. The dashboard (and
later the buttons) map a JSON body or a `custom_id` to a typed op:

```rust
pub enum Op { Skip { expect: Option<Uuid> }, Pause, Resume, Repeat { on: Option<bool> },
              Remove { id: Uuid }, Shuffle, Move { id: Uuid, to: usize } }
pub async fn run(cx: &OpCx, op: Op) -> Result<Done<Outcome>, OpRefused>;
```

`Op` holds only the ops a data-driven surface sends. Commands call the functions
directly.

### Refusals

```rust
pub enum OpRefused {
    NotConnected, NothingPlaying, QueueEmpty, GameInProgress,
    Absent,                 // no track with that id (finished, removed)
    NowPlaying,             // that id is playing; only upcoming tracks move/remove
    Stale,                  // skip's `expect` is not the current track
    OutOfRange { what: &'static str, got: usize, max: usize },
    Failed(&'static str),   // songbird refused (pause, resume, loop, seek)
}
```

Commands map these to the `CrackedError` they replied with before (the mapping is
tested, §4). The dashboard maps them to HTTP answers.

The game check comes first. `lock_queue(guild, PlaybackOwner::Free, actor)` refuses
a game-owned guild before the call is looked up, as `remote::move_by_id` already
does.

### Settling

Every outcome is returned in `Done<T> { outcome: T, settle: Settle }`. `Settle` is
`#[must_use]` and is decided by the op's effect, never by the surface:

| effect | ops | settle |
|---|---|---|
| queue order changed | remove, shuffle, clear, move | refresh the queue messages (`update_queue_messages`) |
| what's playing changed | skip, voteskip that skips | show now-playing (`show_now_playing_after`) |
| playback ended | stop, leave | show finished (`show_finished`) |
| state only | pause, resume, repeat, seek, volume, a vote that did not skip | nothing |

`settle.after(Some((channel, message)))` places now-playing after the surface's
reply, which is what `/skip` does today. `settle.now()` is the same with no anchor.
Both consume the token. Pause, resume and repeat settling to nothing is not a gap:
nothing in Discord shows that state.

### Replies stay with the surfaces

Ops never send a message. Each outcome type has `fn message(&self) -> CrackedMessage`
for the reply or echo a surface wants. A surface renders it as a slash reply, a
dashboard echo line, or (later) an interaction response.

## 2. The dashboard surface (v0.21.0)

### Endpoint

`POST /g/{g}/control`. The body is `ControlRequest`, typed serde with
`#[serde(tag = "type", rename_all = "snake_case")]`:

| body | op |
|---|---|
| `{"type":"skip","id":"<uuid>"}` | skip the current track **only if it is still `id`** |
| `{"type":"pause"}` / `{"type":"resume"}` | explicit, not a toggle |
| `{"type":"repeat","on":true}` | explicit, not a toggle |
| `{"type":"remove","id":"<uuid>"}` | remove an upcoming track by id |
| `{"type":"shuffle"}` | shuffle the upcoming tracks |

- Skip carries the id of the track the viewer saw playing. A double-click, or two
  viewers clicking at once, skips one track. The second click is `Stale` and gets a
  409 with the fresh view.
- Pause and repeat set a state rather than flipping one, so a stale tab can't flip
  it the wrong way.
- Removing the now-playing track is refused (`NowPlaying`). Skip is the control for
  that.

### Gate order

1. Session, else 401.
2. `Content-Type: application/json`, else 415.
3. `Origin` equals the configured origin, else 403.
4. Parse, else 400.
5. Presence (`decide`): `Hidden` gets 404, `Unavailable` gets 503, and `View` (not in
   the bot's channel) gets 403 `not_allowed`.
6. Rate limit, else 429 (see below).
7. **Plan:** `Plan::of(data.get_premium(g))` is read on every request. Free gets 403
   `premium_required`.
8. `Backend::control(user, g, ControlRequest)` → `ops::run`.

### Answers

```rust
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResult {
    Done { view: QueueView },          // 200
    Conflict { view: QueueView },      // 409: Absent, NowPlaying, Stale
    NotAllowed,                        // 403
    PremiumRequired,                   // 403
    GameInProgress,                    // 423
    NotPlaying,                        // 409: NotConnected, NothingPlaying, QueueEmpty
    Failed,                            // 500: songbird refused; logged
    TooMany,                           // 429
}
```

`MoveResult` and `POST /g/{g}/move` are unchanged. Move stays free and is not rate
limited. Internally it now calls `ops::move_track`.

After a `Done`, the route calls `hub.refresh(g)` so open tabs follow, exactly as move
does.

### View

- `QueueView::Playing` gains `paused: bool` and `looping: bool`, read from the
  current track's `TrackState` (`playing == PlayMode::Pause`, `loops ==
  LoopState::Infinite`). `TrackSummary`, or a new `PlaybackSummary` on
  `QueueState::Playing`, carries them out of crack-core. crack-web never touches
  songbird.
- The poller already publishes whenever the view changes, so a `/pause` typed in
  Discord flips the dashboard button within one tick.
- `PageState` gains `plan: PlanView` (`"free"` / `"premium"`), a crack-web wire enum
  mapped from `Plan`. The SSE stream's periodic presence recheck re-reads the plan
  and sends an event when it changes, so an open tab unlocks when premium is granted,
  with no reload.

### Discord echo

After `Done`, crack-web asks the backend to announce it. A crack-core helper
(`messaging::status::announce` or a sibling) posts one line in the channel
`target_channel` picks: the music channel, else the last command channel, else the
tracked status channel. No target means no echo.

- The lines are new `CrackedMessage` variants, with their strings in `messages.rs`
  (the localization convention), for example:
  - ⏭ Skipped **{title}** from the dashboard — {mention}
  - ⏸ Paused from the dashboard — {mention}
  - ▶ Resumed from the dashboard — {mention}
  - 🔁 Repeat on/off from the dashboard — {mention}
  - 🗑 Removed **{title}** from the dashboard — {mention}
  - 🔀 Shuffled the queue from the dashboard — {mention}
- `CreateAllowedMentions` is empty, so the mention renders and pings nobody.
- It is spawned in the background. A failed send is logged at WARN and never changes
  the HTTP answer.
- Move (reorder) posts nothing, as today.

### Rate limit

At most **5 controls per user per 10 seconds**, across all guilds, kept in memory
(`DashMap<UserId, VecDeque<Instant>>` or a fixed window). Over the limit, the route
answers 429 `too_many`. The op does not run and nothing is echoed. The check happens
after presence (so strangers can't fill it) and before the plan and the op.

### Page (app.js, page.rs)

- A control bar above now-playing: ⏯ pause/resume (it shows the action the click
  will take), ⏭ skip, 🔀 shuffle, 🔁 repeat (pressed while `looping`). Each upcoming
  row gets a ✕.
- Enabled only when `can_control && plan == "premium"`.
- With `plan == "free"`, the controls render **disabled**, with the line "Dashboard
  controls are a premium feature." and a link to the CrackTunes Patreon. The string
  goes in `messages.rs` beside `PREMIUM_HISTORY`, and the URL is the one the history
  note uses.
- Premium but not in voice: disabled, with the existing "join the voice channel"
  hint that drag uses.
- A `conflict` re-renders from the view in the answer. Every other refusal shows a
  short inline status, as move's refusals do.
- No new assets. The existing CSP and Trusted Types policy cover the DOM building,
  and buttons are created with `createElement`, never HTML strings.

## 3. Migrating the commands (v0.20.1)

**Rule:** after this release, every user-initiated playback or queue change goes
through `ops`.

### Enforcement

`clippy.toml` gains `disallowed-methods` entries for the guarded mutators in
`crack_core::music::queue`:

- `pause_queue`, `resume_queue`, `remove_at`, `stop_queue`, `clear_from`
- `drain_after_current`, `shuffle_behind_current`, `move_track`, `move_track_by_id`
- `force_skip_top_track`

The reason on each is "orchestrate through music::ops". The legitimate direct
callers carry `#[expect(clippy::disallowed_methods, reason = "...")]`:

- `music::ops` itself;
- `/gp`'s five sites, because it runs its rounds as `PlaybackOwner::Game` and
  interleaves its mutations with game state;
- `track_end`'s autopause, because it is the bot acting on its own (`Actor::bot`).

clippy must be shown to fire. Before the `#[expect]`s are added, a deliberate direct
call from a command fails `cargo clippy -D warnings`. If clippy can't ban a
crate-local path, the fallback is visibility (`pub(in crate::music)`) with the same
two exemptions re-exported through a narrow, named module, and the spec is amended.

### Command → op

| command | op | change |
|---|---|---|
| `/skip [n]` | `skip(n, None)` | none |
| `/voteskip` | `voteskip(author)` | none. Votes stay in `guild_cache_map`, and the threshold rule is unchanged. |
| `/pause`, `/resume` | `pause`, `resume` | none |
| `/repeat` | `repeat(None)` | **now audited** as `Action::Repeat { on }` |
| `/remove a [b]` | `remove(Index / Range)` | the single-track embed no longer `unwrap()`s the title, URL and thumbnail. Missing metadata renders the title (or "Unknown") as plain text, with no thumbnail. |
| `/shuffle` | `shuffle` | none |
| `/movesong` | `move_track(Index, to)` | none. `remote::move_by_id` becomes a call to `move_track(Id, to)`. |
| `/stop` | `stop` | none. It still turns autoplay off first. |
| `/clear` | `clear` | none |
| `/seek mm:ss` | `seek(Duration)` | parsing stays in the command. **It now takes the lease to issue the seek**, so it can't land on the track a concurrent skip just started. The guard is dropped before the seek's callback is awaited, and that wait is bounded by `SEEK_TIMEOUT` (amended 2026-10-04 after the final review: a seek on a remote input can take seconds). |
| `/volume [n]` | `volume(n)` / `volume_now()` | **takes no lease** (amended 2026-10-04: volume changes no queue order, and a lease would let a `/gp` game block it, which it never did). Settings go through a setter on `Data` instead of writing `guild_settings_map` inline, and the setter returns the real old value. The per-call `error!` logs and the `get_info().unwrap()` go. Reply texts are unchanged. |
| `/leave` | `leave` (wraps `music::disconnect`) | none |
| `downvote` (unregistered) | its skip goes through `skip` | stays unregistered |

Replies, ephemeral settings and `cmd_check_music`'s earlier game refusal are
unchanged.

### Audit

- `Action::Repeat { on: bool }`, with the tag `"repeat"`. `queue_audit.action` is
  `TEXT`, so no migration.
- `what_text` renders it as "repeat on" or "repeat off".
- `ActionChoice::Repeat` is added to `/auditlog`'s filter.
- `"repeat"` is added to the history page's action filter list (`page.rs`).
- Seek and volume stay unaudited, because the log covers queue changes.
- `docs/queue-audit.md` lists the new action.

## 4. Testing and rollout

The standing rules apply: every new test is shown failing against sabotaged code
before it is trusted, and tests assert on what is sent or mutated. Each PR body
carries a mutation → caught-by table.

### ops (crack-core), against `Call::standalone` queues

- Each op's mutation: the queue order, the current track, and the paused and loop
  state.
- Each op's audit row: the actor and the action detail, read from the audit channel
  as `lease.rs`'s tests do. Repeat is recorded. Seek and volume record nothing.
- Refusals:
  - a game refuses before the call is read (as remote.rs's tests do);
  - no call is `NotConnected`;
  - an empty queue is `NothingPlaying` or `QueueEmpty`, as each command did before;
  - an out-of-range index is `OutOfRange`;
  - an unknown id is `Absent`;
  - the playing id is `NowPlaying` for remove and move;
  - **a stale `expect` on skip is `Stale` and skips nothing.**
- `Settle`: each op's outcome carries the variant the §1 table gives.
- The `OpRefused` → `CrackedError` mapping matches what each command replied before.

### crack-web, against `FakeBackend`

- Each gate in order. Each refusal before the backend leaves the backend's control
  call count at zero.
- Free gets `premium_required` and never reaches the backend. Premium does.
- The backend receives exactly the op that was sent: a typed `ControlRequest`
  round-trip, unknown `type` → 400.
- `conflict` carries the fresh view. `game_in_progress` is 423.
- The 6th control in 10 s gets 429, never reaches the backend, and is not echoed.
- A `done` reaches open SSE watchers.
- The echo is sent once per `done`, with the expected `CrackedMessage`, through a
  recording fake. None is sent for a refusal or for a move.
- `PageState` serializes `plan`, and `QueueView::Playing` serializes `paused` and
  `looping`. A pause flips the view, so the poller publishes.
- A plan change during an open stream reaches it at the next recheck.

### Audit

`Action::Repeat` round-trips through serde, renders in `what_text`, and parses as an
`/auditlog` filter choice.

### Rollout

1. **PR 1 (`feat/ops-layer`), v0.20.1.** PR 2 (`feat/dashboard-controls`) stacks on
   it, v0.21.0.
2. TuneTitan runs the PR 2 branch image, which contains both. Both image vars are
   overridden, as `docker.yml` dispatch publishes them. The owner checks there:
   - each slash command once;
   - each dashboard control on a premium server and a free one;
   - the echo lines;
   - a double-click on skip;
   - a premium grant unlocking an open tab without a reload.
3. Then merge PR 1, tag `v0.20.1`, merge PR 2, tag `v0.21.0`. The tag push is the
   release (cargo-dist owns it). Production deploys on the owner's go, with
   `./homelab.sh up bots` then `verify bots` after `Ready` logs
   `sleevenote: configured`. 43 commands are still registered.
4. `.env.tunetitan` is re-pinned to `v0.21.0` afterwards.
5. Docs: `docs/web-dashboard.md` gains a controls section.

## Out of scope

- Add (the next arc), likes on `track_reaction` (their own arc), and the embed
  buttons (the arc after this, which only adds a surface over `ops`).
- Volume, seek, stop, clear and leave on the dashboard.
- The history page's follow-ups.
- `/voteskip`'s threshold. `downvote` stays unregistered.
