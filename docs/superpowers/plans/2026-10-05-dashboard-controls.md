# Premium Dashboard Controls (v0.21.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Premium servers get skip, pause/resume, repeat, remove and shuffle on the web
dashboard. Free servers see them disabled, with a premium note. Each control posts a
one-line echo in Discord.

**Architecture:** crack-core's `music::remote` gains `control()`, the dashboard's only
seam into `music::ops` (as `move_by_id` already is). It runs the op, then spawns one
background task that posts the echo and settles anchored after it. `QueueState::Playing`
gains `paused`/`looping`. crack-web adds `POST /g/{g}/control` with the same gates as
move, plus a per-user rate limit and a plan gate. `PageState` carries `plan`, and the SSE
recheck re-reads it. app.js draws a control bar.

**Tech Stack:** Rust (axum 0.8, serde, tokio), serenity, songbird (inside crack-core
only), vanilla JS under CSP + Trusted Types.

**Spec:** `docs/superpowers/specs/2026-10-04-ops-layer-and-dashboard-controls-design.md`, §2.
Plan 1 (`docs/superpowers/plans/2026-10-04-ops-layer.md`) is merged into this branch's base.

## Global Constraints

- Commit trailer, exactly: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.
  No other Co-Authored-By line.
- Never `git add -A` or `git add .`. Name the files.
- **crack-web never touches songbird.** It uses only `crack_core::music::remote` plain
  types (and `crack_core::guild::plan::Plan`).
- Typed serde for every wire type: `#[serde(tag = "type", rename_all = "snake_case")]`
  for requests, and `#[serde(tag = "result", rename_all = "snake_case")]` for answers.
  No `json!` and no `Value`.
- Gate order for `/control`, exactly:
  1. session (401);
  2. `Content-Type: application/json` (415);
  3. `Origin == s.origin` (403);
  4. parse (400);
  5. presence: Hidden → 404, Unavailable → 503, View → 403 `not_allowed`;
  6. rate limit (429 `too_many`);
  7. plan: Free → 403 `premium_required`;
  8. backend.
- Answers:

  | result | status |
  |---|---|
  | `done{view}` | 200 |
  | `conflict{view}` | 409 |
  | `not_allowed` | 403 |
  | `premium_required` | 403 |
  | `game_in_progress` | 423 |
  | `not_playing` | 409 |
  | `failed` | 500 |
  | `too_many` | 429 |
- Rate limit: **5 controls per user per 10 s**, across all guilds, in memory. Move is
  not limited.
- The plan is read on every request: `Plan::of(data.get_premium(g).await)`.
- The echo is never sent for a refusal or for a move. A failed echo send is logged at
  WARN and never changes the HTTP answer.
- User-facing Rust strings are constants in `crack-core/src/messaging/messages.rs`.
  app.js keeps its own short strings inline, as it does today.
- `#[expect(..., reason = "...")]`, never `#[allow]`.
- **Sabotage every new test.** Break the code it guards, watch it fail, restore it.
  Keep a mutation → test table in each task report.
- Format with NIGHTLY: `rustfmt +nightly --edition 2024 <files you touched>`, or
  `cargo +nightly fmt --all` if the tree stays scoped. CI checks nightly.
- Version: root `Cargo.toml` `[workspace.package] version = "0.21.0"` (Task 6).
- Verify before each commit: `cargo test -p crack-core --lib`, `cargo test -p crack-web`,
  and `cargo clippy --workspace --all-targets -- -D warnings`.

## Review Focus

1. **A double-click on skip** skips one track. The second request carries the same
   `expect` id and gets `conflict` (Task 4 pins it at the route, with a fake whose
   second control returns `Conflict`. Task 1 pins `Stale` in core).
2. **A premium grant reaches an open tab** without a reload, at the next SSE recheck
   (Task 5).
3. **A spam-clicker** gets 429 on the 6th control in 10 s. The op does not run and
   nothing is echoed (Task 4).
4. **The echo goes above now-playing, not below it.** The echo is posted first, then
   the settle is anchored after it, so `/skip`'s ordering race can't recur (Task 1).
5. **A stalled driver can't freeze the dashboard view.** The `paused`/`looping` read is
   bounded by `TRACK_INFO_TIMEOUT`, and a timeout reads as `false` (Task 2).

---

## File structure

| File | Responsibility |
|---|---|
| `crack-core/src/music/ops/skip.rs` | `Skipped.skipped: Option<TrackSummary>` (the track that was playing) |
| `crack-core/src/music/remote.rs` | `Control`, `ControlRefused`, `Echo`, `control()`; `QueueState::Playing { paused, looping }` |
| `crack-core/src/messaging/status.rs` | `announce()`: post one echo embed where the status message would go |
| `crack-core/src/messaging/messages.rs` | echo strings, `PREMIUM_CONTROLS` |
| `crack-web/src/view.rs` | `QueueView::Playing { paused, looping }`, `PlanView`, `PageState.plan`, `ControlRequest`, `ControlResult` |
| `crack-web/src/limit.rs` (create) | `RateLimit`, a sliding window per user with an injectable clock |
| `crack-web/src/backend.rs` | `Backend::control`, `Backend::plan` |
| `crack-web/src/lib.rs` | `LiveBackend` impls |
| `crack-web/src/test_support.rs` | `FakeBackend` controls and plan |
| `crack-web/src/routes.rs` | `POST /g/{g}/control`; plan in the page and the SSE stream |
| `crack-web/src/page.rs`, `crack-web/assets/app.js`, `crack-web/assets/app.css` | the control bar, ✕ per row, the premium note |
| `docs/web-dashboard.md`, `Cargo.toml` | docs, v0.21.0 |

---

### Task 1: core `remote::control`, the echo, and `announce`

**Files:**
- Modify: `crack-core/src/music/ops/skip.rs` (`Skipped` gains `skipped`)
- Modify: `crack-core/src/music/remote.rs` (`Control`, `ControlRefused`, `Echo`, `control`)
- Modify: `crack-core/src/messaging/status.rs` (`announce`)
- Modify: `crack-core/src/messaging/messages.rs` (the echo strings)

**Interfaces:**
- Consumes: `ops::{OpCx, skip, pause, resume, repeat, remove, shuffle, Target, OpRefused, Done}`,
  `Done::{outcome, into_parts}`, `Settle::after(cx, call, anchor)`, and
  `messaging::status::{target_channel, DiscordTransport, StatusTransport}`.
- Produces (Task 3's `LiveBackend` calls these):

```rust
// remote.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control { Skip { expect: Uuid }, Pause, Resume, Repeat { on: bool }, Remove { id: Uuid }, Shuffle }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRefused { NotPlaying, GameInProgress, Conflict, Failed }
/// What a control did, for the echo line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Echo { Skipped { title: Option<String> }, Paused, Resumed, Repeat { on: bool },
                Removed { title: Option<String> }, Shuffled }
impl Echo { pub fn line(&self, user: UserId) -> String; }
impl From<&OpRefused> for ControlRefused;
pub async fn control(data: Arc<Data>, http: Arc<Http>, cache: Arc<Cache>, guild_id: GuildId,
                     user: UserId, c: Control) -> Result<Echo, ControlRefused>;
// status.rs
/// Post `embed` where the status message would go. Returns where it landed.
pub async fn announce(data: &Data, transport: &dyn StatusTransport, guild: GuildId,
                      embed: CreateEmbed<'static>) -> Option<(GenericChannelId, MessageId)>;
```

- `ops::Skipped` gains `pub skipped: Option<TrackSummary>`, the track that was playing
  when the skip ran. It is summarized from a handle cloned under the lock before the
  skip.

**Mapping.** `From<&OpRefused> for ControlRefused`:
- `GameInProgress` → `GameInProgress`;
- `NotConnected`, `NothingPlaying`, `QueueEmpty` → `NotPlaying`;
- `Absent`, `NowPlaying`, `Stale` → `Conflict`;
- everything else → `Failed`, logged at WARN by `control` with the `Debug` of the
  refusal.

**`control` body:**
1. `cx = OpCx { data, http, cache, guild_id, actor: Actor::web(user, op_name) }`, where
   `op_name` is `"skip"`, `"pause"`, `"resume"`, `"repeat"`, `"remove"` or `"shuffle"`.
2. Run the op:

   | control | op | echo |
   |---|---|---|
   | `Skip{expect}` | `ops::skip(&cx, 1, Some(expect))` | `Skipped{title: skipped.title}` |
   | `Pause` | `ops::pause` | `Paused` |
   | `Resume` | `ops::resume` | `Resumed` |
   | `Repeat{on}` | `ops::repeat(&cx, Some(on))` | `Repeat{on}` |
   | `Remove{id}` | `ops::remove(&cx, Target::Id(id))` | `Removed{title: first.title}` |
   | `Shuffle` | `ops::shuffle` | `Shuffled` |

3. On `Ok(done)`, take `(outcome, settle, call) = done.into_parts()`, build the `Echo`,
   and `tokio::spawn` one task that runs:
   - `anchor = status::announce(&cx.data, &DiscordTransport{http, cache}, guild, echo_embed(&echo, user)).await`
   - then `settle.after(&cx, call.as_ref(), anchor).await`.

   The echo comes first, so now-playing lands below it.
4. Return `Ok(echo)`.

`echo_embed(echo, user) = CreateEmbed::new().description(echo.line(user))`. An embed
mention never pings, so no `allowed_mentions` is needed.

`Echo::line` uses the new messages.rs constants:

```rust
pub const ECHO_SKIPPED: &str = "⏭ Skipped";
pub const ECHO_PAUSED: &str = "⏸ Paused";
pub const ECHO_RESUMED: &str = "▶ Resumed";
pub const ECHO_REPEAT_ON: &str = "🔁 Repeat on";
pub const ECHO_REPEAT_OFF: &str = "🔁 Repeat off";
pub const ECHO_REMOVED: &str = "🗑 Removed";
pub const ECHO_SHUFFLED: &str = "🔀 Shuffled the queue";
pub const ECHO_FROM_DASHBOARD: &str = "from the dashboard";
```

`line` composes them as:
- `"{ECHO_SKIPPED} **{title}** {ECHO_FROM_DASHBOARD} — <@{user}>"` when there is a title;
- `"{ECHO_SKIPPED} {ECHO_FROM_DASHBOARD} — <@{user}>"` when there isn't;
- the same pattern for the others.

Titles are third-party text, so escape Discord markdown in them with the existing
`crate::music::audit_view::escape`. That is `pub(crate)`, and remote.rs is in the same
crate.

`announce`:
- `music = data.get_music_channel(guild).await`;
- lock the guild's status slot only long enough to copy `last_command_channel` and the
  tracked message's channel;
- `target = target_channel(music, last, tracked)?`;
- `transport.send(target, embed)`, giving `Some((target, id))`. A send error is logged
  at WARN and gives `None`.

It does **not** touch `slot.message`, because the echo is not the status message.

- [ ] **Step 1: Write the failing tests.**

```rust
// skip.rs tests
#[tokio::test]
async fn skip_names_the_track_it_skipped() {
    let (data, call, ids, _) = queue_of(3).await;
    let g = guard(&data).await;
    let done = skip_on(&g, &call, 1, Some(ids[0])).await.unwrap();
    let s = done.outcome();
    assert_eq!(s.skipped.as_ref().map(|t| t.id), Some(ids[0]));
    assert_eq!(s.skipped.as_ref().and_then(|t| t.title.as_deref()), Some("t0"));
}

// remote.rs tests
#[test]
fn refusals_map_to_control_answers() {
    use crate::music::ops::OpRefused as R;
    for (r, want) in [
        (R::GameInProgress, ControlRefused::GameInProgress),
        (R::NotConnected, ControlRefused::NotPlaying),
        (R::NothingPlaying, ControlRefused::NotPlaying),
        (R::QueueEmpty, ControlRefused::NotPlaying),
        (R::Absent, ControlRefused::Conflict),
        (R::NowPlaying, ControlRefused::Conflict),
        (R::Stale, ControlRefused::Conflict),
        (R::Failed(crate::music::ops::Failure::Pause), ControlRefused::Failed),
    ] {
        assert_eq!(ControlRefused::from(&r), want, "{r:?}");
    }
}

#[test]
fn echo_lines_name_the_track_and_the_member() {
    let u = UserId::new(42);
    assert_eq!(Echo::Skipped { title: Some("Song".into()) }.line(u),
               "⏭ Skipped **Song** from the dashboard — <@42>");
    assert_eq!(Echo::Paused.line(u), "⏸ Paused from the dashboard — <@42>");
    assert_eq!(Echo::Repeat { on: false }.line(u), "🔁 Repeat off from the dashboard — <@42>");
    assert_eq!(Echo::Removed { title: None }.line(u), "🗑 Removed from the dashboard — <@42>");
    // Third-party titles cannot inject markdown or a mention.
    let l = Echo::Skipped { title: Some("**x** @everyone".into()) }.line(u);
    assert!(!l.contains("**x**"), "{l}");
}

#[tokio::test]
async fn a_game_refuses_a_control_before_the_call_is_looked_up() {
    let d = Arc::new(data());
    d.claim_playback(G, crate::music::PlaybackOwner::Game).unwrap();
    let http = Arc::new(Http::new(crack_types::get_valid_token()));
    let got = control(d, http, Arc::new(Cache::default()), G, UserId::new(9), Control::Pause).await;
    assert_eq!(got, Err(ControlRefused::GameInProgress));
}

#[tokio::test]
async fn a_control_with_no_call_is_not_playing() {
    let http = Arc::new(Http::new(crack_types::get_valid_token()));
    let got = control(Arc::new(data()), http, Arc::new(Cache::default()), G, UserId::new(9), Control::Shuffle).await;
    assert_eq!(got, Err(ControlRefused::NotPlaying));
}

// status.rs tests: use the existing `Fake` transport (status.rs ~line 488); read it first
#[tokio::test]
async fn announce_posts_where_the_status_would_go_and_leaves_the_status_alone() {
    // A guild whose last music command was in channel 10: the echo goes there,
    // and the slot's tracked status message is unchanged.
    // Build Data + note_command_channel(.., 10), a Fake transport; call announce;
    // assert Fake recorded exactly one send to channel 10, announce returned
    // Some((10, id)), and data.status_slot(g).lock().await.message is unchanged.
}

#[tokio::test]
async fn announce_with_nowhere_to_post_sends_nothing() {
    // No music channel, no last command, no tracked status: None, zero sends.
}
```

The two `announce` tests are written in prose because their shape depends on the
`Fake` in status.rs's tests. Read it and write them concretely, asserting the recorded
sends. Assert on what is SENT, not only on the return value.

- [ ] **Step 2: Run them, and see them fail** (a compile failure on the new symbols counts).

- [ ] **Step 3: Implement.**
  - **`skip_on`:** under the existing Call lock, after the `expect` check, clone
    `current` (`let was = current.clone();`). After the lock is released, add
    `let skipped = Some(summary_of(&was, get_track_handle_metadata(&was).await.unwrap_or_default()))`.
    Use whatever `summarize`/`summary_of` helper the code has; read `remote.rs`. Then
    `Skipped { skipped, now, count }`.
  - **`/skip` and `/voteskip`:** they ignore the new field. Their replies are
    unchanged.
  - **remote.rs and status.rs:** implement as specified above.

- [ ] **Step 4: Run** `cargo test -p crack-core --lib` and clippy.

- [ ] **Step 5: Sabotage.**
  - Map `Stale` to `Failed`. The mapping test fails.
  - Drop the `escape` in `line`. The injection assertion fails.
  - Make `announce` write `slot.message`. The "leaves the status alone" assertion fails.
  - In `skip_on`, summarize the *next* track as `skipped`. The names test fails.
  - Restore all four.

- [ ] **Step 6: Commit.**

```bash
git add crack-core/src/music/ops/skip.rs crack-core/src/music/remote.rs \
  crack-core/src/messaging/status.rs crack-core/src/messaging/messages.rs
git commit -m "remote: control() for the dashboard, with an echo line posted before the settle"
```

---

### Task 2: `paused` and `looping` in the queue state

**Files:**
- Modify: `crack-core/src/music/remote.rs` (`QueueState::Playing`, `state_of_call`)
- Modify: `crack-web/src/view.rs` (`QueueView::Playing`, `view_from_state`)
- Modify: crack-web tests that build `QueueState::Playing` / `QueueView::Playing` (the
  compiler lists them)

**Interfaces:**
- Produces:
  - `QueueState::Playing { bot_channel, tracks, paused: bool, looping: bool }`;
  - `QueueView::Playing { now, upcoming, rev, paused: bool, looping: bool }`.

  Both serialize as snake_case fields.
- `pub(crate) fn playback_flags(info: Option<&TrackState>) -> (bool, bool)`, where
  `None` gives `(false, false)`. Paused is `info.playing == PlayMode::Pause`, and
  looping is `info.loops == LoopState::Infinite`.

- [ ] **Step 1: Write the failing tests.**

```rust
// remote.rs
#[test]
fn flags_read_pause_and_infinite_loop_and_default_to_false() {
    use songbird::tracks::{LoopState, PlayMode, TrackState};
    let mut s = TrackState::default();
    assert_eq!(playback_flags(None), (false, false));
    s.playing = PlayMode::Pause; s.loops = LoopState::Infinite;
    assert_eq!(playback_flags(Some(&s)), (true, true));
    s.playing = PlayMode::Play; s.loops = LoopState::Finite(0);
    assert_eq!(playback_flags(Some(&s)), (false, false));
}

#[tokio::test]
async fn a_stalled_driver_reads_as_neither_paused_nor_looping() {
    // An offline Call::standalone never answers get_info: the state still comes
    // back (bounded by TRACK_INFO_TIMEOUT), with both flags false. Guard with an
    // outer tokio::time::timeout of a few seconds so a regression fails, not hangs.
}
```

If `TrackState` can't be constructed with `Default` or set directly, which depends on
the songbird version, test `playback_flags` over `(PlayMode, LoopState)` instead and
say so.

```rust
// crack-web view.rs
#[test]
fn the_view_carries_the_flags_through_and_serializes_them() {
    let v = view_from_state(QueueState::Playing { bot_channel: ChannelId::new(9),
        tracks: vec![t(1, Some("A"), None)], paused: true, looping: false }, names);
    let QueueView::Playing { paused, looping, .. } = &v else { panic!() };
    assert_eq!((*paused, *looping), (true, false));
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"paused\":true") && json.contains("\"looping\":false"), "{json}");
}
```

- [ ] **Step 2: Run them, and see them fail.**

- [ ] **Step 3: Implement.** In `state_of_call`, once the handles are cloned and the
  lock is dropped:

```rust
let info = match handles.first() {
    Some(h) => tokio::time::timeout(crate::music::ops::TRACK_INFO_TIMEOUT, h.get_info())
        .await.ok().and_then(Result::ok),
    None => None,
};
let (paused, looping) = playback_flags(info.as_ref());
```

Make `TRACK_INFO_TIMEOUT` `pub` if it isn't, and check where it lives.

`view_from_state` copies the flags into `QueueView::Playing`. The poller compares whole
views, so a pause now publishes. No change is needed in watch.rs; confirm that by
reading it.

- [ ] **Step 4: Run** both crates' tests and clippy.
- [ ] **Step 5: Sabotage.**
  - Swap `paused` and `looping` in `view_from_state`. The view test fails.
  - Drop the timeout. The stalled-driver test fails at its outer guard.
  - Restore both.
- [ ] **Step 6: Commit.**

```bash
git add crack-core/src/music/remote.rs crack-web/src/view.rs  # plus any test file the compiler made you touch, by name
git commit -m "dashboard view: paused and looping"
```

---

### Task 3: wire types and the backend seam

**Files:**
- Modify: `crack-web/src/view.rs` (`PlanView`, `PageState.plan`, `ControlRequest`, `ControlResult`)
- Modify: `crack-web/src/backend.rs` (`Backend::control`, `Backend::plan`)
- Modify: `crack-web/src/lib.rs` (`LiveBackend`)
- Modify: `crack-web/src/test_support.rs` (`FakeBackend`)
- Modify: `crack-web/src/routes.rs` (only to compile: `PageState` construction sites pass a plan; behavior comes in Tasks 4-5)

**Interfaces:**
- Produces:

```rust
// view.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanView { Free, Premium }
impl From<crack_core::guild::plan::Plan> for PlanView;
pub struct PageState<'a> { pub view: &'a QueueView, pub can_control: bool, pub plan: PlanView }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest { Skip { id: Uuid }, Pause, Resume, Repeat { on: bool }, Remove { id: Uuid }, Shuffle }
impl From<ControlRequest> for crack_core::music::remote::Control;

#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResult { Done { view: QueueView }, Conflict { view: QueueView }, NotAllowed,
                         PremiumRequired, GameInProgress, NotPlaying, Failed, TooMany }

// backend.rs
pub use crack_core::music::remote::{Control, ControlRefused};
fn control(&self, user: UserId, g: GuildId, c: Control) -> impl Future<Output = Result<(), ControlRefused>> + Send;
fn plan(&self, g: GuildId) -> impl Future<Output = PlanView> + Send;

// test_support.rs FakeBackend
pub plan: Mutex<PlanView>,                                  // default Premium
pub control_result: Mutex<Result<(), ControlRefused>>,      // default Ok(())
pub controls: Mutex<Vec<(GuildId, UserId, Control)>>,       // every control the routes asked for
pub view_after_control: Mutex<Option<QueueView>>,
pub plan_calls: AtomicUsize,
pub fn control_count(&self) -> usize;
```

- `#[serde(deny_unknown_fields)]` together with `tag` must compile for internally
  tagged enums. If it doesn't, drop it and say so. An unknown `type` is a 400 either
  way.
- `LiveBackend::control` calls `remote::control(...)` and maps `Ok(_)` to `Ok(())`.
  The echo has already been spawned in core.
- `LiveBackend::plan` is `Plan::of(self.deps.data.get_premium(g).await).into()`.
- `FakeBackend::control` records `(g, user, c)` and sleeps 1 ms, as `move_track` does.
  If `control_result` is `Ok` and `view_after_control` is `Some`, that becomes the
  view. It returns `control_result`.

- [ ] **Step 1: Write the failing tests** (view.rs):

```rust
#[test]
fn control_requests_parse_by_type() {
    let p = |s: &str| serde_json::from_str::<ControlRequest>(s);
    assert_eq!(p(r#"{"type":"skip","id":"00000000-0000-0000-0000-000000000005"}"#).unwrap(),
               ControlRequest::Skip { id: Uuid::from_u128(5) });
    assert_eq!(p(r#"{"type":"pause"}"#).unwrap(), ControlRequest::Pause);
    assert_eq!(p(r#"{"type":"repeat","on":true}"#).unwrap(), ControlRequest::Repeat { on: true });
    assert!(p(r#"{"type":"stop"}"#).is_err(), "not a dashboard control");
    assert!(p(r#"{"type":"skip"}"#).is_err(), "skip needs the id it saw playing");
    assert!(p(r#"{"type":"repeat"}"#).is_err(), "repeat is explicit, not a toggle");
}

#[test]
fn answers_and_plan_serialize_tagged() {
    let s = |r: &ControlResult| serde_json::to_string(r).unwrap();
    assert_eq!(s(&ControlResult::PremiumRequired), r#"{"result":"premium_required"}"#);
    assert_eq!(s(&ControlResult::TooMany), r#"{"result":"too_many"}"#);
    let st = PageState { view: &QueueView::Idle, can_control: false, plan: PlanView::Free };
    assert!(serde_json::to_string(&st).unwrap().contains(r#""plan":"free""#));
}

#[test]
fn requests_become_core_controls() {
    use crack_core::music::remote::Control;
    assert_eq!(Control::from(ControlRequest::Remove { id: Uuid::from_u128(3) }), Control::Remove { id: Uuid::from_u128(3) });
    assert_eq!(Control::from(ControlRequest::Skip { id: Uuid::from_u128(1) }), Control::Skip { expect: Uuid::from_u128(1) });
}
```

- [ ] **Step 2: Run them, and see them fail.**
- [ ] **Step 3: Implement** as specified. In routes.rs, the existing `PageState { .. }`
  sites (guild_page, the SSE `event()` helper) need a `plan`. For this task only, pass
  `PlanView::Free` with a `// Task 5 reads the real plan` comment. Task 5 replaces
  both. Do not change behavior here.
- [ ] **Step 4: Run** the crack-web tests and clippy.
- [ ] **Step 5: Sabotage.**
  - Make `Skip`'s `id` optional (`#[serde(default)]`). The "skip needs the id"
    assertion fails.
  - Rename `premium_required`. The answer test fails.
  - Restore both.
- [ ] **Step 6: Commit.**

```bash
git add crack-web/src/view.rs crack-web/src/backend.rs crack-web/src/lib.rs \
  crack-web/src/test_support.rs crack-web/src/routes.rs
git commit -m "dashboard: control and plan wire types, backend seam"
```

---

### Task 4: `POST /g/{g}/control` (gates, rate limit, plan)

**Files:**
- Create: `crack-web/src/limit.rs`
- Modify: `crack-web/src/lib.rs` (`pub mod limit;`, and wiring the limiter into `WebState` where the app is built)
- Modify: `crack-web/src/routes.rs` (`WebState.limiter`, the handler, the route)
- Modify: `crack-web/src/test_support.rs` (`state()` builds a limiter)

**Interfaces:**
- Produces:

```rust
// limit.rs
pub const CONTROLS_PER_WINDOW: usize = 5;
pub const CONTROL_WINDOW: Duration = Duration::from_secs(10);
pub struct RateLimit { window: Duration, max: usize, hits: DashMap<UserId, VecDeque<Instant>> }
impl RateLimit {
    pub fn new(max: usize, window: Duration) -> Self;
    /// Count a hit at `now` and say whether it is within the limit. A refused
    /// hit is not counted.
    pub fn allow(&self, user: UserId, now: Instant) -> bool;
}
// routes.rs
pub struct WebState<B> { ..., pub limiter: Arc<RateLimit> }
```

(Use `dashmap`; crack-web already depends on it, see `access::RoleMemo`. Otherwise use
`std::sync::Mutex<HashMap<..>>`.)

The handler mirrors `move_track` (routes.rs) step for step:

```rust
async fn control<B: Backend>(State(s): State<WebState<B>>, session: Session, Path(raw): Path<String>,
                             headers: HeaderMap, body: Bytes) -> Response {
    let Some(g) = parse_guild(&raw) else { return not_found(); };
    let Some((user, _)) = user_id(session) else { return StatusCode::UNAUTHORIZED.into_response(); };
    if !is_json(&headers) { return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(); }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<ControlRequest>(&body) else { return StatusCode::BAD_REQUEST.into_response(); };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return control_answer(StatusCode::FORBIDDEN, ControlResult::NotAllowed),
        Access::Control => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return control_answer(StatusCode::TOO_MANY_REQUESTS, ControlResult::TooMany);
    }
    if s.backend.plan(g).await != PlanView::Premium {
        return control_answer(StatusCode::FORBIDDEN, ControlResult::PremiumRequired);
    }
    match s.backend.control(user, g, req.into()).await {
        Ok(()) => {
            tracing::info!(guild = %g, user = %user, control = ?req, "dashboard control");
            s.hub.refresh(g).await;
            let view = s.backend.view(g).await;
            control_answer(StatusCode::OK, ControlResult::Done { view })
        },
        Err(ControlRefused::Conflict) => {
            let view = s.backend.view(g).await;
            control_answer(StatusCode::CONFLICT, ControlResult::Conflict { view })
        },
        Err(ControlRefused::GameInProgress) => control_answer(StatusCode::LOCKED, ControlResult::GameInProgress),
        Err(ControlRefused::NotPlaying) => control_answer(StatusCode::CONFLICT, ControlResult::NotPlaying),
        Err(ControlRefused::Failed) => control_answer(StatusCode::INTERNAL_SERVER_ERROR, ControlResult::Failed),
    }
}
```

The route is `.route("/g/{guild}/control", post(control::<B>))`, inside `per_user`, so
it gets `no_store`, the timeout and the security headers.

- [ ] **Step 1: Write the failing tests.**

limit.rs (pure, with an injected clock):

```rust
#[test]
fn five_in_ten_seconds_then_no_and_the_window_slides() {
    let l = RateLimit::new(5, Duration::from_secs(10));
    let (u, t0) = (UserId::new(1), Instant::now());
    for i in 0..5 { assert!(l.allow(u, t0 + Duration::from_millis(i)), "hit {i}"); }
    assert!(!l.allow(u, t0 + Duration::from_secs(1)), "6th within the window");
    assert!(l.allow(UserId::new(2), t0), "per user");
    assert!(l.allow(u, t0 + Duration::from_secs(10) + Duration::from_millis(1)), "oldest slid out");
}

#[test]
fn refused_hits_do_not_extend_the_wait() {
    let l = RateLimit::new(1, Duration::from_secs(10));
    let (u, t0) = (UserId::new(1), Instant::now());
    assert!(l.allow(u, t0));
    for s in 1..10 { assert!(!l.allow(u, t0 + Duration::from_secs(s))); }
    assert!(l.allow(u, t0 + Duration::from_secs(10) + Duration::from_millis(1)));
}
```

routes.rs tests: add a `post_control` helper beside `post_move`, the same shape with
the path `/g/5/control`. Use `controller()` (in voice) and `playing()`, which exist.

```rust
const SKIP: &str = r#"{"type":"skip","id":"00000000-0000-0000-0000-000000000001"}"#;

#[tokio::test]
async fn a_premium_controller_skips_and_the_backend_gets_exactly_that() {
    let fake = controller();
    let r = post_control(fake.clone(), Some(&session(9)), Some("application/json"), Some(ORIGIN), SKIP).await;
    assert_eq!(r.status(), StatusCode::OK);
    let a: Answer = serde_json::from_str(&body(r).await).unwrap();
    assert_eq!(a.result, "done");
    assert_eq!(*fake.controls.lock().unwrap(),
               vec![(GuildId::new(5), UserId::new(9), Control::Skip { expect: Uuid::from_u128(1) })]);
}

#[tokio::test]
async fn refusals_before_the_backend_control_nothing() {
    // For each: no session → 401; text/plain → 415; foreign Origin → 403;
    // `{"type":"stop"}` → 400; not a member → 404; Unknown membership → 503;
    // member not in voice → 403 not_allowed. After all of them:
    // fake.control_count() == 0. Mirror `refusals_before_the_backend_move_nothing`.
}

#[tokio::test]
async fn a_free_server_is_premium_required_and_never_reaches_the_backend() {
    let fake = controller();
    *fake.plan.lock().unwrap() = PlanView::Free;
    let r = post_control(fake.clone(), Some(&session(9)), Some("application/json"), Some(ORIGIN), SKIP).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let a: Answer = serde_json::from_str(&body(r).await).unwrap();
    assert_eq!(a.result, "premium_required");
    assert_eq!(fake.control_count(), 0);
}

#[tokio::test]
async fn the_sixth_control_in_ten_seconds_is_too_many_and_runs_nothing() {
    let fake = controller();
    let app = app(fake.clone()); // ONE app, so one limiter, across the six requests
    // send 5 SKIPs → each 200; the 6th → 429, result "too_many";
    // fake.control_count() == 5.
}

#[tokio::test]
async fn a_stale_skip_is_a_conflict_with_the_fresh_view() {
    let fake = controller();
    *fake.control_result.lock().unwrap() = Err(ControlRefused::Conflict);
    // 409, result "conflict", a.view == Some(playing())
}

#[tokio::test]
async fn backend_refusals_map_to_their_answers() {
    // GameInProgress → 423 game_in_progress; NotPlaying → 409 not_playing; Failed → 500 failed.
}

#[tokio::test]
async fn a_control_reaches_open_watchers_at_once() {
    // Mirror `a_move_reaches_open_watchers_at_once`, using view_after_control.
}

#[tokio::test]
async fn a_move_is_not_rate_limited() {
    // Six moves in a row on one app are all 200. Move stays free and unlimited.
}
```

`app(fake)` builds a new router, and so a new limiter, on every call. That is why the
429 test must reuse one `Router` across requests (`app.clone().oneshot(..)`). Check how
`post_move` builds its request, and add an `app`-taking variant if needed.

- [ ] **Step 2: Run them, and see them fail.**
- [ ] **Step 3: Implement** `limit.rs` and the route. Add a `control_answer(status,
  ControlResult) -> Response` helper beside `answer`. In `WebState`'s `Clone` impl,
  clone `limiter`. In lib.rs, construct the limiter where `WebState` is built for
  production, using `RateLimit::new(CONTROLS_PER_WINDOW, CONTROL_WINDOW)`.
- [ ] **Step 4: Run** the crack-web tests and clippy.
- [ ] **Step 5: Sabotage.**
  - Move the rate-limit check after the plan check. Nothing in the tests should break,
    since the order between those two isn't observable with a premium fake. Make it
    observable: add `a_free_spam_clicker_is_premium_required_until_limited`, or decide
    the order doesn't matter and record that ruling. Your call; state it.
  - Remove the plan check. The free test fails.
  - Count refused hits in `allow`. `refused_hits_do_not_extend_the_wait` fails.
  - Skip `hub.refresh`. The watchers test fails.
  - Restore all of them.
- [ ] **Step 6: Commit.**

```bash
git add crack-web/src/limit.rs crack-web/src/lib.rs crack-web/src/routes.rs crack-web/src/test_support.rs
git commit -m "dashboard: POST /control, gated like move, rate limited, premium only"
```

---

### Task 5: the plan in the page and the stream

**Files:**
- Modify: `crack-web/src/routes.rs` (`guild_page`, `events`, `event()`)

**Behavior:**
- `guild_page` reads `s.backend.plan(g).await` and puts it in `PageState`.
- The SSE task reads the plan once at open, and **re-reads it at every recheck**,
  alongside presence. It sends a new event when either `can_control` or `plan` changes.
- `event(&last, can_control, plan)`.
- A plan read is a cache and DB read with no Discord call. Still, bound it with
  `PRESENCE_TIMEOUT` in the recheck, keeping the old value on a timeout, as presence
  does.

- [ ] **Step 1: Write the failing tests.**

```rust
#[tokio::test]
async fn the_page_and_the_first_event_carry_the_plan() {
    // fake.plan = Free → GET /g/5 body contains `"plan":"free"` in the inlined JSON;
    // the first SSE event (use `first_event`) contains `"plan":"free"`.
}

#[tokio::test]
async fn a_premium_grant_reaches_an_open_stream_at_the_next_recheck() {
    // Mirror `leaving_the_voice_channel_drops_control_within_a_recheck`: open the
    // stream with plan Free, read the first event, set fake.plan = Premium, wait
    // past RECHECK (that test shows how it waits), the next event has "plan":"premium".
}
```

- [ ] **Step 2: Run them, and see them fail.**
- [ ] **Step 3: Implement.** Replace Task 3's two `PlanView::Free` placeholders.
- [ ] **Step 4: Run** the tests and clippy.
- [ ] **Step 5: Sabotage.**
  - Read the plan only at open. The grant test fails.
  - Hard-code Premium in `guild_page`. The page test fails.
  - Restore both.
- [ ] **Step 6: Commit.**

```bash
git add crack-web/src/routes.rs
git commit -m "dashboard: the plan reaches the page and open streams"
```

---

### Task 6: the control bar, docs, version

**Files:**
- Modify: `crack-web/assets/app.js`, `crack-web/assets/app.css`, `crack-web/src/page.rs`
- Modify: `crack-core/src/messaging/messages.rs` (`PREMIUM_CONTROLS`)
- Modify: `docs/web-dashboard.md`, `Cargo.toml` (and `Cargo.lock` via `cargo check`)

**Page (page.rs `queue_page`).** Between `#note` and `#now`, add:

```html
<div id="controls" hidden>
  <button type="button" id="c-pause"></button>
  <button type="button" id="c-skip">⏭ Skip</button>
  <button type="button" id="c-shuffle">🔀 Shuffle</button>
  <button type="button" id="c-repeat" aria-pressed="false">🔁 Repeat</button>
</div>
<p id="premium-controls" hidden>{PREMIUM_CONTROLS} <a href="{PATREON_URL}">CrackTunes Patreon</a></p>
```

`PREMIUM_CONTROLS = "Dashboard controls are a premium feature."` goes in messages.rs,
next to `PREMIUM_HISTORY`. Escape the inserted strings with `esc`, as the history note
does (page.rs ~142).

Add a page.rs test, mirroring `the_premium_note_is_shown_only_when_capped`: the page
contains the `#controls` bar and the note's text and link. JS controls visibility, so
the server test checks only that they are present.

**app.js** (all DOM built with createElement or textContent, never HTML strings):
- `const controlsEl`, `premiumEl`, and the four buttons, by id.
- In `render()`, when `v.state === "playing"`:
  - `controlsEl.hidden = false`;
  - `const premium = state.plan === "premium"`;
  - `const live = premium && state.can_control`;
  - each button's `disabled = !live`;
  - `premiumEl.hidden = premium`;
  - the pause button's text is `v.paused ? "▶ Resume" : "⏸ Pause"`;
  - the repeat button's `aria-pressed = String(!!v.looping)`, plus a `pressed` class;
  - each upcoming row gets a ✕ button (`el("button", "remove", "✕")`, `type="button"`,
    an `aria-label` of `Remove <title>`), disabled unless `live`.

  Otherwise `controlsEl.hidden = true` and `premiumEl.hidden = true`.
- `async function control(body)` mirrors `move()`: POST to `/g/${guild}/control` with
  the same options. The answer handling:

  | result | note shown |
  |---|---|
  | `done` | clear the note |
  | `conflict` | "The queue changed — here it is now." |
  | `not_allowed` | "Join the bot's voice channel to use the controls." |
  | `premium_required` | "Dashboard controls are a premium feature." |
  | `game_in_progress` | "A Guilty Pleasure game is on — the queue is locked." |
  | `not_playing` | "Nothing is playing." |
  | `too_many` | "Slow down a little." |
  | `failed` | "That did not work." |
  | any other status | `That did not work (${status}).` |

  Apply `body.view` as move does, respecting `dragging` and `pending`.
- **Handlers.** Read the current view at click time:
  - pause sends `{type: v.paused ? "resume" : "pause"}`;
  - skip sends `{type: "skip", id: v.now.id}`;
  - shuffle sends `{type: "shuffle"}`;
  - repeat sends `{type: "repeat", on: !v.looping}`;
  - ✕ sends `{type: "remove", id: t.id}`.

  The ✕ click must not start a drag. Sortable's handle is `.handle`, so a button
  outside it is fine; confirm it.
- **Double-click guard.** While a control request is in flight, disable all control
  buttons, and re-enable them in `render()`. The server's `expect` is the real guard;
  this one is courtesy.

**app.css:** a `#controls` flex row with gap, and button styles matching the existing
`#logout` look. `.pressed` and `[disabled]` styles. A `.remove` button that is small,
right-aligned in the row, and not shown on the now-playing row. `#premium-controls`
styled like `#premium-note` (copy its rule).

**Docs:** `docs/web-dashboard.md` gains a "Controls (premium)" section. It covers:
- which controls exist, and that premium is read per request;
- that free servers see them disabled with the Patreon note;
- the in-voice rule;
- the 5-per-10-s limit;
- that each control posts an echo line in Discord, and that move stays silent;
- the `dashboard <op>` audit command values.

Read the doc first and match its voice.

**Version:** `[workspace.package] version = "0.21.0"`, then `cargo check --workspace`.

- [ ] **Step 1:** Write the page.rs test, and see it fail.
- [ ] **Step 2:** Implement page.rs, messages.rs, app.js and app.css.
- [ ] **Step 3:** JS has no harness here, so verify app.js by reading it and by
  `node --check crack-web/assets/app.js` (if node is installed; say if it isn't).
  Grep that no `innerHTML`, `insertAdjacentHTML` or `outerHTML` was introduced.
- [ ] **Step 4:** Update the docs and the version. Run the full verification:
  `cargo +nightly fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, and `cargo test --workspace`.
- [ ] **Step 5: Sabotage** the page test (drop the note). Restore it.
- [ ] **Step 6: Commit** in two commits, by name:
  - `dashboard: control bar, remove buttons, premium note` (page.rs, messages.rs,
    app.js, app.css);
  - `v0.21.0` (Cargo.toml, Cargo.lock, docs/web-dashboard.md).
