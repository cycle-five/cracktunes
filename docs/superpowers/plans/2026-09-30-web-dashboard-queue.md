# Web dashboard, arc 1 (view + reorder the queue) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Guild members sign in with Discord at `dash.cracktun.es`, see their server's queue update live, and — when in the bot's voice channel — reorder it by dragging.

**Architecture:** A new workspace crate `crack-web` (axum) runs inside the bot process, started by `crack-cli` behind a default-on `web` feature. It never touches songbird: crack-core gains a public facade `music::remote` (read the queue, move a track by uuid, list active guilds) that keeps every songbird access behind crack-core's `clippy.toml` bans. Auth is catacombs v0.1.0's website flow. Routes are generic over a `Backend` trait so their logic is tested against a fake; `LiveBackend` is thin glue over `remote`, serenity's cache and HTTP. Live updates: a per-guild `watch` hub polls once a second while anyone watches and pushes whole views over SSE. One JS renderer draws the inlined first view and every event.

**Tech Stack:** Rust 2021, axum 0.8, tower-http 0.6 (`timeout`), tokio + tokio-stream, serde, uuid, catacombs v0.1.0 (git tag), serenity `next`, songbird; vanilla JS + vendored SortableJS 1.15.x.

**Spec:** `docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md`. Prerequisite: `docs/superpowers/plans/2026-09-30-catacombs-website-flow.md` is complete and catacombs `v0.1.0` is tagged.

**Branch:** `feat/web-dashboard` (already exists; the spec is committed on it).

## Global Constraints

- Version bump: root `Cargo.toml` `[workspace.package] version = "0.15.0"` (minor: a feature). The new crate uses `version.workspace = true`.
- Typed serde only; never `serde_json::json!`/`serde_json::Value` for data we own — including tests (deserialize into typed structs to assert).
- `clippy.toml` bans apply to crack-web too: never `Songbird::get`, `TrackHandle::data`; songbird is touched only inside crack-core.
- Lint gate is CI's: `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`, and `cargo fmt --all -- --check`.
- Tests: `cargo test --workspace` must pass without a database or network. **Sabotage every test** — each task lists mutations; the PR body carries the mutation → caught-by table; uncaught mutations are reported. **Assert on what is sent** (the move the backend received, the Discord calls made) and on counts.
- Never print env values or secrets; list key names only.
- Commit trailer, exactly: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`; no other Co-Authored-By.
- Never `git add -A`; add paths explicitly. Worktrees, if used, go outside the repo.
- Web env (spec "Configuration", as refined here): `DISCORD_CLIENT_ID` (falls back to `DISCORD_APP_ID`), `DISCORD_CLIENT_SECRET`, `WEB_PUBLIC_ORIGIN`, `WEB_JWT_SECRET` (≥ 32 chars), `DISCORD_TOKEN` (already required by the bot), optional `WEB_BIND` (default `0.0.0.0:8090`). There is no `WEB_ENCRYPTION_KEY`: catacombs' memory storage stores nothing encrypted, so a random per-process key is generated. Any missing → one WARN naming the missing keys, dashboard off, bot unaffected.
- `MoveRequest` is `{ id, to }` — the spec's `rev` field is dropped: the server never acted on it; the client compares `rev` from the returned view itself.
- `rev` is masked to 53 bits so JavaScript numbers hold it exactly.

## Review Focus

1. **Track titles are hostile text.** A title like `</script><img src=x onerror=alert(1)>` must neither end the inlined JSON element nor execute when rendered. Pinned: Task 5 (inline JSON escaping test) and Task 6 (JS uses `textContent` only — enforced by a test that `app.js` contains no `innerHTML`/`insertAdjacentHTML`/`outerHTML`).
2. **Malformed guild ids in URLs** (`/g/0`, `/g/abc`, `/g/99999999999999999999`) must 404, never panic (`GuildId::new(0)` panics). Pinned: Task 5.
3. **A drag that races a track change** (the dragged track finished, or is now playing) must move nothing and re-render from the server's view. Pinned: Task 1 (`Absent`/`NowPlaying` leave the queue untouched) and Task 6 (409 `conflict` carries a view).
4. **Viewers of large guilds** whose member is not in the partial cache must still see the page (via the HTTP member lookup), and a Discord 5xx must not be remembered as "not a member". Pinned: Task 3.
5. **Leaving the voice channel while the page is open** removes the drag handles within one recheck, and leaving the guild closes the stream. Pinned: Task 6 (SSE recheck tests).

---

### Task 1: crack-core — `music::remote`, and moving a track by id

**Files:**
- Create: `crack-core/src/music/remote.rs`
- Modify: `crack-core/src/music/mod.rs` (declare `pub mod remote;`)
- Modify: `crack-core/src/music/queue.rs` (add `move_track_by_id` after `move_track` at ~line 816; tests in its `mod test`)
- Modify: `crack-core/Cargo.toml` (add `uuid = "1"`)

**Interfaces:**
- Consumes: `Data::lock_queue`, `Data::playback_owner`, `Data::claim_playback` (tests), `commands::music_utils::connected_call` (crate-private), `handlers::track_end::update_queue_messages`, `utils::{get_track_handle_metadata, get_requesting_user}`, queue test helper `enqueue_input_back`.
- Produces (all in `crack_core::music::remote`, public):
  ```rust
  pub enum Requester { Auto, User(UserId) }
  pub struct TrackSummary { pub id: Uuid, pub title: Option<String>, pub url: Option<String>,
                            pub duration: Option<Duration>, pub requester: Option<Requester> }
  pub enum QueueState { Idle, Hidden, Playing { bot_channel: ChannelId, tracks: Vec<TrackSummary> } }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum MoveRefused { NotPlaying, GameInProgress, Absent, NowPlaying }
  pub async fn queue_state(data: &Data, guild_id: GuildId) -> QueueState;
  pub async fn bot_channel(data: &Data, guild_id: GuildId) -> Option<ChannelId>;
  pub async fn active_guilds(data: &Data) -> Vec<(GuildId, ChannelId)>;
  pub async fn move_by_id(data: Arc<Data>, http: &Http, guild_id: GuildId, id: Uuid, to_upcoming: usize) -> Result<usize, MoveRefused>;
  ```
  and in `music::queue` (crate-visible, like its neighbours): `pub fn move_track_by_id(guard: &QueueGuard, handler: &Call, id: Uuid, to_upcoming: usize) -> Result<usize, MoveRefused>`. `to_upcoming` is 0-based within the tracks *after* the one playing; the `Ok` value is the clamped position actually used.

- [ ] **Step 1: Write the failing `move_track_by_id` tests**

Add to `crack-core/Cargo.toml` `[dependencies]`: `uuid = "1"` (songbird's `TrackHandle::uuid()` returns `uuid::Uuid`; already in the lockfile at 1.26).

Add `pub mod remote;` to `crack-core/src/music/mod.rs`, and create `crack-core/src/music/remote.rs` containing only:

```rust
//! Queue operations for callers with no poise `Context` -- the web dashboard.

/// Why a move was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefused {
    /// The bot is not connected in this guild.
    NotPlaying,
    /// A `/gp` game owns playback.
    GameInProgress,
    /// No track with that id is queued (it finished, or was removed).
    Absent,
    /// That track is the one playing; only upcoming tracks move.
    NowPlaying,
}
```

In `crack-core/src/music/queue.rs`'s `mod test`, add:

```rust
    async fn queue_of(n: usize) -> (Data, Arc<Mutex<Call>>, Vec<uuid::Uuid>) {
        let data = Data(Arc::new(DataInner::default()));
        let call = offline_call();
        let mut ids = Vec::new();
        {
            let guard = data.lock_queue(GUILD, PlaybackOwner::Free).await.unwrap();
            for i in 0..n {
                let file = format!("/nonexistent/{i}.opus");
                let h = enqueue_input_back(
                    &guard,
                    &call,
                    songbird::input::File::new(file).into(),
                    Some(titled(&format!("t{i}"))),
                    None,
                )
                .await;
                ids.push(h.uuid());
            }
        }
        (data, call, ids)
    }

    async fn order(call: &Arc<Mutex<Call>>) -> Vec<uuid::Uuid> {
        call.lock()
            .await
            .queue()
            .current_queue()
            .iter()
            .map(|h| h.uuid())
            .collect()
    }

    async fn move_in(
        data: &Data,
        call: &Arc<Mutex<Call>>,
        id: uuid::Uuid,
        to: usize,
    ) -> Result<usize, crate::music::remote::MoveRefused> {
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free).await.unwrap();
        let handler = call.lock().await;
        move_track_by_id(&guard, &handler, id, to)
    }

    #[tokio::test]
    async fn a_track_moves_by_id_to_an_upcoming_position() {
        let (data, call, ids) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[3], 0).await, Ok(0));
        assert_eq!(order(&call).await, vec![ids[0], ids[3], ids[1], ids[2]]);
    }

    #[tokio::test]
    async fn a_target_past_the_end_lands_last() {
        let (data, call, ids) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[1], 99).await, Ok(2));
        assert_eq!(order(&call).await, vec![ids[0], ids[2], ids[3], ids[1]]);
    }

    #[tokio::test]
    async fn the_playing_track_does_not_move() {
        use crate::music::remote::MoveRefused;
        let (data, call, ids) = queue_of(3).await;
        assert_eq!(move_in(&data, &call, ids[0], 1).await, Err(MoveRefused::NowPlaying));
        assert_eq!(order(&call).await, ids);
    }

    #[tokio::test]
    async fn an_id_no_longer_queued_moves_nothing() {
        use crate::music::remote::MoveRefused;
        let (data, call, ids) = queue_of(3).await;
        let gone = uuid::Uuid::from_u128(42);
        assert_eq!(move_in(&data, &call, gone, 0).await, Err(MoveRefused::Absent));
        assert_eq!(order(&call).await, ids);
    }
```

Run: `cargo test -p crack-core music::queue::test::a_track_moves` — Expected: FAIL to compile (`move_track_by_id` not found).

- [ ] **Step 2: Implement `move_track_by_id`**

In `crack-core/src/music/queue.rs`, directly after `move_track`:

```rust
/// Move a track, named by its id, to a position among the upcoming tracks.
/// Used by the web dashboard, whose view of the queue can be seconds old:
/// an index would move whichever track now sits there, an id cannot.
///
/// `to_upcoming` is 0-based among the tracks after the one playing and is
/// clamped into range; the position actually used is returned. The playing
/// track never moves, and an id no longer queued moves nothing.
///
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
pub fn move_track_by_id(
    guard: &QueueGuard,
    handler: &Call,
    id: uuid::Uuid,
    to_upcoming: usize,
) -> Result<usize, crate::music::remote::MoveRefused> {
    use crate::music::remote::MoveRefused;
    let _ = guard;
    handler.queue().modify_queue(|queue| {
        let at = queue
            .iter()
            .position(|q| q.uuid() == id)
            .ok_or(MoveRefused::Absent)?;
        if at == 0 {
            return Err(MoveRefused::NowPlaying);
        }
        // `at >= 1` means at least two tracks, so the range is never empty.
        let to = (to_upcoming + 1).clamp(1, queue.len() - 1);
        let track = queue.remove(at).expect("the position came from this queue");
        queue.insert(to, track);
        Ok(to - 1)
    })
}
```

Run: `cargo test -p crack-core music::queue::test` — Expected: all pass (the four new ones included).

- [ ] **Step 3: Sabotage `move_track_by_id`**

Each must fail the named test; revert after each:
| Mutation | Must fail |
|---|---|
| remove the `if at == 0` branch | `the_playing_track_does_not_move` |
| `clamp(1, queue.len() - 1)` → `clamp(0, queue.len() - 1)` and `to_upcoming + 1` → `to_upcoming` | `a_track_moves_by_id_to_an_upcoming_position` |
| return `Ok(to)` instead of `Ok(to - 1)` | `a_target_past_the_end_lands_last` |
| on absent id, `Ok(0)` | `an_id_no_longer_queued_moves_nothing` |

- [ ] **Step 4: Write the failing `remote` tests**

Append to `crack-core/src/music/remote.rs`:

```rust
#[cfg(test)]
mod test {
    use super::*;
    use crate::{Data, DataInner};
    use serenity::all::{GuildId, Http};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);

    fn data() -> Data {
        Data(Arc::new(DataInner::default()))
    }

    #[tokio::test]
    async fn a_game_hides_the_queue_before_anything_else_is_asked() {
        let d = data();
        d.claim_playback(G, crate::music::PlaybackOwner::Game).unwrap();
        assert!(matches!(queue_state(&d, G).await, QueueState::Hidden));
    }

    #[tokio::test]
    async fn no_call_is_idle() {
        assert!(matches!(queue_state(&data(), G).await, QueueState::Idle));
        assert_eq!(bot_channel(&data(), G).await, None);
        assert!(active_guilds(&data()).await.is_empty());
    }

    #[tokio::test]
    async fn a_game_refuses_a_move_before_the_call_is_looked_up() {
        let d = Arc::new(data());
        d.claim_playback(G, crate::music::PlaybackOwner::Game).unwrap();
        let http = Http::new(crack_types::get_valid_token());
        let got = move_by_id(d, &http, G, uuid::Uuid::from_u128(1), 0).await;
        assert_eq!(got, Err(MoveRefused::GameInProgress));
    }

    #[tokio::test]
    async fn a_move_with_no_call_is_not_playing() {
        let http = Http::new(crack_types::get_valid_token());
        let got = move_by_id(Arc::new(data()), &http, G, uuid::Uuid::from_u128(1), 0).await;
        assert_eq!(got, Err(MoveRefused::NotPlaying));
    }
}
```

and in `crack-core/src/music/queue.rs`'s `mod test` (it has `enqueue_input_back` and the fixtures):

```rust
    #[tokio::test]
    async fn a_summary_carries_the_id_title_and_who_asked() {
        use crate::music::remote::{summarize, Requester};
        let data = Data(Arc::new(DataInner::default()));
        let call = offline_call();
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free).await.unwrap();
        let auto = enqueue_input_back(
            &guard,
            &call,
            songbird::input::File::new("/nonexistent/a.opus").into(),
            Some(titled("Auto Pick")),
            None,
        )
        .await;
        let asked = queue_track_ready_front(
            &guard,
            &call,
            TrackReadyData {
                source: songbird::input::File::new("/nonexistent/b.opus").into(),
                metadata: NewAuxMetadata(titled("Asked For")),
                user_id: Some(UserId::new(7)),
                username: None,
            },
        )
        .await
        .unwrap();
        drop(guard);

        let handles = call.lock().await.queue().current_queue();
        let s = summarize(&handles).await;

        let by_id = |id| s.iter().find(|t| t.id == id).expect("summarized");
        assert_eq!(by_id(auto.uuid()).title.as_deref(), Some("Auto Pick"));
        assert!(matches!(by_id(auto.uuid()).requester, Some(Requester::Auto)));
        let asked = asked.last().unwrap();
        assert_eq!(by_id(asked.uuid()).title.as_deref(), Some("Asked For"));
        assert!(matches!(
            by_id(asked.uuid()).requester,
            Some(Requester::User(u)) if u == UserId::new(7)
        ));
    }

    #[tokio::test]
    async fn an_offline_call_with_tracks_is_idle_not_playing() {
        use crate::music::remote::{state_of_call, QueueState};
        let (_data, call, _ids) = queue_of(2).await;
        // No voice connection means no channel, and a dashboard cannot say
        // who may control a queue with no channel.
        assert!(matches!(state_of_call(&call).await, QueueState::Idle));
    }
```

Run: `cargo test -p crack-core remote` — Expected: FAIL to compile.

- [ ] **Step 5: Implement `remote`**

Replace `crack-core/src/music/remote.rs`'s contents above `MoveRefused` so the file reads (keep `MoveRefused` and the tests):

```rust
//! Queue operations for callers with no poise `Context` -- the web dashboard.
//!
//! Everything that touches songbird for the dashboard lives here, inside
//! crack-core, so it stays behind this crate's `clippy.toml` bans (no
//! `Songbird::get`, no `TrackHandle::data`). crack-web only sees the plain
//! types below.

use crate::{
    commands::music_utils::connected_call,
    handlers::track_end::update_queue_messages,
    music::{move_track_by_id, PlaybackOwner},
    utils::{get_requesting_user, get_track_handle_metadata},
    CrackedError, Data,
};
use serenity::all::{ChannelId, GuildId, Http, UserId};
use songbird::{tracks::TrackHandle, Call};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Who queued a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requester {
    /// Autoplay picked it (stored as user id 1, see `requesting_user_to_string`).
    Auto,
    User(UserId),
}

/// One queued track, as the dashboard shows it.
#[derive(Debug, Clone)]
pub struct TrackSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub url: Option<String>,
    pub duration: Option<Duration>,
    pub requester: Option<Requester>,
}

/// A guild's queue, as the dashboard may show it.
#[derive(Debug, Clone)]
pub enum QueueState {
    /// Not connected, or nothing queued.
    Idle,
    /// A `/gp` game owns playback: the queue would give the answers away.
    Hidden,
    /// `tracks[0]` is playing; `bot_channel` is where.
    Playing {
        bot_channel: ChannelId,
        tracks: Vec<TrackSummary>,
    },
}

/// The queue in `guild_id`. A game hides it before the call is even looked up.
pub async fn queue_state(data: &Data, guild_id: GuildId) -> QueueState {
    if data.playback_owner(guild_id) != PlaybackOwner::Free {
        return QueueState::Hidden;
    }
    match connected_call(&data.songbird, guild_id, None).await {
        Some(call) => state_of_call(&call).await,
        None => QueueState::Idle,
    }
}

/// The queue on one call. The call lock is held only to clone the handles;
/// metadata is read after it is released.
pub(crate) async fn state_of_call(call: &Arc<Mutex<Call>>) -> QueueState {
    let (channel, handles) = {
        let handler = call.lock().await;
        (handler.current_channel(), handler.queue().current_queue())
    };
    match channel {
        Some(channel) if !handles.is_empty() => QueueState::Playing {
            bot_channel: ChannelId::new(channel.get()),
            tracks: summarize(&handles).await,
        },
        _ => QueueState::Idle,
    }
}

/// Read what the dashboard shows from each handle.
pub(crate) async fn summarize(handles: &[TrackHandle]) -> Vec<TrackSummary> {
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        // No metadata is a blank row, not an error: `/queue` does the same.
        let meta = get_track_handle_metadata(handle).await.unwrap_or_default();
        let requester = get_requesting_user(handle).await.ok().map(|u| {
            if u.get() == 1 {
                Requester::Auto
            } else {
                Requester::User(u)
            }
        });
        out.push(TrackSummary {
            id: handle.uuid(),
            title: meta.title,
            url: meta.source_url,
            duration: meta.duration,
            requester,
        });
    }
    out
}

/// The voice channel the bot is connected to in `guild_id`, if any.
pub async fn bot_channel(data: &Data, guild_id: GuildId) -> Option<ChannelId> {
    let call = connected_call(&data.songbird, guild_id, None).await?;
    let channel = call.lock().await.current_channel()?;
    Some(ChannelId::new(channel.get()))
}

/// Every guild with a connected call, and the channel it is in.
pub async fn active_guilds(data: &Data) -> Vec<(GuildId, ChannelId)> {
    let calls: Vec<_> = data.songbird.iter().collect();
    let mut out = Vec::new();
    for (guild_id, call) in calls {
        let handler = call.lock().await;
        if handler.current_connection().is_none() {
            continue;
        }
        if let Some(channel) = handler.current_channel() {
            out.push((GuildId::new(guild_id.get()), ChannelId::new(channel.get())));
        }
    }
    out
}

/// Move a track by id, then refresh the queue messages in Discord. Posts no
/// reply: a drag is silent in the channel (owner's decision).
pub async fn move_by_id(
    data: Arc<Data>,
    http: &Http,
    guild_id: GuildId,
    id: Uuid,
    to_upcoming: usize,
) -> Result<usize, MoveRefused> {
    // The lease first: a game refuses at once, before the call is touched.
    let guard = data
        .lock_queue(guild_id, PlaybackOwner::Free)
        .await
        .map_err(|e| match e {
            CrackedError::GameInProgress => MoveRefused::GameInProgress,
            other => {
                tracing::warn!("lock_queue refused a dashboard move: {other}");
                MoveRefused::GameInProgress
            },
        })?;
    let call = connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(MoveRefused::NotPlaying)?;
    let handler = call.lock().await;
    let moved = move_track_by_id(&guard, &handler, id, to_upcoming);
    // Held only for the mutation, as every command does -- see lease.rs.
    drop(guard);
    let queue = handler.queue().current_queue();
    drop(handler);
    if moved.is_ok() {
        update_queue_messages(http, data.clone(), &queue, guild_id).await;
    }
    moved
}
```

If `songbird::id::GuildId`/`ChannelId` expose the value differently than `.get()` (they do expose `get` via a macro in `songbird/src/id.rs`), adjust. If `update_queue_messages` wants `&impl CacheHttp`, `&Http` satisfies it (`impl CacheHttp for Http`). Make `summarize` and `state_of_call` visible to queue.rs's tests: they are `pub(crate)` already.

Run: `cargo test -p crack-core remote && cargo test -p crack-core music::queue::test` — Expected: all pass.

- [ ] **Step 6: Sabotage `remote`**

| Mutation | Must fail |
|---|---|
| `queue_state`: move the owner check after `connected_call` and return `Idle` when no call | `a_game_hides_the_queue_before...` |
| `move_by_id`: look up the call before taking the lease | `a_game_refuses_a_move_before_the_call...` |
| `summarize`: `Requester::User(u)` for every id | `a_summary_carries_the_id_title_and_who_asked` |
| `state_of_call`: `Some(_) \| None if !handles.is_empty()` → Playing with a dummy channel | `an_offline_call_with_tracks_is_idle_not_playing` |

Not caught by any test (report in the PR): `update_queue_messages` being skipped after a successful move, and `active_guilds`' connected filter — both need a live voice connection and are covered by the TuneTitan check in Task 8.

- [ ] **Step 7: Lint and commit**

```bash
cargo fmt --all && cargo clippy -p crack-core -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-core/Cargo.toml Cargo.lock crack-core/src/music/mod.rs crack-core/src/music/remote.rs crack-core/src/music/queue.rs
git commit -m "feat(core): music::remote -- the queue for callers with no poise Context; move a track by id

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: crack-web — the crate, its config, and the wire types

**Files:**
- Modify: `Cargo.toml` (workspace `members` += `"crack-web"`)
- Create: `crack-web/Cargo.toml`
- Create: `crack-web/src/lib.rs`
- Create: `crack-web/src/config.rs`
- Create: `crack-web/src/view.rs`

**Interfaces:**
- Consumes: `crack_core::music::remote::{QueueState, TrackSummary, Requester}` (Task 1); `catacombs::{Config, DiscordConfig, SecurityConfig, ServerConfig, WebConfig}` (catacombs v0.1.0).
- Produces:
  - `crack_web::config::WebEnv { client_id, client_secret, public_origin, jwt_secret, bot_token, bind: String }`, `WebEnv::from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<WebEnv, Vec<&'static str>>`, `WebEnv::redirect_uri(&self) -> String`, `WebEnv::catacombs_config(&self) -> catacombs::Config`, `pub const DEFAULT_BIND: &str = "0.0.0.0:8090"`.
  - `crack_web::view::{TrackView, QueueView, PageState, MoveRequest, MoveResult, REV_MASK, rev_of, view_from_state}`:
    ```rust
    pub struct TrackView { pub id: Uuid, pub title: String, pub url: Option<String>, pub duration_secs: Option<u64>, pub requester: Option<String> }
    #[serde(tag = "state", rename_all = "snake_case")]
    pub enum QueueView { Idle, Hidden, Playing { now: TrackView, upcoming: Vec<TrackView>, rev: u64 } }
    pub struct PageState<'a> { pub view: &'a QueueView, pub can_control: bool }
    pub struct MoveRequest { pub id: Uuid, pub to: usize }
    #[serde(tag = "result", rename_all = "snake_case")]
    pub enum MoveResult { Moved { view: QueueView }, Conflict { view: QueueView }, NotAllowed, GameInProgress, NotPlaying }
    pub fn view_from_state(state: QueueState, name_of: impl Fn(UserId) -> Option<String>) -> QueueView
    ```

- [ ] **Step 1: Scaffold the crate**

Add `"crack-web",` to the root `Cargo.toml` `members` list (after `"crack-voting"`). Create `crack-web/Cargo.toml`:

```toml
[package]
name = "crack-web"
version.workspace = true
edition = "2021"
authors = ["Cycle Five <cycle.five@proton.me>"]
publish = true
license = "MIT"
description = "The Crack Tunes web dashboard: view and reorder a guild's queue."
homepage = "https://cracktun.es/"
repository = "https://github.com/cycle-five/cracktunes"
workspace = ".."

[dependencies]
crack-core = { path = "../crack-core" }
# The website login flow (v0.1.0). Memory storage: nothing about web users is
# persisted in this arc, and SqlxStorage would migrate into our own
# `_sqlx_migrations` table.
catacombs = { git = "https://github.com/cycle-five/catacombs", tag = "v0.1.0", default-features = false, features = ["memory-storage", "rustls-tls"] }
axum = { version = "0.8", features = ["macros"] }
tower-http = { version = "0.6", features = ["timeout"] }
tokio = { workspace = true }
tokio-stream = { version = "0.1", features = ["sync"] }
serenity = { workspace = true }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_urlencoded = "0.7"
uuid = { version = "1", features = ["serde", "v4"] }
dashmap = "6"
tracing = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
crack-types = { path = "../crack-types" }
```

Create `crack-web/src/lib.rs`:

```rust
//! The Crack Tunes web dashboard (arc 1): view a guild's queue live, and
//! reorder it from the bot's voice channel. Runs inside the bot process; see
//! docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md.

pub mod config;
pub mod view;
```

Run: `cargo check -p crack-web` — Expected: success (downloads catacombs at the tag).

- [ ] **Step 2: Write the failing config tests**

Create `crack-web/src/config.rs` with the tests first:

```rust
//! The dashboard's environment. Missing keys switch the dashboard off; they
//! never stop the bot.

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;

    const SECRET32: &str = "0123456789abcdef0123456789abcdef";

    fn full() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DISCORD_CLIENT_ID", "111".to_string()),
            ("DISCORD_CLIENT_SECRET", "shh".to_string()),
            ("WEB_PUBLIC_ORIGIN", "https://dash.cracktun.es/".to_string()),
            ("WEB_JWT_SECRET", SECRET32.to_string()),
            ("DISCORD_TOKEN", "bot".to_string()),
        ])
    }

    fn load(env: &HashMap<&'static str, String>) -> Result<WebEnv, Vec<&'static str>> {
        WebEnv::from_lookup(|k| env.get(k).cloned())
    }

    #[test]
    fn a_full_environment_loads_with_defaults() {
        let env = load(&full()).unwrap();
        assert_eq!(env.public_origin, "https://dash.cracktun.es");
        assert_eq!(env.redirect_uri(), "https://dash.cracktun.es/auth/callback");
        assert_eq!(env.bind, DEFAULT_BIND);
        let c = env.catacombs_config();
        assert_eq!(c.discord.client_id, "111");
        assert_eq!(c.discord.redirect_uri, "https://dash.cracktun.es/auth/callback");
        assert_eq!(c.web.scopes, vec!["identify".to_string()]);
        assert!(c.web.secure_cookies);
    }

    #[test]
    fn the_app_id_stands_in_for_a_missing_client_id() {
        let mut e = full();
        e.remove("DISCORD_CLIENT_ID");
        e.insert("DISCORD_APP_ID", "222".to_string());
        assert_eq!(load(&e).unwrap().client_id, "222");
    }

    #[test]
    fn every_missing_key_is_named_and_blank_counts_as_missing() {
        let mut e = full();
        e.remove("DISCORD_CLIENT_SECRET");
        e.insert("WEB_PUBLIC_ORIGIN", "  ".to_string());
        e.remove("DISCORD_CLIENT_ID");
        let missing = load(&e).unwrap_err();
        assert_eq!(
            missing,
            vec!["DISCORD_CLIENT_ID", "DISCORD_CLIENT_SECRET", "WEB_PUBLIC_ORIGIN"]
        );
    }

    #[test]
    fn a_short_jwt_secret_is_refused() {
        let mut e = full();
        e.insert("WEB_JWT_SECRET", "short".to_string());
        assert_eq!(load(&e).unwrap_err(), vec!["WEB_JWT_SECRET (32+ characters)"]);
    }

    #[test]
    fn a_plain_http_origin_is_refused_except_localhost() {
        let mut e = full();
        e.insert("WEB_PUBLIC_ORIGIN", "http://dash.cracktun.es".to_string());
        assert_eq!(
            load(&e).unwrap_err(),
            vec!["WEB_PUBLIC_ORIGIN (https://, or http://localhost)"]
        );
        e.insert("WEB_PUBLIC_ORIGIN", "http://localhost:8090".to_string());
        assert!(load(&e).is_ok());
    }

    #[test]
    fn web_bind_overrides_the_default() {
        let mut e = full();
        e.insert("WEB_BIND", "127.0.0.1:9000".to_string());
        assert_eq!(load(&e).unwrap().bind, "127.0.0.1:9000");
    }
}
```

Run: `cargo test -p crack-web config` — Expected: FAIL to compile.

- [ ] **Step 3: Implement `WebEnv`**

Above the tests in `crack-web/src/config.rs`:

```rust
/// Where the dashboard listens unless `WEB_BIND` says otherwise. Not 8080:
/// the edge proxy already sends the bots VM's 8080 to crack-voting.
pub const DEFAULT_BIND: &str = "0.0.0.0:8090";

/// The dashboard's configuration, read from the environment.
#[derive(Clone)]
pub struct WebEnv {
    pub client_id: String,
    pub client_secret: String,
    /// e.g. `https://dash.cracktun.es`, no trailing slash. Builds the OAuth
    /// redirect and is the only `Origin` a move is accepted from.
    pub public_origin: String,
    pub jwt_secret: String,
    pub bot_token: String,
    pub bind: String,
}

// Hand-written: the derived Debug would print the secrets.
impl std::fmt::Debug for WebEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebEnv")
            .field("client_id", &self.client_id)
            .field("public_origin", &self.public_origin)
            .field("bind", &self.bind)
            .finish_non_exhaustive()
    }
}

impl WebEnv {
    /// Read the configuration through `get`. On failure, returns the name of
    /// every missing or unusable key -- names only, never values.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, Vec<&'static str>> {
        let val = |k: &str| get(k).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let mut missing = Vec::new();

        let client_id = val("DISCORD_CLIENT_ID").or_else(|| val("DISCORD_APP_ID"));
        if client_id.is_none() {
            missing.push("DISCORD_CLIENT_ID");
        }
        let client_secret = val("DISCORD_CLIENT_SECRET");
        if client_secret.is_none() {
            missing.push("DISCORD_CLIENT_SECRET");
        }
        let public_origin = val("WEB_PUBLIC_ORIGIN").map(|o| o.trim_end_matches('/').to_owned());
        match &public_origin {
            None => missing.push("WEB_PUBLIC_ORIGIN"),
            Some(o)
                if !(o.starts_with("https://")
                    || o.starts_with("http://localhost")
                    || o.starts_with("http://127.0.0.1")) =>
            {
                missing.push("WEB_PUBLIC_ORIGIN (https://, or http://localhost)")
            },
            Some(_) => {},
        }
        let jwt_secret = val("WEB_JWT_SECRET");
        match &jwt_secret {
            None => missing.push("WEB_JWT_SECRET"),
            Some(s) if s.len() < 32 => missing.push("WEB_JWT_SECRET (32+ characters)"),
            Some(_) => {},
        }
        let bot_token = val("DISCORD_TOKEN");
        if bot_token.is_none() {
            missing.push("DISCORD_TOKEN");
        }
        if !missing.is_empty() {
            return Err(missing);
        }
        Ok(Self {
            client_id: client_id.unwrap(),
            client_secret: client_secret.unwrap(),
            public_origin: public_origin.unwrap(),
            jwt_secret: jwt_secret.unwrap(),
            bot_token: bot_token.unwrap(),
            bind: val("WEB_BIND").unwrap_or_else(|| DEFAULT_BIND.to_owned()),
        })
    }

    /// The OAuth redirect: catacombs' callback, mounted at `/auth`.
    pub fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_origin)
    }

    /// catacombs' configuration. Built here rather than by
    /// `catacombs::Config::from_env`, whose variable names differ from ours.
    pub fn catacombs_config(&self) -> catacombs::Config {
        catacombs::Config {
            discord: catacombs::DiscordConfig {
                client_id: self.client_id.clone(),
                client_secret: self.client_secret.clone(),
                redirect_uri: self.redirect_uri(),
                bot_token: self.bot_token.clone(),
                premium_sku_id: None,
                api_base: catacombs::config::default_api_base(),
            },
            security: catacombs::SecurityConfig {
                jwt_secret: self.jwt_secret.clone(),
                // Memory storage keeps nothing encrypted; the field is
                // required, so it gets a per-process random value.
                encryption_key: format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                ),
            },
            server: catacombs::ServerConfig::default(),
            web: catacombs::WebConfig::default(),
        }
    }
}
```

Add `pub mod config;` is already in lib.rs. If `catacombs::config` is not a public module path, use the re-export (`catacombs::config::default_api_base` is declared `pub fn` in catacombs Task 2; `pub mod config` is in its `lib.rs`).

Run: `cargo test -p crack-web config` — Expected: 6 passed.
Sabotage: drop `.filter(|v| !v.is_empty())` → `every_missing_key...` fails; drop `trim_end_matches('/')` → `a_full_environment...` fails; drop the `DISCORD_APP_ID` fallback → `the_app_id_stands_in...` fails. Revert each.

- [ ] **Step 4: Write the failing view tests**

Create `crack-web/src/view.rs` with tests first:

```rust
//! The dashboard's wire types -- typed serde, one enum per direction -- and
//! the conversion from crack-core's `QueueState`.

#[cfg(test)]
mod test {
    use super::*;
    use crack_core::music::remote::{QueueState, Requester, TrackSummary};
    use serenity::all::{ChannelId, UserId};
    use std::time::Duration;

    fn t(n: u128, title: Option<&str>, requester: Option<Requester>) -> TrackSummary {
        TrackSummary {
            id: Uuid::from_u128(n),
            title: title.map(str::to_owned),
            url: Some(format!("https://example.com/{n}")),
            duration: Some(Duration::from_secs(61)),
            requester,
        }
    }

    fn playing(tracks: Vec<TrackSummary>) -> QueueState {
        QueueState::Playing { bot_channel: ChannelId::new(9), tracks }
    }

    fn names(u: UserId) -> Option<String> {
        (u == UserId::new(7)).then(|| "Seven".to_owned())
    }

    #[test]
    fn the_first_track_is_now_and_the_rest_are_upcoming() {
        let v = view_from_state(
            playing(vec![
                t(1, Some("A"), Some(Requester::User(UserId::new(7)))),
                t(2, None, Some(Requester::Auto)),
                t(3, Some("C"), Some(Requester::User(UserId::new(8)))),
            ]),
            names,
        );
        let QueueView::Playing { now, upcoming, .. } = v else { panic!("playing") };
        assert_eq!(now.title, "A");
        assert_eq!(now.requester.as_deref(), Some("Seven"));
        assert_eq!(now.duration_secs, Some(61));
        assert_eq!(upcoming.len(), 2);
        assert_eq!(upcoming[0].title, "Unknown title");
        assert_eq!(upcoming[0].requester.as_deref(), Some("(auto)"));
        assert_eq!(upcoming[1].requester, None, "an uncached user has no name");
    }

    #[test]
    fn rev_follows_the_order_and_nothing_else() {
        let a = rev_of(&[t(1, Some("A"), None), t(2, Some("B"), None)]);
        let same_ids_new_titles = rev_of(&[t(1, Some("x"), None), t(2, Some("y"), None)]);
        let swapped = rev_of(&[t(2, Some("B"), None), t(1, Some("A"), None)]);
        assert_eq!(a, same_ids_new_titles);
        assert_ne!(a, swapped);
    }

    #[test]
    fn rev_fits_in_a_javascript_number() {
        for n in 0..200u128 {
            assert!(rev_of(&[t(n, None, None)]) <= REV_MASK);
        }
    }

    #[test]
    fn idle_hidden_and_an_empty_playing_state_map_across() {
        assert_eq!(view_from_state(QueueState::Idle, names), QueueView::Idle);
        assert_eq!(view_from_state(QueueState::Hidden, names), QueueView::Hidden);
        assert_eq!(view_from_state(playing(vec![]), names), QueueView::Idle);
    }

    #[test]
    fn the_wire_format_is_tagged_snake_case() {
        #[derive(serde::Deserialize)]
        struct Tagged {
            state: String,
        }
        let hidden: Tagged =
            serde_json::from_str(&serde_json::to_string(&QueueView::Hidden).unwrap()).unwrap();
        assert_eq!(hidden.state, "hidden");

        #[derive(serde::Deserialize)]
        struct Res {
            result: String,
        }
        let r: Res =
            serde_json::from_str(&serde_json::to_string(&MoveResult::NotAllowed).unwrap()).unwrap();
        assert_eq!(r.result, "not_allowed");

        let req: MoveRequest = serde_json::from_str(
            r#"{"id":"00000000-0000-0000-0000-000000000005","to":3}"#,
        )
        .unwrap();
        assert_eq!((req.id, req.to), (Uuid::from_u128(5), 3));
    }
}
```

Add `pub mod view;` (already in lib.rs). Run: `cargo test -p crack-web view` — Expected: FAIL to compile.

- [ ] **Step 5: Implement the view types**

Above the tests in `crack-web/src/view.rs`:

```rust
use crack_core::music::remote::{QueueState, Requester, TrackSummary};
use serde::{Deserialize, Serialize};
use serenity::all::UserId;
use std::hash::{DefaultHasher, Hash, Hasher};
use uuid::Uuid;

/// `rev` is masked to 53 bits: JavaScript numbers are exact up to 2^53, and
/// the browser echoes `rev` back when it compares views.
pub const REV_MASK: u64 = (1 << 53) - 1;

/// One track as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackView {
    pub id: Uuid,
    pub title: String,
    pub url: Option<String>,
    pub duration_secs: Option<u64>,
    pub requester: Option<String>,
}

/// A guild's queue as the page shows it. Sent whole, never as a diff: a
/// client that missed an event is right again after the next one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum QueueView {
    Idle,
    Hidden,
    Playing {
        now: TrackView,
        upcoming: Vec<TrackView>,
        rev: u64,
    },
}

/// What the page renders: the shared view plus this viewer's permission.
/// Inlined into the page and sent as every SSE event.
#[derive(Debug, Serialize)]
pub struct PageState<'a> {
    pub view: &'a QueueView,
    pub can_control: bool,
}

/// `POST /g/{id}/move`: move track `id` to position `to` among the upcoming
/// tracks (0-based).
#[derive(Debug, Deserialize)]
pub struct MoveRequest {
    pub id: Uuid,
    pub to: usize,
}

/// The answer to a move. `Moved` and `Conflict` carry the view to render.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MoveResult {
    Moved { view: QueueView },
    Conflict { view: QueueView },
    NotAllowed,
    GameInProgress,
    NotPlaying,
}

/// A change token for the queue's order: the ids, in order, hashed.
pub fn rev_of(tracks: &[TrackSummary]) -> u64 {
    let mut h = DefaultHasher::new();
    for t in tracks {
        t.id.hash(&mut h);
    }
    h.finish() & REV_MASK
}

fn track_view(t: TrackSummary, name_of: &impl Fn(UserId) -> Option<String>) -> TrackView {
    TrackView {
        id: t.id,
        title: t.title.unwrap_or_else(|| "Unknown title".to_owned()),
        url: t.url,
        duration_secs: t.duration.map(|d| d.as_secs()),
        requester: match t.requester {
            Some(Requester::Auto) => Some("(auto)".to_owned()),
            Some(Requester::User(u)) => name_of(u),
            None => None,
        },
    }
}

/// Build the page's view of a queue. `name_of` names a requester, or `None`.
pub fn view_from_state(state: QueueState, name_of: impl Fn(UserId) -> Option<String>) -> QueueView {
    match state {
        QueueState::Idle => QueueView::Idle,
        QueueState::Hidden => QueueView::Hidden,
        QueueState::Playing { tracks, .. } => {
            let rev = rev_of(&tracks);
            let mut tracks = tracks.into_iter().map(|t| track_view(t, &name_of));
            match tracks.next() {
                None => QueueView::Idle,
                Some(now) => QueueView::Playing {
                    now,
                    upcoming: tracks.collect(),
                    rev,
                },
            }
        },
    }
}
```

Run: `cargo test -p crack-web` — Expected: all pass.
Sabotage: hash titles into `rev` too → `rev_follows_the_order...` fails; remove `& REV_MASK` → `rev_fits_in_a_javascript_number` fails (with overwhelming probability over 200 ids); map empty `Playing` to `Playing` with a default now → `idle_hidden_and_an_empty...` fails; `rename_all = "camelCase"` on `MoveResult` → `the_wire_format...` fails. Revert each.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all && cargo clippy -p crack-web -- -D clippy::all -D warnings --allow clippy::needless_return
git add Cargo.toml Cargo.lock crack-web/Cargo.toml crack-web/src/lib.rs crack-web/src/config.rs crack-web/src/view.rs
git commit -m "feat(web): the crack-web crate -- its environment and its wire types

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: crack-web — access: who may view, who may control

**Files:**
- Create: `crack-web/src/access.rs`
- Modify: `crack-web/src/lib.rs` (`pub mod access;`)

**Interfaces:**
- Consumes: serenity `Cache`, `Http`.
- Produces:
  ```rust
  pub enum Membership { Member, NotMember, Unknown }
  pub struct Presence { pub membership: Membership, pub user_channel: Option<ChannelId>, pub bot_channel: Option<ChannelId> }
  pub enum Access { Hidden, Unavailable, View, Control }   // Hidden → 404, Unavailable → 503
  pub fn decide(p: &Presence) -> Access;
  pub enum Lookup { Member, NotMember, Failed }
  pub fn lookup_from_status(result: Result<(), Option<u16>>) -> Lookup;
  pub struct MemberMemo { .. }  // MemberMemo::new(ttl: Duration); get(g, u, now) -> Option<bool>; record(g, u, &Lookup, now)
  pub const MEMBER_TTL: Duration = Duration::from_secs(300);
  pub struct CachedPresence { pub guild_known: bool, pub member: bool, pub user_channel: Option<ChannelId> }
  pub fn cached_presence(cache: &Cache, g: GuildId, u: UserId) -> CachedPresence;
  pub async fn presence(cache: &Cache, http: &Http, memo: &MemberMemo, g: GuildId, u: UserId, bot_channel: Option<ChannelId>) -> Presence;
  ```

- [ ] **Step 1: Write the failing tests**

Create `crack-web/src/access.rs` with the tests:

```rust
//! Who may see a guild's queue, and who may change it.
//!
//! The decision is a pure function over a [`Presence`], as `music::perms`
//! does it: the cache and HTTP lookups that build a `Presence` are thin
//! glue, and every branch of the decision is tested without either.

#[cfg(test)]
mod test {
    use super::*;
    use std::time::{Duration, Instant};

    const A: ChannelId = ChannelId::new(10);
    const B: ChannelId = ChannelId::new(11);
    const G: GuildId = GuildId::new(1);
    const U: UserId = UserId::new(2);

    fn p(membership: Membership, user: Option<ChannelId>, bot: Option<ChannelId>) -> Presence {
        Presence { membership, user_channel: user, bot_channel: bot }
    }

    #[test]
    fn the_decision_table() {
        use Membership::*;
        let cases = [
            (p(NotMember, Some(A), Some(A)), Access::Hidden),
            (p(Unknown, None, Some(A)), Access::Unavailable),
            (p(Member, None, Some(A)), Access::View),
            (p(Member, Some(B), Some(A)), Access::View),
            (p(Member, Some(A), None), Access::View),
            (p(Member, None, None), Access::View),
            (p(Member, Some(A), Some(A)), Access::Control),
        ];
        for (presence, want) in cases {
            assert_eq!(decide(&presence), want, "{presence:?}");
        }
    }

    #[test]
    fn only_a_404_means_not_a_member() {
        assert_eq!(lookup_from_status(Ok(())), Lookup::Member);
        assert_eq!(lookup_from_status(Err(Some(404))), Lookup::NotMember);
        assert_eq!(lookup_from_status(Err(Some(500))), Lookup::Failed);
        assert_eq!(lookup_from_status(Err(Some(429))), Lookup::Failed);
        assert_eq!(lookup_from_status(Err(None)), Lookup::Failed);
    }

    #[test]
    fn the_memo_remembers_answers_not_failures_and_forgets_in_time() {
        let memo = MemberMemo::new(Duration::from_secs(300));
        let t0 = Instant::now();
        memo.record(G, U, &Lookup::Failed, t0);
        assert_eq!(memo.get(G, U, t0), None, "a failure is not remembered");
        memo.record(G, U, &Lookup::NotMember, t0);
        assert_eq!(memo.get(G, U, t0 + Duration::from_secs(299)), Some(false));
        assert_eq!(memo.get(G, U, t0 + Duration::from_secs(301)), None, "expired");
        memo.record(G, U, &Lookup::Member, t0);
        assert_eq!(memo.get(G, U, t0), Some(true));
        assert_eq!(memo.get(G, UserId::new(3), t0), None, "keyed by user");
    }

    #[test]
    fn an_uncached_guild_is_not_known() {
        let cache = serenity::all::Cache::new();
        let c = cached_presence(&cache, G, U);
        assert!(!c.guild_known);
        assert!(!c.member);
        assert_eq!(c.user_channel, None);
    }

    #[tokio::test]
    async fn a_guild_the_bot_is_not_in_is_not_a_membership_question() {
        // No HTTP is made: the token is garbage and no server is reachable,
        // so a lookup would come back `Failed` and read `Unknown`.
        let cache = serenity::all::Cache::new();
        let http = serenity::all::Http::new(crack_types::get_valid_token());
        let memo = MemberMemo::new(MEMBER_TTL);
        let got = presence(&cache, &http, &memo, G, U, Some(A)).await;
        assert_eq!(got.membership, Membership::NotMember);
    }
}
```

Add `pub mod access;` to `lib.rs`. Run: `cargo test -p crack-web access` — Expected: FAIL to compile.

- [ ] **Step 2: Implement**

Above the tests:

```rust
use dashmap::DashMap;
use serenity::all::{Cache, ChannelId, GuildId, Http, UserId};
use std::time::{Duration, Instant};

/// How long an HTTP membership answer is trusted.
pub const MEMBER_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    Member,
    NotMember,
    /// Discord did not answer; we cannot say.
    Unknown,
}

/// Everything the access decision depends on.
#[derive(Debug, Clone, Copy)]
pub struct Presence {
    pub membership: Membership,
    pub user_channel: Option<ChannelId>,
    pub bot_channel: Option<ChannelId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Not a member: answer 404, so a guild id confirms nothing.
    Hidden,
    /// Could not tell: answer 503.
    Unavailable,
    View,
    /// In the bot's voice channel: may reorder.
    Control,
}

/// The whole rule. Viewing needs membership; controlling needs the user's
/// voice channel to be the bot's.
pub fn decide(p: &Presence) -> Access {
    match p.membership {
        Membership::NotMember => Access::Hidden,
        Membership::Unknown => Access::Unavailable,
        Membership::Member => match (p.user_channel, p.bot_channel) {
            (Some(user), Some(bot)) if user == bot => Access::Control,
            _ => Access::View,
        },
    }
}

/// The outcome of asking Discord whether a user is a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Member,
    NotMember,
    /// Anything but success or 404: a 5xx, a 429, a timeout.
    Failed,
}

/// `Ok` is membership; only a 404 is a definite "no".
pub fn lookup_from_status(result: Result<(), Option<u16>>) -> Lookup {
    match result {
        Ok(()) => Lookup::Member,
        Err(Some(404)) => Lookup::NotMember,
        Err(_) => Lookup::Failed,
    }
}

/// Remembered HTTP membership answers. Failures are never remembered.
pub struct MemberMemo {
    ttl: Duration,
    entries: DashMap<(GuildId, UserId), (bool, Instant)>,
}

impl MemberMemo {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, entries: DashMap::new() }
    }

    pub fn get(&self, g: GuildId, u: UserId, now: Instant) -> Option<bool> {
        let entry = self.entries.get(&(g, u))?;
        let (member, at) = *entry;
        (now.saturating_duration_since(at) < self.ttl).then_some(member)
    }

    pub fn record(&self, g: GuildId, u: UserId, lookup: &Lookup, now: Instant) {
        match lookup {
            Lookup::Member => {
                self.entries.insert((g, u), (true, now));
            },
            Lookup::NotMember => {
                self.entries.insert((g, u), (false, now));
            },
            Lookup::Failed => {},
        }
    }
}

/// What the cache alone can say.
#[derive(Debug, Clone, Copy)]
pub struct CachedPresence {
    /// The bot is in this guild (it is cached).
    pub guild_known: bool,
    /// In the cached member list, or has a voice state here. The member list
    /// of a large guild is partial -- the bot never requests member chunks --
    /// so `false` here is not "not a member".
    pub member: bool,
    pub user_channel: Option<ChannelId>,
}

pub fn cached_presence(cache: &Cache, g: GuildId, u: UserId) -> CachedPresence {
    let Some(guild) = cache.guild(g) else {
        return CachedPresence { guild_known: false, member: false, user_channel: None };
    };
    let voice = guild.voice_states.get(&u);
    CachedPresence {
        guild_known: true,
        member: guild.members.get(&u).is_some() || voice.is_some(),
        user_channel: voice.and_then(|v| v.channel_id),
    }
}

/// Build a [`Presence`]: the cache, then the memo, then one HTTP lookup.
pub async fn presence(
    cache: &Cache,
    http: &Http,
    memo: &MemberMemo,
    g: GuildId,
    u: UserId,
    bot_channel: Option<ChannelId>,
) -> Presence {
    let cached = cached_presence(cache, g, u);
    let membership = if !cached.guild_known {
        Membership::NotMember
    } else if cached.member {
        Membership::Member
    } else if let Some(known) = memo.get(g, u, Instant::now()) {
        if known { Membership::Member } else { Membership::NotMember }
    } else {
        let status = match http.get_member(g, u).await {
            Ok(_) => Ok(()),
            Err(serenity::Error::Http(e)) => Err(e.status_code().map(|s| s.as_u16())),
            Err(_) => Err(None),
        };
        let lookup = lookup_from_status(status);
        memo.record(g, u, &lookup, Instant::now());
        match lookup {
            Lookup::Member => Membership::Member,
            Lookup::NotMember => Membership::NotMember,
            Lookup::Failed => Membership::Unknown,
        }
    };
    Presence { membership, user_channel: cached.user_channel, bot_channel }
}
```

If `guild.voice_states`/`guild.members` are `ExtractMap`s whose `get` takes the key by value, pass `u` instead of `&u`; if `VoiceState::channel_id` is not an `Option<ChannelId>` on serenity `next`, adapt (check `src/model/voice.rs`).

Run: `cargo test -p crack-web access` — Expected: 5 passed.

- [ ] **Step 3: Sabotage**

| Mutation | Must fail |
|---|---|
| `decide`: `Unknown => Access::View` | `the_decision_table` |
| `decide`: Control for any member in voice (drop the `user == bot` guard) | `the_decision_table` |
| `lookup_from_status`: any `Err(Some(_))` → `NotMember` | `only_a_404_means_not_a_member` |
| `MemberMemo::record`: store `Failed` as `false` | `the_memo_remembers...` |
| `MemberMemo::get`: ignore the TTL | `the_memo_remembers...` |
| `presence`: treat an uncached guild as "ask HTTP" | `a_guild_the_bot_is_not_in...` (reads `Unknown`) |

Not caught (report): the voice-state-implies-member branch of `cached_presence`, which needs a populated cache.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p crack-web -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/access.rs crack-web/src/lib.rs
git commit -m "feat(web): access -- member to view, same voice channel to control

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: crack-web — the watch hub

**Files:**
- Create: `crack-web/src/watch.rs`
- Modify: `crack-web/src/lib.rs` (`pub mod watch;`)

**Interfaces:**
- Consumes: `QueueView` (Task 2).
- Produces:
  ```rust
  pub trait ViewSource: Send + Sync + 'static {
      fn view(&self, guild_id: GuildId) -> impl Future<Output = QueueView> + Send;
  }
  pub struct Hub<S: ViewSource> { .. }
  impl<S: ViewSource> Hub<S> {
      pub fn new(source: Arc<S>, tick: Duration, linger: Duration) -> Arc<Self>;
      pub async fn subscribe(self: &Arc<Self>, guild_id: GuildId) -> watch::Receiver<Arc<QueueView>>;
      pub async fn publish(&self, guild_id: GuildId, view: QueueView);
  }
  pub const TICK: Duration = Duration::from_secs(1);
  pub const LINGER: Duration = Duration::from_secs(30);
  ```

- [ ] **Step 1: Write the failing tests**

Create `crack-web/src/watch.rs` with the tests:

```rust
//! One `watch` channel per guild, alive only while someone is watching.
//! Its task asks the source for the view every tick and publishes only when
//! it changed. Polling, deliberately: the queue changes from a dozen places,
//! and "remember to notify" at each is the bug class #434 removed.

#[cfg(test)]
mod test {
    use super::*;
    use crate::view::{QueueView, TrackView};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use uuid::Uuid;

    const G: GuildId = GuildId::new(1);

    struct Fake {
        view: Mutex<QueueView>,
        calls: AtomicUsize,
    }

    impl ViewSource for Fake {
        async fn view(&self, _g: GuildId) -> QueueView {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.view.lock().unwrap().clone()
        }
    }

    fn playing(n: u128) -> QueueView {
        let t = |id| TrackView {
            id: Uuid::from_u128(id),
            title: "t".into(),
            url: None,
            duration_secs: None,
            requester: None,
        };
        QueueView::Playing { now: t(n), upcoming: vec![], rev: n as u64 }
    }

    fn hub(view: QueueView) -> (Arc<Fake>, Arc<Hub<Fake>>) {
        let fake = Arc::new(Fake { view: Mutex::new(view), calls: AtomicUsize::new(0) });
        let hub = Hub::new(fake.clone(), Duration::from_secs(1), Duration::from_secs(30));
        (fake, hub)
    }

    async fn ticks(n: u32) {
        for _ in 0..n {
            tokio::time::advance(Duration::from_secs(1)).await;
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_subscriber_starts_with_the_current_view() {
        let (_fake, hub) = hub(playing(1));
        let rx = hub.subscribe(G).await;
        assert_eq!(**rx.borrow(), playing(1));
    }

    #[tokio::test(start_paused = true)]
    async fn a_change_is_published_and_no_change_is_not() {
        let (fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        ticks(3).await;
        assert!(!rx.has_changed().unwrap(), "same view, no event");
        *fake.view.lock().unwrap() = playing(2);
        ticks(2).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(**rx.borrow_and_update(), playing(2));
    }

    #[tokio::test(start_paused = true)]
    async fn two_watchers_share_one_poller() {
        let (fake, hub) = hub(playing(1));
        let _a = hub.subscribe(G).await;
        let _b = hub.subscribe(G).await;
        let before = fake.calls.load(Ordering::SeqCst);
        ticks(5).await;
        let polled = fake.calls.load(Ordering::SeqCst) - before;
        assert!((4..=6).contains(&polled), "one poll per tick, got {polled}");
    }

    #[tokio::test(start_paused = true)]
    async fn polling_stops_after_the_last_watcher_lingers_out_and_restarts() {
        let (fake, hub) = hub(playing(1));
        let rx = hub.subscribe(G).await;
        drop(rx);
        ticks(35).await;
        let stopped_at = fake.calls.load(Ordering::SeqCst);
        ticks(10).await;
        assert_eq!(fake.calls.load(Ordering::SeqCst), stopped_at, "no polling with nobody watching");
        let rx = hub.subscribe(G).await;
        assert_eq!(**rx.borrow(), playing(1));
        ticks(3).await;
        assert!(fake.calls.load(Ordering::SeqCst) > stopped_at + 1, "polling again");
    }

    #[tokio::test(start_paused = true)]
    async fn publish_reaches_watchers_at_once() {
        let (_fake, hub) = hub(playing(1));
        let mut rx = hub.subscribe(G).await;
        rx.borrow_and_update();
        hub.publish(G, playing(3)).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(**rx.borrow(), playing(3));
    }
}
```

Add `pub mod watch;` to `lib.rs`. Run: `cargo test -p crack-web watch` — Expected: FAIL to compile.

- [ ] **Step 2: Implement**

Above the tests:

```rust
use crate::view::QueueView;
use serenity::all::GuildId;
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex};

/// How often a watched guild's queue is read.
pub const TICK: Duration = Duration::from_secs(1);
/// How long a guild's poller outlives its last watcher (a reload is cheap).
pub const LINGER: Duration = Duration::from_secs(30);

/// Where views come from.
pub trait ViewSource: Send + Sync + 'static {
    fn view(&self, guild_id: GuildId) -> impl Future<Output = QueueView> + Send;
}

pub struct Hub<S: ViewSource> {
    source: Arc<S>,
    guilds: Mutex<HashMap<GuildId, watch::Sender<Arc<QueueView>>>>,
    tick: Duration,
    linger: Duration,
}

impl<S: ViewSource> Hub<S> {
    pub fn new(source: Arc<S>, tick: Duration, linger: Duration) -> Arc<Self> {
        Arc::new(Self { source, guilds: Mutex::new(HashMap::new()), tick, linger })
    }

    /// Watch a guild. The first watcher starts its poller.
    pub async fn subscribe(self: &Arc<Self>, guild_id: GuildId) -> watch::Receiver<Arc<QueueView>> {
        if let Some(tx) = self.guilds.lock().await.get(&guild_id) {
            return tx.subscribe();
        }
        // Read outside the map lock: a slow read must not stall other guilds.
        let initial = Arc::new(self.source.view(guild_id).await);
        let mut guilds = self.guilds.lock().await;
        if let Some(tx) = guilds.get(&guild_id) {
            return tx.subscribe(); // another watcher raced us here
        }
        let (tx, rx) = watch::channel(initial);
        guilds.insert(guild_id, tx.clone());
        drop(guilds);
        tokio::spawn(self.clone().run(guild_id, tx));
        rx
    }

    /// Push a view now -- after a move, so every open tab follows at once.
    pub async fn publish(&self, guild_id: GuildId, view: QueueView) {
        if let Some(tx) = self.guilds.lock().await.get(&guild_id) {
            tx.send_if_modified(|cur| replace_if_changed(cur, view));
        }
    }

    async fn run(self: Arc<Self>, guild_id: GuildId, tx: watch::Sender<Arc<QueueView>>) {
        let mut interval = tokio::time::interval(self.tick);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await; // the first tick is immediate; the view is fresh
        let mut idle_since: Option<tokio::time::Instant> = None;
        loop {
            interval.tick().await;
            if tx.receiver_count() == 0 {
                let since = *idle_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() < self.linger {
                    continue;
                }
                // Re-checked under the map lock, which `subscribe` also holds
                // while it calls `tx.subscribe()`: no watcher can slip in
                // between this check and the removal.
                let mut guilds = self.guilds.lock().await;
                if tx.receiver_count() == 0 {
                    guilds.remove(&guild_id);
                    return;
                }
            }
            idle_since = None;
            let view = self.source.view(guild_id).await;
            tx.send_if_modified(|cur| replace_if_changed(cur, view));
        }
    }
}

fn replace_if_changed(cur: &mut Arc<QueueView>, view: QueueView) -> bool {
    if **cur == view {
        return false;
    }
    *cur = Arc::new(view);
    true
}
```

Run: `cargo test -p crack-web watch` — Expected: 5 passed. If the paused-clock tests are flaky under `advance`, replace `ticks` with `tokio::time::sleep(Duration::from_secs(n)).await` (auto-advance under `start_paused`) and re-run 10 times: `for i in $(seq 10); do cargo test -p crack-web watch -q || break; done`.

- [ ] **Step 3: Sabotage**

| Mutation | Must fail |
|---|---|
| `send_if_modified` → `send_replace` in `run` (publish every tick) | `a_change_is_published_and_no_change_is_not` |
| `subscribe`: always spawn a new poller | `two_watchers_share_one_poller` |
| `run`: never exit (drop the `return`) | `polling_stops_after...` |
| `run`: exit without removing the map entry | `polling_stops_after...` (restart never polls) |
| `publish`: no-op | `publish_reaches_watchers_at_once` |

- [ ] **Step 4: Commit**

```bash
cargo fmt --all && cargo clippy -p crack-web -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/watch.rs crack-web/src/lib.rs
git commit -m "feat(web): a per-guild watch hub that polls only while someone watches

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: crack-web — pages, assets, headers, and the Backend seam

**Files:**
- Create: `crack-web/src/backend.rs`
- Create: `crack-web/src/page.rs`
- Create: `crack-web/src/routes.rs`
- Create: `crack-web/src/test_support.rs` (`#[cfg(test)]`)
- Create: `crack-web/assets/app.css`, `crack-web/assets/app.js` (placeholder bodies here; Task 7 writes them), `crack-web/assets/sortable.min.js`
- Modify: `crack-web/src/lib.rs`

**Interfaces:**
- Consumes: `access::{Presence, Access, decide}`, `view::*`, `watch::{Hub, ViewSource}`, catacombs `AppState`, `AuthenticatedUser`, `routes::auth_router`.
- Produces:
  ```rust
  // backend.rs
  pub struct GuildEntry { pub id: GuildId, pub name: String, pub channel: Option<String> }
  pub trait Backend: ViewSource {
      fn presence(&self, g: GuildId, u: UserId) -> impl Future<Output = Presence> + Send;
      fn move_track(&self, g: GuildId, id: Uuid, to: usize) -> impl Future<Output = Result<usize, MoveRefused>> + Send;
      fn guilds_for(&self, u: UserId) -> impl Future<Output = Vec<GuildEntry>> + Send;
      fn guild_name(&self, g: GuildId) -> Option<String>;
  }
  // routes.rs
  pub struct WebState<B: Backend> { pub auth: Arc<catacombs::AppState>, pub backend: Arc<B>, pub hub: Arc<Hub<B>>, pub origin: Arc<str> }
  pub fn router<B: Backend>(state: WebState<B>) -> axum::Router;
  pub const CSP: &str;
  // page.rs
  pub fn esc(s: &str) -> String;
  pub fn inline_json<T: Serialize>(v: &T) -> String;
  pub fn picker_page(username: &str, guilds: &[GuildEntry]) -> String;
  pub fn queue_page(guild_name: &str, guild_id: GuildId, state: &PageState) -> String;
  pub fn message_page(title: &str, text: &str) -> String;
  // test_support.rs (tests only)
  pub struct FakeBackend { .. } ; pub fn app(fake: Arc<FakeBackend>) -> Router; pub fn session(user: u64) -> String; pub const ORIGIN: &str;
  ```

- [ ] **Step 1: Vendor SortableJS and stub the other assets**

```bash
mkdir -p crack-web/assets
curl -fsSL https://cdn.jsdelivr.net/npm/sortablejs@1.15.6/Sortable.min.js -o crack-web/assets/sortable.min.js
head -c 200 crack-web/assets/sortable.min.js   # expect the "/*! Sortable 1.15.6 ... MIT" banner
sha256sum crack-web/assets/sortable.min.js     # record in the commit message
printf '/* written in Task 7 */\n' > crack-web/assets/app.js
printf '/* written in Task 7 */\n' > crack-web/assets/app.css
```

If 1.15.6 is not the latest 1.15.x on jsdelivr (`curl -s https://data.jsdelivr.com/v1/package/npm/sortablejs | head`), use the latest 1.15.x and record which.

- [ ] **Step 2: Write `page.rs` with its tests first**

Create `crack-web/src/page.rs`:

```rust
//! The dashboard's HTML. Two small pages, written by hand: every inserted
//! string goes through [`esc`], and the queue itself is not HTML at all --
//! it is inlined as JSON and drawn by `app.js`, the one renderer.

use crate::{backend::GuildEntry, view::PageState};
use serde::Serialize;
use serenity::all::GuildId;

/// Escape text for HTML element content and double-quoted attributes.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// JSON safe to place inside `<script type="application/json">`. Every `<`
/// becomes `<`, so no string -- a track title of `</script>` included --
/// can end the element. JSON.parse reads it back unchanged.
pub fn inline_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v)
        .expect("the view types serialize infallibly")
        .replace('<', "\\u003c")
}

fn layout(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{title}</title><link rel=\"stylesheet\" href=\"/assets/app.css\"></head>\
<body><header><a class=\"brand\" href=\"/\">Crack Tunes</a>\
<button type=\"button\" id=\"logout\">Log out</button></header>\
<main>{body}</main><script src=\"/assets/sortable.min.js\"></script>\
<script src=\"/assets/app.js\"></script></body></html>",
        title = esc(title),
    )
}

/// `GET /`: the guilds this user can open.
pub fn picker_page(username: &str, guilds: &[GuildEntry]) -> String {
    let items: String = if guilds.is_empty() {
        "<p class=\"empty\">The bot is not playing in any server you are in.</p>".to_owned()
    } else {
        let li: String = guilds
            .iter()
            .map(|g| {
                format!(
                    "<li><a href=\"/g/{id}\">{name}</a>{chan}</li>",
                    id = g.id,
                    name = esc(&g.name),
                    chan = g
                        .channel
                        .as_deref()
                        .map(|c| format!(" <span class=\"chan\">🔊 {}</span>", esc(c)))
                        .unwrap_or_default(),
                )
            })
            .collect();
        format!("<ul class=\"guilds\">{li}</ul>")
    };
    layout(
        "Crack Tunes",
        &format!("<h1>Hi, {}</h1>{items}", esc(username)),
    )
}

/// `GET /g/{id}`: the queue page. The state is inlined; `app.js` draws it.
pub fn queue_page(guild_name: &str, guild_id: GuildId, state: &PageState) -> String {
    layout(
        guild_name,
        &format!(
            "<section id=\"dash\" data-guild=\"{guild_id}\">\
<h1>{name}</h1><p id=\"badge\" hidden>Reconnecting…</p><p id=\"note\" hidden></p>\
<div id=\"now\"></div><h2>Up next</h2><ol id=\"upcoming\"></ol></section>\
<script type=\"application/json\" id=\"initial\">{json}</script>",
            name = esc(guild_name),
            json = inline_json(state),
        ),
    )
}

/// A plain page for 404/503 and similar.
pub fn message_page(title: &str, text: &str) -> String {
    layout(title, &format!("<h1>{}</h1><p>{}</p>", esc(title), esc(text)))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::view::{QueueView, TrackView};
    use uuid::Uuid;

    #[test]
    fn esc_covers_the_five() {
        assert_eq!(esc(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }

    #[test]
    fn a_hostile_title_cannot_end_the_inlined_json() {
        let hostile = "</script><img src=x onerror=alert(1)>";
        let view = QueueView::Playing {
            now: TrackView {
                id: Uuid::from_u128(1),
                title: hostile.to_owned(),
                url: None,
                duration_secs: None,
                requester: None,
            },
            upcoming: vec![],
            rev: 1,
        };
        let html = queue_page("G", GuildId::new(5), &PageState { view: &view, can_control: false });
        assert_eq!(html.matches("</script>").count(), 3, "only the three real closers: {html}");
        assert!(!html.contains("<img"));
        // And the JSON still parses back to the same title.
        let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
        let end = start + html[start..].find("</script>").unwrap();
        #[derive(serde::Deserialize)]
        struct State {
            view: QueueView,
        }
        let parsed: State = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(parsed.view, view);
    }

    #[test]
    fn guild_and_user_names_are_escaped() {
        let html = picker_page(
            "<b>me</b>",
            &[GuildEntry { id: GuildId::new(5), name: "<i>G</i>".into(), channel: Some("<u>c</u>".into()) }],
        );
        assert!(!html.contains("<b>me") && !html.contains("<i>G") && !html.contains("<u>c"));
        assert!(html.contains("href=\"/g/5\""));
    }
}
```

(The page has exactly three `</script>` closers: the inline JSON, `sortable.min.js`, and `app.js`.)

- [ ] **Step 3: Write `backend.rs`**

```rust
//! The seam between the routes and the bot. Routes are generic over
//! [`Backend`], so their logic is tested against a fake; `LiveBackend`
//! (lib.rs) is the thin glue over crack-core, the cache and Discord.

use crate::{access::Presence, watch::ViewSource};
pub use crack_core::music::remote::MoveRefused;
use serenity::all::{GuildId, UserId};
use std::future::Future;
use uuid::Uuid;

/// A guild on the picker.
#[derive(Debug, Clone)]
pub struct GuildEntry {
    pub id: GuildId,
    pub name: String,
    /// The voice channel the bot is in, by name.
    pub channel: Option<String>,
}

pub trait Backend: ViewSource {
    fn presence(&self, g: GuildId, u: UserId) -> impl Future<Output = Presence> + Send;
    fn move_track(
        &self,
        g: GuildId,
        id: Uuid,
        to: usize,
    ) -> impl Future<Output = Result<usize, MoveRefused>> + Send;
    fn guilds_for(&self, u: UserId) -> impl Future<Output = Vec<GuildEntry>> + Send;
    fn guild_name(&self, g: GuildId) -> Option<String>;
}
```

- [ ] **Step 4: Write the test support**

Create `crack-web/src/test_support.rs`:

```rust
//! A fake backend that records what the routes asked of it, and an app
//! wired to it.

use crate::{
    access::{Membership, Presence},
    backend::{Backend, GuildEntry, MoveRefused},
    routes::{router, WebState},
    view::QueueView,
    watch::{Hub, ViewSource},
};
use axum::Router;
use serenity::all::{ChannelId, GuildId, UserId};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const ORIGIN: &str = "https://dash.test";
pub const JWT_SECRET: &str = "test-secret-test-secret-test-secret!";
pub const BOT_CHANNEL: ChannelId = ChannelId::new(77);

pub struct FakeBackend {
    pub membership: Mutex<Membership>,
    pub user_channel: Mutex<Option<ChannelId>>,
    pub view: Mutex<QueueView>,
    pub move_result: Mutex<Result<usize, MoveRefused>>,
    /// Every move the routes asked for: (guild, track, to).
    pub moves: Mutex<Vec<(GuildId, Uuid, usize)>>,
    pub guilds: Vec<GuildEntry>,
}

impl FakeBackend {
    pub fn new(membership: Membership, user_channel: Option<ChannelId>, view: QueueView) -> Arc<Self> {
        Arc::new(Self {
            membership: Mutex::new(membership),
            user_channel: Mutex::new(user_channel),
            view: Mutex::new(view),
            move_result: Mutex::new(Ok(0)),
            moves: Mutex::new(Vec::new()),
            guilds: vec![GuildEntry { id: GuildId::new(5), name: "Five".into(), channel: Some("Music".into()) }],
        })
    }

    pub fn move_count(&self) -> usize {
        self.moves.lock().unwrap().len()
    }
}

impl ViewSource for FakeBackend {
    async fn view(&self, _g: GuildId) -> QueueView {
        self.view.lock().unwrap().clone()
    }
}

impl Backend for FakeBackend {
    async fn presence(&self, _g: GuildId, _u: UserId) -> Presence {
        Presence {
            membership: *self.membership.lock().unwrap(),
            user_channel: *self.user_channel.lock().unwrap(),
            bot_channel: Some(BOT_CHANNEL),
        }
    }

    async fn move_track(&self, g: GuildId, id: Uuid, to: usize) -> Result<usize, MoveRefused> {
        self.moves.lock().unwrap().push((g, id, to));
        *self.move_result.lock().unwrap()
    }

    async fn guilds_for(&self, _u: UserId) -> Vec<GuildEntry> {
        self.guilds.clone()
    }

    fn guild_name(&self, _g: GuildId) -> Option<String> {
        Some("Five".into())
    }
}

pub fn state(fake: Arc<FakeBackend>) -> WebState<FakeBackend> {
    let mut config = crate::config::WebEnv {
        client_id: "1".into(),
        client_secret: "s".into(),
        public_origin: ORIGIN.into(),
        jwt_secret: JWT_SECRET.into(),
        bot_token: "t".into(),
        bind: "127.0.0.1:0".into(),
    }
    .catacombs_config();
    config.security.jwt_secret = JWT_SECRET.into();
    WebState {
        auth: Arc::new(catacombs::AppState::new(config, catacombs::MemoryStorage::new())),
        hub: Hub::new(fake.clone(), crate::watch::TICK, crate::watch::LINGER),
        backend: fake,
        origin: ORIGIN.into(),
    }
}

pub fn app(fake: Arc<FakeBackend>) -> Router {
    router(state(fake))
}

/// A `Cookie` header value logging in as `user`.
pub fn session(user: u64) -> String {
    let jwt = catacombs::auth::generate_token(user as i64, "tester", JWT_SECRET).unwrap();
    format!("catacombs_session={jwt}")
}

pub async fn body(resp: axum::response::Response) -> String {
    use http_body_util::BodyExt;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
```

- [ ] **Step 5: Write the failing GET-route tests**

Create `crack-web/src/routes.rs` with only the test module (the implementation comes next):

```rust
#[cfg(test)]
mod test {
    use crate::{access::Membership, test_support::*, view::QueueView};
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn get(fake: std::sync::Arc<FakeBackend>, uri: &str, cookie: Option<&str>) -> axum::response::Response {
        let mut req = Request::get(uri);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        app(fake).oneshot(req.body(Body::empty()).unwrap()).await.unwrap()
    }

    fn member_viewing() -> std::sync::Arc<FakeBackend> {
        FakeBackend::new(Membership::Member, None, QueueView::Idle)
    }

    #[tokio::test]
    async fn signed_out_visitors_are_sent_to_login_and_back() {
        let r = get(member_viewing(), "/", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(r.headers()[header::LOCATION], "/auth/login?return_to=%2F");
        let r = get(member_viewing(), "/g/5", None).await;
        assert_eq!(r.headers()[header::LOCATION], "/auth/login?return_to=%2Fg%2F5");
    }

    #[tokio::test]
    async fn the_picker_lists_the_backends_guilds() {
        let r = get(member_viewing(), "/", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        assert!(html.contains("href=\"/g/5\"") && html.contains("Five"));
    }

    #[tokio::test]
    async fn malformed_guild_ids_are_404_not_a_panic() {
        for uri in ["/g/0", "/g/abc", "/g/99999999999999999999", "/g/-1"] {
            let r = get(member_viewing(), uri, Some(&session(9))).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn a_non_member_gets_404_and_an_unknown_gets_503() {
        let r = get(FakeBackend::new(Membership::NotMember, None, QueueView::Idle), "/g/5", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = get(FakeBackend::new(Membership::Unknown, None, QueueView::Idle), "/g/5", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn the_page_inlines_the_view_and_this_viewers_permission() {
        #[derive(serde::Deserialize)]
        struct State {
            view: QueueView,
            can_control: bool,
        }
        async fn state_of(fake: std::sync::Arc<FakeBackend>) -> State {
            let html = body(get(fake, "/g/5", Some(&session(9))).await).await;
            let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
            let end = start + html[start..].find("</script>").unwrap();
            serde_json::from_str(&html[start..end]).unwrap()
        }
        let viewer = state_of(member_viewing()).await;
        assert_eq!(viewer.view, QueueView::Idle);
        assert!(!viewer.can_control);
        let controller = state_of(FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), QueueView::Idle)).await;
        assert!(controller.can_control);
    }

    #[tokio::test]
    async fn every_response_carries_the_security_headers() {
        let r = get(member_viewing(), "/g/5", Some(&session(9))).await;
        let csp = r.headers()["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
        assert_eq!(r.headers()["x-content-type-options"], "nosniff");
    }

    #[tokio::test]
    async fn assets_are_served_by_name_only() {
        let r = get(member_viewing(), "/assets/app.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/javascript"));
        let r = get(member_viewing(), "/assets/sortable.min.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        for uri in ["/assets/nope.js", "/assets/..%2FCargo.toml"] {
            assert_eq!(get(member_viewing(), uri, None).await.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }
}
```

In `crack-web/src/lib.rs` add:

```rust
pub mod access;
pub mod backend;
pub mod page;
pub mod routes;
pub mod view;
pub mod watch;
#[cfg(test)]
mod test_support;
```

(keeping `config`). Run: `cargo test -p crack-web routes` — Expected: FAIL to compile (`router`, `WebState` missing).

- [ ] **Step 6: Implement the router and GET routes**

Above the tests in `crack-web/src/routes.rs`:

```rust
//! The dashboard's HTTP surface.

use crate::{
    access::{decide, Access},
    backend::Backend,
    page,
    view::PageState,
    watch::Hub,
};
use axum::{
    extract::{FromRef, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use catacombs::AuthenticatedUser;
use serenity::all::{GuildId, UserId};
use std::{num::NonZeroU64, sync::Arc, time::Duration};
use tower_http::timeout::TimeoutLayer;

/// Everything but the stylesheet and two scripts is off; no framing.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

pub struct WebState<B: Backend> {
    pub auth: Arc<catacombs::AppState>,
    pub backend: Arc<B>,
    pub hub: Arc<Hub<B>>,
    /// The only `Origin` a move is accepted from.
    pub origin: Arc<str>,
}

impl<B: Backend> Clone for WebState<B> {
    fn clone(&self) -> Self {
        Self {
            auth: self.auth.clone(),
            backend: self.backend.clone(),
            hub: self.hub.clone(),
            origin: self.origin.clone(),
        }
    }
}

impl<B: Backend> FromRef<WebState<B>> for Arc<catacombs::AppState> {
    fn from_ref(s: &WebState<B>) -> Self {
        s.auth.clone()
    }
}

/// The session's user, or `None` if the session is missing or invalid.
type Session = Result<AuthenticatedUser, StatusCode>;

fn user_id(s: Session) -> Option<(UserId, String)> {
    let u = s.ok()?;
    let id = NonZeroU64::new(u64::try_from(u.user_id).ok()?)?;
    Some((UserId::new(id.get()), u.username))
}

/// `/g/0`, `/g/abc` and overflow are not guilds. `GuildId::new(0)` panics.
pub(crate) fn parse_guild(raw: &str) -> Option<GuildId> {
    raw.parse::<NonZeroU64>().ok().map(|n| GuildId::new(n.get()))
}

fn login_redirect(return_to: &str) -> Response {
    let q = serde_urlencoded::to_string([("return_to", return_to)]).expect("a plain string");
    Redirect::to(&format!("/auth/login?{q}")).into_response()
}

pub(crate) fn not_found() -> Response {
    (StatusCode::NOT_FOUND, Html(page::message_page("Not found", "There is nothing here."))).into_response()
}

pub(crate) fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(page::message_page("Try again", "Discord did not answer. Try again in a moment.")),
    )
        .into_response()
}

async fn picker<B: Backend>(State(s): State<WebState<B>>, session: Session) -> Response {
    let Some((user, name)) = user_id(session) else {
        return login_redirect("/");
    };
    let guilds = s.backend.guilds_for(user).await;
    Html(page::picker_page(&name, &guilds)).into_response()
}

async fn guild_page<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return login_redirect(&format!("/g/{g}"));
    };
    let access = decide(&s.backend.presence(g, user).await);
    match access {
        Access::Hidden => not_found(),
        Access::Unavailable => unavailable(),
        Access::View | Access::Control => {
            let view = s.backend.view(g).await;
            let name = s.backend.guild_name(g).unwrap_or_else(|| "Server".to_owned());
            let state = PageState { view: &view, can_control: access == Access::Control };
            Html(page::queue_page(&name, g, &state)).into_response()
        },
    }
}

async fn asset(Path(name): Path<String>) -> Response {
    let (body, ctype): (&'static str, &'static str) = match name.as_str() {
        "app.js" => (include_str!("../assets/app.js"), "text/javascript; charset=utf-8"),
        "sortable.min.js" => (include_str!("../assets/sortable.min.js"), "text/javascript; charset=utf-8"),
        "app.css" => (include_str!("../assets/app.css"), "text/css; charset=utf-8"),
        _ => return not_found(),
    };
    ([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn security_headers(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    resp
}

/// The whole dashboard: pages, the stream, moves, assets, and catacombs at
/// `/auth`. Everything but the event stream has a timeout.
pub fn router<B: Backend>(state: WebState<B>) -> Router {
    let timed = Router::new()
        .route("/", get(picker::<B>))
        .route("/g/{guild}", get(guild_page::<B>))
        .route("/assets/{name}", get(asset))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            Duration::from_secs(15),
        ));
    Router::new()
        .merge(timed)
        .nest("/auth", catacombs::routes::auth_router().with_state(state.auth.clone()))
        .with_state(state)
        .layer(axum::middleware::map_response(security_headers))
}
```

Run: `cargo test -p crack-web` — Expected: all pass. If `catacombs::AuthenticatedUser` is not re-exported at the crate root, import `catacombs::auth::AuthenticatedUser`.

- [ ] **Step 7: Sabotage**

| Mutation | Must fail |
|---|---|
| `inline_json`: drop the `<` replacement | `a_hostile_title_cannot_end_the_inlined_json` |
| `esc`: skip `"` | `esc_covers_the_five` |
| `parse_guild`: `raw.parse::<u64>().ok().map(GuildId::new)` | `malformed_guild_ids_are_404_not_a_panic` (panics) |
| `guild_page`: `Access::Hidden => unavailable()` | `a_non_member_gets_404...` |
| `guild_page`: `can_control: true` | `the_page_inlines_the_view...` |
| remove the `security_headers` layer | `every_response_carries...` |
| `asset`: serve any name from a directory | `assets_are_served_by_name_only` |

- [ ] **Step 8: Commit**

```bash
cargo fmt --all && cargo clippy -p crack-web --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src crack-web/assets
git commit -m "feat(web): pages, assets and headers behind a Backend seam; vendor SortableJS 1.15.x (sha256 <hash>)

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: crack-web — the move endpoint and the event stream

**Files:**
- Modify: `crack-web/src/routes.rs` (handlers `move_track`, `events`; register them)

**Interfaces:**
- Consumes: Task 5's router, `WebState`, `Session`, `parse_guild`, `not_found`, `unavailable`; `view::{MoveRequest, MoveResult, PageState}`; `watch::Hub::{subscribe, publish}`.
- Produces: `POST /g/{guild}/move` and `GET /g/{guild}/events`; `pub const RECHECK: Duration = Duration::from_secs(5)`, `pub const KEEPALIVE: Duration = Duration::from_secs(15)`.

Status codes: `Moved` 200; `Conflict` 409; `NotPlaying` 409; `GameInProgress` 423; `NotAllowed` 403 (JSON body in each). Unauthenticated 401; non-JSON 415; bad `Origin` 403 (no body); unparseable JSON 400; non-member 404; unknown 503.

- [ ] **Step 1: Write the failing move tests**

Append to `routes.rs`'s test module:

```rust
    use crate::backend::MoveRefused;
    use crate::view::TrackView;
    use serenity::all::GuildId;
    use uuid::Uuid;

    fn playing() -> QueueView {
        let t = |n| TrackView { id: Uuid::from_u128(n), title: format!("t{n}"), url: None, duration_secs: None, requester: None };
        QueueView::Playing { now: t(1), upcoming: vec![t(2), t(3)], rev: 7 }
    }

    fn controller() -> std::sync::Arc<FakeBackend> {
        FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing())
    }

    const MOVE: &str = r#"{"id":"00000000-0000-0000-0000-000000000003","to":0}"#;

    async fn post_move(
        fake: std::sync::Arc<FakeBackend>,
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        body_text: &str,
    ) -> axum::response::Response {
        let mut req = Request::post("/g/5/move");
        if let Some(c) = cookie { req = req.header(header::COOKIE, c); }
        if let Some(c) = ctype { req = req.header(header::CONTENT_TYPE, c); }
        if let Some(o) = origin { req = req.header(header::ORIGIN, o); }
        app(fake).oneshot(req.body(Body::from(body_text.to_owned())).unwrap()).await.unwrap()
    }

    #[derive(serde::Deserialize)]
    struct Answer {
        result: String,
        view: Option<QueueView>,
    }

    #[tokio::test]
    async fn a_controller_moves_and_the_backend_gets_exactly_that_move() {
        let fake = controller();
        let r = post_move(fake.clone(), Some(&session(9)), Some("application/json"), Some(ORIGIN), MOVE).await;
        assert_eq!(r.status(), StatusCode::OK);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "moved");
        assert_eq!(a.view, Some(playing()));
        assert_eq!(*fake.moves.lock().unwrap(), vec![(GuildId::new(5), Uuid::from_u128(3), 0)]);
    }

    #[tokio::test]
    async fn refusals_before_the_backend_move_nothing() {
        let cases: [(Option<String>, Option<&str>, Option<&str>, &str, StatusCode); 6] = [
            (None, Some("application/json"), Some(ORIGIN), MOVE, StatusCode::UNAUTHORIZED),
            (Some(session(9)), Some("text/plain"), Some(ORIGIN), MOVE, StatusCode::UNSUPPORTED_MEDIA_TYPE),
            (Some(session(9)), None, Some(ORIGIN), MOVE, StatusCode::UNSUPPORTED_MEDIA_TYPE),
            (Some(session(9)), Some("application/json"), Some("https://evil.example"), MOVE, StatusCode::FORBIDDEN),
            (Some(session(9)), Some("application/json"), None, MOVE, StatusCode::FORBIDDEN),
            (Some(session(9)), Some("application/json"), Some(ORIGIN), "{not json", StatusCode::BAD_REQUEST),
        ];
        for (cookie, ctype, origin, text, want) in cases {
            let fake = controller();
            let r = post_move(fake.clone(), cookie.as_deref(), ctype, origin, text).await;
            assert_eq!(r.status(), want, "{ctype:?} {origin:?} {text}");
            assert_eq!(fake.move_count(), 0, "nothing moved for {want}");
        }
    }

    #[tokio::test]
    async fn a_viewer_not_in_the_bots_channel_is_not_allowed() {
        for channel in [None, Some(serenity::all::ChannelId::new(1))] {
            let fake = FakeBackend::new(Membership::Member, channel, playing());
            let r = post_move(fake.clone(), Some(&session(9)), Some("application/json"), Some(ORIGIN), MOVE).await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, "not_allowed");
            assert_eq!(fake.move_count(), 0);
        }
    }

    #[tokio::test]
    async fn a_non_member_gets_404_even_to_a_move() {
        let fake = FakeBackend::new(Membership::NotMember, Some(BOT_CHANNEL), playing());
        let r = post_move(fake.clone(), Some(&session(9)), Some("application/json"), Some(ORIGIN), MOVE).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(fake.move_count(), 0);
    }

    #[tokio::test]
    async fn backend_refusals_map_to_their_answers() {
        let cases = [
            (MoveRefused::Absent, StatusCode::CONFLICT, "conflict", true),
            (MoveRefused::NowPlaying, StatusCode::CONFLICT, "conflict", true),
            (MoveRefused::GameInProgress, StatusCode::LOCKED, "game_in_progress", false),
            (MoveRefused::NotPlaying, StatusCode::CONFLICT, "not_playing", false),
        ];
        for (refusal, status, result, has_view) in cases {
            let fake = controller();
            *fake.move_result.lock().unwrap() = Err(refusal);
            let r = post_move(fake, Some(&session(9)), Some("application/json"), Some(ORIGIN), MOVE).await;
            assert_eq!(r.status(), status, "{refusal:?}");
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, result);
            assert_eq!(a.view.is_some(), has_view, "{refusal:?}");
        }
    }

    #[tokio::test]
    async fn a_move_reaches_open_watchers_at_once() {
        let fake = controller();
        let st = state(fake.clone());
        let mut rx = st.hub.subscribe(GuildId::new(5)).await;
        rx.borrow_and_update();
        *fake.view.lock().unwrap() = QueueView::Idle; // what the backend reads after the move
        let req = Request::post("/g/5/move")
            .header(header::COOKIE, session(9))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, ORIGIN)
            .body(Body::from(MOVE))
            .unwrap();
        let r = crate::routes::router(st).oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(rx.has_changed().unwrap(), "published without waiting for a tick");
        assert_eq!(**rx.borrow(), QueueView::Idle);
    }
```

Run: `cargo test -p crack-web routes` — Expected: the new tests FAIL (405/404: no route).

- [ ] **Step 2: Implement the move handler**

In `routes.rs` add imports `axum::{body::Bytes, http::HeaderMap, Json, routing::post}` and `crate::view::{MoveRequest, MoveResult}`, then:

```rust
fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json")))
}

fn answer(status: StatusCode, result: MoveResult) -> Response {
    (status, Json(result)).into_response()
}

async fn move_track<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    // Cookie auth means a cross-site POST would carry the cookie. SameSite=Lax
    // withholds it; these two refuse it independently: no HTML form can send
    // JSON, and a foreign page cannot forge Origin.
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<MoveRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return answer(StatusCode::FORBIDDEN, MoveResult::NotAllowed),
        Access::Control => {},
    }
    match s.backend.move_track(g, req.id, req.to).await {
        Ok(to) => {
            tracing::info!(guild = %g, user = %user, track = %req.id, to, "dashboard move");
            let view = s.backend.view(g).await;
            s.hub.publish(g, view.clone()).await;
            answer(StatusCode::OK, MoveResult::Moved { view })
        },
        Err(MoveRefused::Absent | MoveRefused::NowPlaying) => {
            let view = s.backend.view(g).await;
            answer(StatusCode::CONFLICT, MoveResult::Conflict { view })
        },
        Err(MoveRefused::GameInProgress) => answer(StatusCode::LOCKED, MoveResult::GameInProgress),
        Err(MoveRefused::NotPlaying) => answer(StatusCode::CONFLICT, MoveResult::NotPlaying),
    }
}
```

(import `crate::backend::MoveRefused`). Register it in the `timed` router: `.route("/g/{guild}/move", post(move_track::<B>))`.

Run: `cargo test -p crack-web routes` — Expected: all pass.

- [ ] **Step 3: Write the failing event-stream tests**

Append to the test module:

```rust
    async fn first_event(resp: axum::response::Response) -> String {
        use http_body_util::BodyExt;
        let mut body = resp.into_body();
        let mut text = String::new();
        while !text.contains("\n\n") {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
                .await
                .expect("an event within 2s")
                .expect("stream open")
                .unwrap();
            if let Ok(data) = frame.into_data() {
                text.push_str(std::str::from_utf8(&data).unwrap());
            }
        }
        text.lines().find_map(|l| l.strip_prefix("data: ")).expect("a data line").to_owned()
    }

    #[tokio::test]
    async fn the_stream_needs_a_session_and_membership() {
        let r = get(controller(), "/g/5/events", None).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = get(FakeBackend::new(Membership::NotMember, None, playing()), "/g/5/events", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_stream_opens_with_the_current_view_and_permission() {
        #[derive(serde::Deserialize)]
        struct State { view: QueueView, can_control: bool }
        let r = get(controller(), "/g/5/events", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/event-stream"));
        let s: State = serde_json::from_str(&first_event(r).await).unwrap();
        assert_eq!(s.view, playing());
        assert!(s.can_control);
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_voice_channel_drops_control_within_a_recheck() {
        use http_body_util::BodyExt;
        #[derive(serde::Deserialize)]
        struct State { can_control: bool }
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _first = body.frame().await.unwrap().unwrap();
        *fake.user_channel.lock().unwrap() = None;
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await;
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let s: State = serde_json::from_str(data).unwrap();
        assert!(!s.can_control);
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_guild_closes_the_stream() {
        use http_body_util::BodyExt;
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _first = body.frame().await.unwrap().unwrap();
        *fake.membership.lock().unwrap() = Membership::NotMember;
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await;
        // Keep-alive comments may arrive first; the stream must end.
        loop {
            match body.frame().await {
                None => break,
                Some(Ok(f)) => assert!(!f.into_data().map(|d| d.starts_with(b"data:")).unwrap_or(false), "no more events"),
                Some(Err(e)) => panic!("{e}"),
            }
        }
    }
```

Run: `cargo test -p crack-web routes` — Expected: the four new tests FAIL.

- [ ] **Step 4: Implement the stream**

Add imports `axum::response::sse::{Event, KeepAlive, Sse}`, `std::convert::Infallible`, `tokio_stream::{wrappers::ReceiverStream, StreamExt}`, `crate::view::QueueView`, and:

```rust
/// How often an open stream re-checks who is watching.
pub const RECHECK: Duration = Duration::from_secs(5);
/// SSE comment interval, so proxies do not idle the stream out.
pub const KEEPALIVE: Duration = Duration::from_secs(15);

fn event(view: &QueueView, can_control: bool) -> Event {
    Event::default().data(
        serde_json::to_string(&PageState { view, can_control }).expect("the view serializes"),
    )
}

async fn events<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    // EventSource cannot follow a login redirect: a plain 401.
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let access = decide(&s.backend.presence(g, user).await);
    match access {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View | Access::Control => {},
    }
    let mut rx = s.hub.subscribe(g).await;
    let (tx, out) = tokio::sync::mpsc::channel::<Event>(8);
    let backend = s.backend.clone();
    tokio::spawn(async move {
        let mut can_control = access == Access::Control;
        let mut last = rx.borrow_and_update().clone();
        if tx.send(event(&last, can_control)).await.is_err() {
            return;
        }
        let mut recheck = tokio::time::interval(RECHECK);
        recheck.tick().await;
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    last = rx.borrow_and_update().clone();
                    if tx.send(event(&last, can_control)).await.is_err() {
                        return;
                    }
                },
                _ = recheck.tick() => match decide(&backend.presence(g, user).await) {
                    // Left the guild: close the stream.
                    Access::Hidden => return,
                    // Cannot tell right now: keep what we had.
                    Access::Unavailable => {},
                    now => {
                        let now = now == Access::Control;
                        if now != can_control {
                            can_control = now;
                            if tx.send(event(&last, can_control)).await.is_err() {
                                return;
                            }
                        }
                    },
                },
                () = tx.closed() => return,
            }
        }
    });
    let stream = ReceiverStream::new(out).map(Ok::<_, Infallible>);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEPALIVE))
        .into_response()
}
```

Register it **outside** the timed router (a stream must not time out): in `router`, `Router::new().merge(timed).route("/g/{guild}/events", get(events::<B>))...`.

Run: `cargo test -p crack-web` — Expected: all pass.

- [ ] **Step 5: Sabotage**

| Mutation | Must fail |
|---|---|
| move: drop the `is_json` check | `refusals_before_the_backend_move_nothing` |
| move: accept a missing `Origin` | `refusals_before_the_backend_move_nothing` |
| move: `Access::View` falls through to the move | `a_viewer_not_in_the_bots_channel_is_not_allowed` |
| move: pass `req.to + 1` to the backend | `a_controller_moves_and_the_backend_gets_exactly_that_move` |
| move: skip `hub.publish` | `a_move_reaches_open_watchers_at_once` |
| move: `Conflict` without a view (`NotAllowed`) | `backend_refusals_map_to_their_answers` |
| events: skip the recheck arm | `leaving_the_voice_channel_drops_control...` and `leaving_the_guild_closes_the_stream` |
| events: return 200 for `Hidden` | `the_stream_needs_a_session_and_membership` |
| events: put the route inside the timed router | observe: a stream held > 15s ends (manual in Task 8; report if no test catches it) |

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy -p crack-web --all-targets -- -D clippy::all -D warnings --allow clippy::needless_return
git add crack-web/src/routes.rs
git commit -m "feat(web): the move endpoint and the live event stream

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 7: The page's JS and CSS; LiveBackend; starting it from the bot; v0.15.0

**Files:**
- Modify: `crack-web/assets/app.js`, `crack-web/assets/app.css`
- Modify: `crack-web/src/lib.rs` (`LiveBackend`, `WebDeps`, `spawn_if_configured`, `serve`; an `assets` test)
- Modify: `crack-cli/Cargo.toml` (feature `web`, dependency)
- Modify: `crack-cli/src/main.rs:~78-93`
- Modify: `Cargo.toml` (version `0.15.0`)
- Modify: `docs/` — add `docs/web-dashboard.md` (operator notes)

**Interfaces:**
- Consumes: everything above; `crack_core::music::remote::*`; `Data`.
- Produces: `crack_web::{WebDeps, spawn_if_configured, LiveBackend}`; `pub struct WebDeps { pub data: Arc<Data>, pub cache: Arc<Cache>, pub http: Arc<Http> }`; `pub fn spawn_if_configured(deps: WebDeps) -> Option<tokio::task::JoinHandle<()>>`.

- [ ] **Step 1: Write the asset guard test**

Append to `crack-web/src/lib.rs`:

```rust
#[cfg(test)]
mod asset_test {
    /// Track titles are third-party text: the page must never parse them as
    /// HTML. `app.js` builds every node with createElement + textContent.
    #[test]
    fn app_js_never_parses_strings_as_html() {
        let js = include_str!("../assets/app.js");
        for banned in ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval("] {
            assert!(!js.contains(banned), "app.js uses {banned}");
        }
        assert!(js.contains("textContent"), "app.js renders text");
    }
}
```

Run: `cargo test -p crack-web asset_test` — Expected: FAIL (placeholder has no `textContent`).

- [ ] **Step 2: Write `app.js`**

Replace `crack-web/assets/app.js`:

```js
// The dashboard's one renderer. It draws the view inlined in the page and
// every view the server sends after it. Track titles are third-party text,
// so nothing here parses a string as HTML: every node is built with
// createElement and filled with textContent.
(() => {
  "use strict";

  const logout = document.getElementById("logout");
  if (logout) {
    logout.addEventListener("click", async () => {
      await fetch("/auth/logout", { method: "POST", credentials: "same-origin" });
      window.location.assign("/");
    });
  }

  const root = document.getElementById("dash");
  if (!root) return; // the picker page
  const guild = root.dataset.guild;
  const nowEl = document.getElementById("now");
  const listEl = document.getElementById("upcoming");
  const badge = document.getElementById("badge");
  const note = document.getElementById("note");

  let state = JSON.parse(document.getElementById("initial").textContent);
  let dragging = false;
  let pending = null;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  }

  function duration(secs) {
    if (secs === null || secs === undefined) return "";
    const m = Math.floor(secs / 60);
    const s = String(secs % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  function track(t, handle) {
    const li = el("li", "track");
    li.dataset.id = t.id;
    if (handle) li.appendChild(el("span", "handle", "⠿"));
    const title = t.url ? el("a", "title", t.title) : el("span", "title", t.title);
    if (t.url) {
      title.href = t.url;
      title.rel = "noopener noreferrer";
      title.target = "_blank";
    }
    li.appendChild(title);
    li.appendChild(el("span", "meta", [duration(t.duration_secs), t.requester].filter(Boolean).join(" · ")));
    return li;
  }

  function say(text) {
    note.textContent = text || "";
    note.hidden = !text;
  }

  function render() {
    const v = state.view;
    nowEl.replaceChildren();
    listEl.replaceChildren();
    if (v.state === "idle") {
      nowEl.appendChild(el("p", "empty", "Nothing is playing."));
    } else if (v.state === "hidden") {
      nowEl.appendChild(el("p", "empty", "A Guilty Pleasure game is on — the queue is hidden until it ends."));
    } else {
      const now = track(v.now, false);
      now.classList.add("now");
      nowEl.appendChild(now);
      for (const t of v.upcoming) listEl.appendChild(track(t, state.can_control));
      if (v.upcoming.length === 0) listEl.appendChild(el("li", "empty", "Nothing queued after this."));
    }
    sortable.option("disabled", !(state.can_control && v.state === "playing"));
    root.classList.toggle("can-control", !!state.can_control);
  }

  async function move(id, to) {
    let body = null;
    let status = 0;
    try {
      const res = await fetch(`/g/${guild}/move`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id, to }),
      });
      status = res.status;
      body = await res.json().catch(() => null);
    } catch (_) {
      say("Could not reach the server. Nothing was moved.");
    }
    const result = body && body.result;
    if (result === "moved") say("");
    else if (result === "conflict") say("The queue changed while you were dragging — here it is now.");
    else if (result === "not_allowed") say("Join the bot's voice channel to reorder.");
    else if (result === "game_in_progress") say("A Guilty Pleasure game is on — the queue is locked.");
    else if (result === "not_playing") say("Nothing is playing.");
    else if (status) say(`That did not work (${status}).`);
    if (body && body.view) state = { ...state, view: body.view };
    render(); // never keep an order the server did not accept
  }

  const sortable = Sortable.create(listEl, {
    handle: ".handle",
    animation: 150,
    disabled: true,
    onStart: () => { dragging = true; },
    onEnd: (ev) => {
      dragging = false;
      if (ev.oldIndex === ev.newIndex) {
        if (pending) { state = pending; pending = null; render(); }
        return;
      }
      pending = null;
      move(ev.item.dataset.id, ev.newIndex);
    },
  });

  const stream = new EventSource(`/g/${guild}/events`);
  stream.onmessage = (e) => {
    const next = JSON.parse(e.data);
    badge.hidden = true;
    if (dragging) { pending = next; return; } // do not yank a row from under the cursor
    state = next;
    render();
  };
  stream.onerror = () => {
    badge.textContent = stream.readyState === EventSource.CLOSED
      ? "Disconnected — reload the page."
      : "Reconnecting…";
    badge.hidden = false;
  };

  render();
})();
```

- [ ] **Step 3: Write `app.css`**

Replace `crack-web/assets/app.css`:

```css
:root {
  --bg: #fafafa; --fg: #1b1b1f; --muted: #6b6b76; --card: #ffffff;
  --line: #e4e4ea; --accent: #6d28d9; --note: #fff7d6;
}
@media (prefers-color-scheme: dark) {
  :root { --bg: #111114; --fg: #ececf1; --muted: #9a9aa6; --card: #1b1b21;
          --line: #2c2c35; --accent: #a78bfa; --note: #3a3218; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); color: var(--fg);
       font: 16px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif; }
header { display: flex; justify-content: space-between; align-items: center;
         padding: 12px 16px; border-bottom: 1px solid var(--line); }
.brand { color: var(--fg); font-weight: 700; text-decoration: none; }
header button { background: none; border: 1px solid var(--line); color: var(--muted);
                border-radius: 6px; padding: 4px 10px; cursor: pointer; }
main { max-width: 720px; margin: 0 auto; padding: 16px; }
h1 { font-size: 1.4rem; margin: 8px 0 16px; overflow-wrap: anywhere; }
h2 { font-size: 1rem; color: var(--muted); margin: 24px 0 8px; }
ul.guilds, ol#upcoming { list-style: none; padding: 0; margin: 0; }
ul.guilds li { padding: 10px 0; border-bottom: 1px solid var(--line); }
ul.guilds a { color: var(--accent); font-weight: 600; }
.chan { color: var(--muted); margin-left: 8px; }
.track { display: flex; align-items: baseline; gap: 10px; padding: 10px 12px;
         background: var(--card); border: 1px solid var(--line); border-radius: 8px;
         margin-bottom: 6px; min-width: 0; }
.track.now { border-color: var(--accent); }
.title { flex: 1; min-width: 0; overflow-wrap: anywhere; color: var(--fg); text-decoration: none; }
a.title:hover { text-decoration: underline; }
.meta { color: var(--muted); font-size: 0.85rem; white-space: nowrap; }
.handle { cursor: grab; color: var(--muted); user-select: none; touch-action: none; padding: 0 4px; }
.sortable-ghost { opacity: 0.4; }
.empty { color: var(--muted); }
#badge, #note { background: var(--note); padding: 8px 12px; border-radius: 6px; }
@media (max-width: 480px) { .track { flex-wrap: wrap; } .meta { width: 100%; } }
```

Run: `cargo test -p crack-web` — Expected: all pass, including `app_js_never_parses_strings_as_html`.

- [ ] **Step 4: `LiveBackend`, `WebDeps`, `spawn_if_configured`**

Append to `crack-web/src/lib.rs` (above the test module):

```rust
use crate::{
    access::{Membership, MemberMemo, Presence, MEMBER_TTL},
    backend::{Backend, GuildEntry, MoveRefused},
    config::WebEnv,
    routes::WebState,
    view::{view_from_state, QueueView},
    watch::{Hub, ViewSource, LINGER, TICK},
};
use crack_core::{music::remote, Data};
use serenity::all::{Cache, GuildId, Http, UserId};
use std::sync::Arc;
use uuid::Uuid;

/// What the bot hands the dashboard.
pub struct WebDeps {
    pub data: Arc<Data>,
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
}

/// The real backend: crack-core's `remote`, the cache, and Discord.
pub struct LiveBackend {
    deps: WebDeps,
    memo: MemberMemo,
}

impl LiveBackend {
    fn member_name(&self, g: GuildId, u: UserId) -> Option<String> {
        let guild = self.deps.cache.guild(g)?;
        guild.members.get(&u).map(|m| m.display_name().to_owned())
    }
}

impl ViewSource for LiveBackend {
    async fn view(&self, g: GuildId) -> QueueView {
        let state = remote::queue_state(&self.deps.data, g).await;
        view_from_state(state, |u| self.member_name(g, u))
    }
}

impl Backend for LiveBackend {
    async fn presence(&self, g: GuildId, u: UserId) -> Presence {
        let bot = remote::bot_channel(&self.deps.data, g).await;
        access::presence(&self.deps.cache, &self.deps.http, &self.memo, g, u, bot).await
    }

    async fn move_track(&self, g: GuildId, id: Uuid, to: usize) -> Result<usize, MoveRefused> {
        remote::move_by_id(self.deps.data.clone(), &self.deps.http, g, id, to).await
    }

    async fn guilds_for(&self, u: UserId) -> Vec<GuildEntry> {
        let mut out = Vec::new();
        for (g, channel) in remote::active_guilds(&self.deps.data).await {
            let p = access::presence(&self.deps.cache, &self.deps.http, &self.memo, g, u, Some(channel)).await;
            if p.membership != Membership::Member {
                continue;
            }
            let (name, channel) = match self.deps.cache.guild(g) {
                Some(guild) => (
                    guild.name.to_string(),
                    guild.channels.get(&channel).map(|c| c.base.name.to_string()),
                ),
                None => continue,
            };
            out.push(GuildEntry { id: g, name, channel });
        }
        out
    }

    fn guild_name(&self, g: GuildId) -> Option<String> {
        self.deps.cache.guild(g).map(|guild| guild.name.to_string())
    }
}

/// Start the dashboard if its environment is complete. Missing keys are
/// logged by name and the dashboard stays off; nothing here can stop the bot.
pub fn spawn_if_configured(deps: WebDeps) -> Option<tokio::task::JoinHandle<()>> {
    let env = match WebEnv::from_lookup(|k| std::env::var(k).ok()) {
        Ok(env) => env,
        Err(missing) => {
            tracing::warn!("web dashboard off; missing or unusable: {}", missing.join(", "));
            return None;
        },
    };
    Some(tokio::spawn(async move {
        if let Err(e) = serve(env, deps).await {
            tracing::error!("web dashboard stopped: {e}");
        }
    }))
}

async fn serve(env: WebEnv, deps: WebDeps) -> std::io::Result<()> {
    let auth = Arc::new(catacombs::AppState::new(
        env.catacombs_config(),
        catacombs::MemoryStorage::new(),
    ));
    let backend = Arc::new(LiveBackend { deps, memo: MemberMemo::new(MEMBER_TTL) });
    let state = WebState {
        auth,
        hub: Hub::new(backend.clone(), TICK, LINGER),
        backend,
        origin: env.public_origin.clone().into(),
    };
    let listener = tokio::net::TcpListener::bind(&env.bind).await?;
    tracing::info!("web dashboard on {} for {}", env.bind, env.public_origin);
    axum::serve(listener, routes::router(state)).await
}
```

Adjust the channel-name lookup to serenity `next`'s `GuildChannel` shape if `c.base.name` does not compile (`c.name` on older shapes). Run: `cargo check -p crack-web && cargo test -p crack-web`.

- [ ] **Step 5: Wire it into the bot**

`crack-cli/Cargo.toml`: in `[features]`, `default = ["crack-tracing", "web"]` and add

```toml
# The web dashboard (crack-web). On by default; it stays dormant until its
# environment is set -- see docs/web-dashboard.md.
web = ["dep:crack-web"]
```

and in `[dependencies]`: `crack-web = { path = "../crack-web/", optional = true }`.

In `crack-cli/src/main.rs`, after `let data_arc = client.data::<crack_core::Data>().clone();` add:

```rust
    // The dashboard shares the bot's Data, cache and HTTP client. It starts
    // only if its environment is complete, and its failures are logged, never
    // propagated: the dashboard must not be able to take the bot down.
    #[cfg(feature = "web")]
    let _web = crack_web::spawn_if_configured(crack_web::WebDeps {
        data: data_arc.clone(),
        cache: client.cache.clone(),
        http: client.http.clone(),
    });
```

Run: `cargo build -p cracktunes && cargo build -p cracktunes --no-default-features --features crack-tracing` — Expected: both succeed (the second proves the bot builds without the dashboard).

Smoke test (no Discord connection needed for the dashboard to decline):
```bash
DISCORD_TOKEN=x timeout 5 cargo run -p cracktunes 2>&1 | grep -m1 "web dashboard" || true
```
Expected: a line `web dashboard off; missing or unusable: DISCORD_CLIENT_ID, DISCORD_CLIENT_SECRET, WEB_PUBLIC_ORIGIN, WEB_JWT_SECRET` (the bot then fails on the bad token, which is fine).

- [ ] **Step 6: Operator notes and version**

Root `Cargo.toml`: `version = "0.15.0"`. Create `docs/web-dashboard.md`:

```markdown
# Web dashboard

Arc 1: view a guild's queue live; reorder it from the bot's voice channel.
Design: `docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md`.

## Turning it on

Built in by default (crack-cli feature `web`); dormant until all of these are set:

| Variable | Value |
|---|---|
| `DISCORD_CLIENT_ID` | the application id (falls back to `DISCORD_APP_ID`) |
| `DISCORD_CLIENT_SECRET` | Developer Portal → OAuth2 → Client Secret |
| `WEB_PUBLIC_ORIGIN` | `https://dash.cracktun.es` (or `http://localhost:8090` for a tunnel) |
| `WEB_JWT_SECRET` | 32+ random characters: `openssl rand -hex 32` |
| `WEB_BIND` | optional, default `0.0.0.0:8090` |

Register `<WEB_PUBLIC_ORIGIN>/auth/callback` under the application's OAuth2
redirects. A missing variable logs `web dashboard off; missing or unusable: …`
and the bot runs on.

## What it holds

A user id and username in a signed, HttpOnly cookie (24 h). Nothing is written
to the database. Restarting the bot invalidates nothing: sessions are JWTs
signed with `WEB_JWT_SECRET`; rotate that to sign everyone out.
```

- [ ] **Step 7: Full local gate, then commit**

```bash
cargo fmt --all -- --check
cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return
cargo check -p crack-core --features crack-bf,crack-osint --locked
cargo test --workspace
git add crack-web/assets crack-web/src/lib.rs crack-cli/Cargo.toml crack-cli/src/main.rs Cargo.toml Cargo.lock docs/web-dashboard.md
git commit -m "feat: the web dashboard -- live queue and drag to reorder (v0.15.0)

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

Expected: fmt clean, clippy clean, tests pass (the db-gated ones are compiled but not run without `--features ...db-tests`).

---

### Task 8: Prove it on TuneTitan, then PR

This task needs the owner for two things only they can do; ask for both together, early:
1. **TuneTitan's Discord application:** its Client Secret (into `~/projects/homelab/.env.tunetitan` as `DISCORD_CLIENT_SECRET`, via Vaultwarden per the homelab's secrets flow — never pasted into chat), and add the redirect `http://localhost:8090/auth/callback` under OAuth2 → Redirects.
2. Confirmation to deploy a branch image to TuneTitan.

**Files:** none in cracktunes beyond fixes found here. homelab: `tunetitan/docker-compose.yml` (env passthrough + `127.0.0.1:8090:8090` port), on a homelab branch.

- [ ] **Step 1: Build branch images**

Push the branch and open a **draft** PR (CI runs only on PRs). Dispatch `docker.yml` for the branch so `cracktunes:<branch>` and `cracktunes-migrate:<branch>` are published (see memory `tunetitan-beta-tenant`: override BOTH images).

- [ ] **Step 2: Configure TuneTitan**

On a homelab branch, add to the tunetitan cracktunes service: `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET`, `WEB_PUBLIC_ORIGIN: http://localhost:8090`, `WEB_JWT_SECRET` from `.env.tunetitan`, and `ports: ["127.0.0.1:8090:8090"]` (loopback only: reached through ssh). Generate `WEB_JWT_SECRET` into the env file without printing it. `./homelab.sh up tunetitan </dev/null` with both image overrides, then `./homelab.sh verify tunetitan`.

- [ ] **Step 3: Walk it in a browser**

`ssh -N -L 8090:127.0.0.1:8090 root@192.168.1.116 &`, then with the claude-in-chrome tools (record a GIF, `dashboard_reorder.gif`):
1. `http://localhost:8090/` → Discord login → back on the picker (empty if nothing plays).
2. In Discord: join voice in the test guild, `/play` a playlist of ≥ 4 tracks.
3. Picker lists the guild → open it; drag handles present.
4. Drag track 3 above track 1 of "Up next": order changes; the Discord queue message (if one is open via `/queue`) refreshes; no reply is posted in the channel.
5. Second tab on the same page: it follows the drag within ~1 s.
6. `/skip` in Discord: both tabs update within ~1–2 s.
7. Leave voice: handles disappear within ~5 s; a drag attempt (if started) is answered "Join the bot's voice channel".
8. `/gp start` (if practical in the test guild): the page shows the hidden notice and no titles.
9. Hold a page open > 30 s: the stream stays up (keep-alives), no timeout.
10. Log out → back to the login redirect.

Record anything that fails as a bug, fix it on the branch (systematic-debugging), re-deploy, re-walk.

- [ ] **Step 4: Ready the PR**

Mark the PR ready. Body: design and change (spec path; the catacombs dependency and why; the `remote` facade and why the queue module stays private; the membership fallback; polling vs notifying), the full mutation → caught-by table from Tasks 1–7 including the uncaught ones, the TuneTitan walk with the GIF, and the rollout steps still to come (production homelab PR, `dash.cracktun.es` DNS + tunnel rule + Caddy block, Developer Portal redirect, cracktun.es privacy paragraph). End with the attribution lines from the session's system reminder. Iterate on CI and review; merge; `git switch master && git pull --ff-only`; `git tag -s v0.15.0 -m "v0.15.0: web dashboard, arc 1"`; `git push origin v0.15.0`. The cargo-dist Release workflow creates the GitHub release — **never `gh release create`**.
