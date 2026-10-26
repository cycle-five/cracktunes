# Split step 3: crack-rooms Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the voice rooms (`rooms/`), the mod log (`modlog/`) and their queries (`db/rooms.rs`, `db/modlog.rs`) out of crack-core into a new crate, `crack-rooms`, with no change to what the bot does.

**Architecture:** rooms and the mod log send through crack-core's delivery layer (`Transport`, `Press`, the courier's sends, `Throttle`), so the poise-free part of that layer moves down to crack-common first (Task 1). Then, still inside crack-core, the mod log (Task 2) and the rooms (Task 3) stop taking `Data` and take a narrow `RoomsData` handle instead, as the economy did in step 2. Last, `git mv` moves the files and crack-core keeps `pub use` shims, so every `crate::rooms::…`, `crate::modlog::…` and `crate::db::{rooms,modlog}::…` path keeps working (Task 4).

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`, `crack-test-db`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 3, rules 1–9).

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same messages, buttons, modals and Discord actions, byte for byte;
  - the same slash commands registered, with the same options and choices;
  - the same log lines (text and level);
  - no storage, schema, migration, `.sqlx/` or SQL text change.
- **Function bodies don't change** in Tasks 1–3, apart from the two named in "Calls" (`post`'s channel arm and `ChannelSink::post`). Only signatures, imports, call sites and test set-up change. Any other changed body line needs a stated reason in the report.
- **crack-rooms never depends on poise or crack-core,** not even as a dev-dependency (spec rules 1, 4 and 9). It may depend on crack-common, crack-model, crack-types, crack-economy (carved before it), serenity and the external crates the moved files already use (rule 6). **crack-common never depends on poise** (its existing lint step).
- **No `Data`, `DataInner` or poise `Context` in a moved file** (rule 2). serenity's `Context`, `Http` and `Cache` are allowed.
- **Dependency versions.** Copy each new dependency line exactly from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains the new crate and no second version of any crate.
- **Paths keep working.** crack-core re-exports what moves at its old path. Call sites elsewhere in crack-core, crack-web and crack-cli change only where this plan says so or the compiler points.
- **Strings stay in crack-common's `messages.rs`** (#537). No new `pub const` strings anywhere else.
- **Typed serde only.** Never `serde_json::json!`, and never index a `serde_json::Value` in new code.
- **No local mutants runs** (owner, 2026-10-25). `cargo mutants --list` is allowed: it builds nothing.
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. Write paths in full, never through a shell variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap, from the worktree root `/home/lothrop/projects/cracktunes-split`). `FEATURES` below is literal text to paste, not a shell variable:
  - Tasks 1–3: `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin`
  - Task 4 adds `,crack-rooms/db-tests`.
  - suite: `cargo nextest run --workspace --features FEATURES`
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `cargo clippy -p crack-economy --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Test counts are conserved.** Before your first change, record the count from `cargo nextest list --workspace --features FEATURES --message-format json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["test-count"])'`, and the suite's passed/skipped line. Report both, before and after. A test may move crate; none may disappear, and the skipped count may not rise. A new test raises the count by one; say which.
- **Waiting:** wait on long jobs in the foreground, with a blocking loop under the 10-minute Bash timeout. Never end the turn to wait for a notification.
- **Commits:**
  - never `git add -A`; add files by name, and use `git mv` for moves;
  - run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.

## Calls this plan makes against the spec

- **The delivery primitives move down to crack-common** (spec rule 1: a feature crate depends only on what is below it). Step 1 left `courier`, `transport`, `discord` and `throttle` in crack-core because they named `Context`, `Data` or poise. Measured now, only parts of two files do:
  - `throttle.rs` and `transport.rs` are poise-free whole. `transport.rs` needs `http_utils::is_unknown_message`, which moves with it.
  - `discord.rs`: everything except `ToDiscord::to_reply` and `to_reply_edit` (they build `poise::CreateReply`). Those two become a crack-core trait, `ToPoiseReply`, implemented for `Rendered` with the same bodies.
  - `courier.rs`: the sends that take a `Transport` or a `Press` (`post_message`, `edit_message`, `edit_rendered_message`, `acknowledge`, `answer_privately`, `respond`, `update`). `ReplySink`, the poise replies, `Destination` and `post` stay, because `post` reads `Data` for the status message and echoes.
  - `test_support.rs`: `FakeTransport`/`Op` and `FakePress`/`PressOp`, behind a crack-common feature `test-support` that crates enable as a dev-dependency. `FakeReplies`/`ReplyOp` stay in crack-core.
  - crack-core keeps every old path: `messaging/mod.rs` re-exports `throttle` and `transport`, and the thinner `courier.rs`, `discord.rs` and `test_support.rs` start with `pub use crack_common::<same module>::*;`.
- **One new function, `crack_common::courier::post_to_channel`.** It is `post`'s `Destination::Channel` arm, verbatim: send, log `post to {channel} failed: {err:?}` at warn on failure, return where it landed. `post`'s channel arm calls it, and the mod log's `ChannelSink::post` calls it instead of `post(&data, …, Destination::Channel(…))`. Same sends, same log lines. These are the only two changed bodies in Tasks 1–3.
- **The handle is a narrow copy of `Data`'s three fields, under the same names** (step 2's pattern):
  ```rust
  #[derive(Debug, Clone, Default)]
  pub struct RoomsData {
      pub database_pool: Option<sqlx::PgPool>,
      pub rooms: Arc<registry::RoomRegistry>,
      pub modlog: Arc<crate::modlog::registry::ModLogRegistry>,
  }
  ```
  - It lives in `rooms/mod.rs` and moves with it, so its path is `crate::rooms::RoomsData` before and after.
  - `DataInner.rooms` and `DataInner.modlog` become `Arc<…>`, and `Data::rooms_data(&self) -> RoomsData` clones the three handles. `Default::default()` still builds both (`Arc<T: Default>`).
  - One handle for both features, because rooms and the mod log call each other both ways (spec) and both read all three fields.
  - Why: same field names, so a converted body is unchanged; the `Arc`s keep the registries that spawned tasks write (batch timers, seed, sweeps, rechecks) the same ones `Data` and the commands read.
- **crack-rooms depends on crack-economy** for `ActionKind` and `SHOP_AUDIT_REASON_PREFIX` (spec step 3: "Builder's call"). It is downward, rule 1 allows it, and the alternative changes `ModLogEvent`'s fields in a move PR. Cost: a change in crack-economy rebuilds crack-rooms (seconds).
- **`economy/facts.rs` stays in crack-core** (the spec says it "may move"; nothing requires it, and it reads `Data`).
- **One PR,** as in steps 1 and 2. **v0.41.0**, a minor bump, done by the controller after Task 4.

## Review Focus

1. **State a spawned task writes is the state the commands read.** The mod-log batch timer and seed, the room sweeps and rechecks hold a `RoomsData` clone. If the handle copied a registry, their writes would go nowhere. Pinned in Task 2 by `rooms_data_carries_the_pool_and_shares_the_registries` (`Arc::ptr_eq` on both).
2. **A bot with no database.** Every converted function must see the same `database_pool` its `Data` has; a handle built with `None` silently stops rooms persisting and the mod log loading. Pinned by the same test.
3. **The mod log still posts, and says so when it can't.** `ChannelSink` now goes through `post_to_channel`. Pinned in Task 1 by `post_to_channel_says_where_it_landed` and `post_to_channel_swallows_a_failed_send`, and by modlog/live's existing sink tests.
4. **Discord payloads are unchanged by the `ToDiscord` split.** Pinned by `discord.rs`'s conversion tests (they move with the code) and crack-core's byte pins in `messaging/pins.rs` (they stay).
5. **Room and mod-log db tests still run in CI, not compile-and-skip,** and the offline Docker build compiles crack-rooms' `query!` macros. Checked by the conserved test and skip counts, and in Task 4 with `SQLX_OFFLINE=true`.

---

### Task 1: The poise-free delivery layer moves down to crack-common

**Files:**
- Move (`git mv`): `crack-core/src/messaging/{throttle,transport}.rs` → `crack-common/src/`
- Move then trim: `crack-core/src/messaging/discord.rs` → `crack-common/src/discord.rs`; a new `crack-core/src/messaging/discord.rs` keeps `ToPoiseReply`
- Create: `crack-common/src/courier.rs` and `crack-common/src/test_support.rs`, from the parts of crack-core's `courier.rs` and `test_support.rs` named in "Calls"
- Modify: `crack-common/{Cargo.toml,src/lib.rs}`, `crack-core/Cargo.toml` (dev-dependency), `crack-core/src/messaging/{mod,courier,test_support}.rs`, `crack-core/src/http_utils.rs`, `.cargo/mutants.toml`, and the files that call `.to_reply(`/`.to_reply_edit(` (`commands/music/doplay.rs`, `messaging/{courier,pins}.rs`)

**Interfaces:**
- Produces in crack-common:
  - `crack_common::{throttle, transport, discord, courier}` with the items listed in "Calls". `transport::is_unknown_message` is `pub`, and `http_utils` re-exports it.
  - `crack_common::courier::post_to_channel(transport: &dyn Transport, channel: GenericChannelId, out: Rendered) -> Option<(GenericChannelId, MessageId)>`.
  - `#[cfg(any(test, feature = "test-support"))] pub mod test_support;`, with feature `test-support = []`.
- Produces in crack-core: `messaging::discord::ToPoiseReply { fn to_reply(&self, ephemeral: bool) -> poise::CreateReply<'static>; fn to_reply_edit<'a>(&self) -> (poise::CreateReply<'a>, bool); }`, implemented for `Rendered`.
- Inside crack-common, the moved files' `crate::messaging::X` imports become `crate::X`, `crate::errors` stays `crate::errors`, and `async_trait::async_trait` becomes `serenity::async_trait` (no new dependency). The only new dependency is `dashmap = "6"` (for `Throttle`).
- A test in `discord.rs` that calls `to_reply` or `to_reply_edit` stays in crack-core; the rest move.

- [ ] **Step 1: Write the failing tests** in the new `crack-common/src/courier.rs`:
  ```rust
  #[tokio::test]
  async fn post_to_channel_says_where_it_landed() {
      let t = FakeTransport::default();
      let ch = GenericChannelId::new(7);
      let landed = post_to_channel(&t, ch, Rendered::text("hi")).await;
      assert_eq!(landed.map(|(c, _)| c), Some(ch));
      assert_eq!(t.ops(), [Op::Send(7)]);
  }

  #[tokio::test]
  async fn post_to_channel_swallows_a_failed_send() {
      let t = FakeTransport::default();
      *t.send_error.lock().unwrap() = Some(TransportError::Other("down".into()));
      let landed = post_to_channel(&t, GenericChannelId::new(7), Rendered::text("hi")).await;
      assert_eq!(landed, None);
  }
  ```
  Run them: they fail to compile.
- [ ] **Step 2:** Move the files and split them as above, add `post_to_channel`, and make `post`'s channel arm call it. Run the two tests: PASS.
- [ ] **Step 3:** Exclude `crack-common/src/test_support.rs` from mutants in `.cargo/mutants.toml`'s `exclude_globs`, with a reason comment like crack-test-db's (test-only fakes; as `#[cfg(test)]` code in crack-core they listed no mutants).
- [ ] **Step 4:** Verify: `cargo tree -p crack-common -e normal --prefix none | grep -E '^(poise|crack-core) '` prints nothing, and `grep -n 'poise' crack-common/src/{courier,discord,transport,throttle,test_support}.rs` prints nothing.
- [ ] **Step 5:** Run the suite, the doctests and the lint. All green; the count rises by exactly the two new tests.
- [ ] **Step 6: Commit:** `refactor(messaging): the poise-free delivery layer (transport, presses, throttle, Discord conversion, courier sends) moves to crack-common`

### Task 2: The `RoomsData` handle; the mod log stops taking `Data`

**Files:**
- Modify: `crack-core/src/rooms/mod.rs` (define `RoomsData`), `crack-core/src/lib.rs` (`DataInner.rooms`/`modlog` become `Arc`, `Data::rooms_data`, its test), `crack-core/src/modlog/{mod,live,batch,preflight,registry}.rs`
- Modify call sites where the compiler points: expect `rooms/*` (temporarily), `handlers/serenity.rs`, `commands/modlog.rs`, `crack-web/src/lib.rs`

**Interfaces:**
- Consumes: `crack_common::courier::post_to_channel` (Task 1).
- Produces: `crate::rooms::RoomsData` (exactly the struct in "Calls"), `Data::rooms_data(&self) -> RoomsData`, `DataInner.rooms: Arc<RoomRegistry>`, `DataInner.modlog: Arc<ModLogRegistry>`.
- Converted (`&Data` → `&RoomsData`, `Data` → `RoomsData`): every function, struct field and type alias in `modlog/` that names `Data`, including `load_settings`, `save_settings`, `live::{spawn_seed, on_voice_state, on_audit_event, after_save, mark_reconnected, on_guild_create}`, `live::ChannelSink.data`, `live::Load`, `batch::seed` and its helpers, and `preflight::for_guild`.
- `ChannelSink::post` renders the event and calls `post_to_channel(self.transport.as_ref(), channel.widen(), render(&msg, &RenderCx::now()))`, then logs its own failure line as now.
- Temporary bridge, removed in Task 3: rooms code that calls a converted mod-log function passes `&data.rooms_data()`.

- [ ] **Step 1: Write the failing test** in `crack-core/src/lib.rs`'s `lib_test` module:
  ```rust
  #[tokio::test]
  async fn rooms_data_carries_the_pool_and_shares_the_registries() {
      let pool = sqlx::postgres::PgPoolOptions::new()
          .connect_lazy("postgres://localhost/unused")
          .unwrap();
      let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
      let handle = data.rooms_data();
      assert!(handle.database_pool.is_some());
      assert!(Arc::ptr_eq(&handle.rooms, &data.rooms));
      assert!(Arc::ptr_eq(&handle.modlog, &data.modlog));
      assert!(Data::default().rooms_data().database_pool.is_none());
  }
  ```
- [ ] **Step 2:** Run it and see it fail to compile.
- [ ] **Step 3:** Define `RoomsData`, make the two fields `Arc`, add `Data::rooms_data`. Run the test: PASS.
- [ ] **Step 4:** Convert `modlog/` and fix each call site. Tests build `RoomsData { database_pool: Some(pool), ..Default::default() }` (or `RoomsData::default()`) where they built a `Data`.
- [ ] **Step 5:** Verify: `grep -nE '\bData\b|DataInner' crack-core/src/modlog/*.rs` prints nothing but plain comments (no intra-doc links).
- [ ] **Step 6:** Run the suite, the doctests and the lint. All green; the count rises by one.
- [ ] **Step 7: Commit:** `refactor(modlog): a RoomsData handle; the mod log stops taking Data`

### Task 3: The rooms stop taking `Data`; the cut is verified

**Files:**
- Modify: `crack-core/src/rooms/{interact,ops,lifecycle}.rs` and any other `rooms/` file the grep below names
- Modify call sites where the compiler points: expect `handlers/serenity.rs`, `commands/rooms/{room,admin,mod}.rs`, `crack-web/src/lib.rs`

**Interfaces:**
- Consumes: `RoomsData`, `Data::rooms_data` (Task 2).
- Converted: every function, struct field and type alias in `rooms/` that names `Data`, including `ops::{RoomCx.data, no_leave_coming, fold_channel, persist, load_settings, save_settings}`, `interact::{Live::of, Live::cx, handle_press, handle_submit, room_state}` and `lifecycle`'s (`kicked_from`, `abandon`, `discard`, `destroy`, `delete_room`, `delete_attempt`, `on_channel_delete`, `forget`, `drop_row`, `sweep_after`, `after_hub_join`, `reconcile_guild`, `reconcile_what_reads`, `load_rooms`, `settings_read`, `reconcile_with`, the voice-state entry points and their spawned rechecks).
- Task 2's `rooms_data()` bridges inside `rooms/` go away.
- Where a caller keeps a borrow (`Live::cx` returns `RoomCx<'a>`), bind the handle first: `let rooms = data.rooms_data();`.

- [ ] **Step 1:** Convert `rooms/` and fix each call site, as in Task 2 Step 4.
- [ ] **Step 2: Verify the cut.** Over the files that move in Task 4, this prints nothing, tests included:
  ```
  grep -nP 'crate::(Data|DataInner|Error|Context|commands|handlers|guild|http_utils|utils|connection|poise_ext|COIN_BUILD)\b|crate::music::(?!perms)|crate::messaging::(buttons|interface|status|placeholder|track_failed|pins)|courier::(post\b|Destination|ReplySink|PoiseReplies|reply)|\bpoise\b|\bData\b|DataInner' \
    crack-core/src/rooms/*.rs crack-core/src/modlog/*.rs crack-core/src/db/rooms.rs crack-core/src/db/modlog.rs
  ```
  (`-P` for the lookahead.) A match in a plain comment is fine (say so in the report). An intra-doc link (`` [`Data`] ``) is not: reword it.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green, counts conserved.
- [ ] **Step 4: Commit:** `refactor(rooms): the voice rooms stop taking Data`

### Task 4: Move into `crack-rooms`

**Files:**
- Create: `crack-rooms/Cargo.toml`, `crack-rooms/src/lib.rs`, `crack-rooms/src/db/mod.rs` (`pub mod modlog; pub mod rooms;`)
- Move (`git mv`):
  - `crack-core/src/rooms/` → `crack-rooms/src/rooms/`
  - `crack-core/src/modlog/` → `crack-rooms/src/modlog/`
  - `crack-core/src/db/{rooms,modlog}.rs` → `crack-rooms/src/db/`
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml`, `crack-core/src/lib.rs`, `crack-core/src/db/mod.rs`, `.github/workflows/{lint,build,coverage}.yml`, `docs/testing.md` (the feature lists and the `#[db_test]` crate list), `scripts/sqlx_sync.py` (build crack-rooms' tests too, like crack-economy's line)

**Interfaces:**
- crack-rooms' `lib.rs`: a doc comment, `pub mod db; pub mod modlog; pub mod rooms;`, and `#[cfg(test)] pub(crate) use crack_test_db::test_db;`.
- crack-rooms' features: `db-tests = []`, `mutation-testing = ["db-tests"]`.
- crack-rooms' dependencies are what the moved files import (including `crack-common`, `crack-economy`, `crack-types`, `crack-model` if named). Its dev-dependencies are what their tests import, including `crack-common = { path = "../crack-common", features = ["test-support"] }`, `crack-test-db` and `crack-test-macros`.
- crack-core:
  - `crack-rooms = { path = "../crack-rooms" }`; feature `db-tests` gains `"crack-rooms/db-tests"`;
  - `lib.rs`: `pub mod modlog; pub mod rooms;` become `pub use crack_rooms::{modlog, rooms};`;
  - `db/mod.rs`: `pub use crack_rooms::db::{modlog, rooms};`.
- Path rewrites, inside the moved files only:

  | From | To |
  |---|---|
  | `crate::messaging::{messages,format,render,cards,message,failure,throttle,transport,courier,discord,test_support}` | `crack_common::…` (same module) |
  | `crate::errors` | `crack_common::errors` |
  | `crate::music::perms::roles_resolvable` | `crack_common::perms::roles_resolvable` |
  | `crate::economy::…` | `crack_economy::…` |
  | `sqlx::migrate!("./test_migrations")` | `sqlx::migrate!("../crack-core/test_migrations")` |

  `crate::rooms::…`, `crate::modlog::…`, `crate::db::{rooms,modlog}…` and `super::…` stay as they are.
- An item crack-core or crack-web uses that was `pub(crate)` becomes `pub`.

- [ ] **Step 1:** Create the crate, move the files, apply the rewrites and write the shims. `cargo check --workspace --all-features` compiles.
- [ ] **Step 2:** Prove the boundary. Each of these prints nothing:
  - `cargo tree -p crack-rooms -e normal --prefix none | grep -E '^(poise|crack-core) '`
  - `cargo tree -p crack-rooms -e normal,dev --prefix none | grep -E '^crack-core '`
- [ ] **Step 3:** Prove the offline build: `env -u DATABASE_URL SQLX_OFFLINE=true cargo check -p crack-rooms --tests` compiles.
- [ ] **Step 4: CI.**
  - `lint.yml`: add "crack-rooms depends on no poise or crack-core" and "crack-rooms has no crack-core dev-dependency", modelled on crack-economy's two steps.
  - `build.yml` (all four lists) and `coverage.yml`: add `crack-rooms/db-tests`.
- [ ] **Step 5:** Run the suite with the Task 4 `FEATURES`, the doctests, the lint and `cargo test -p crack-rooms --features db-tests`. Confirm that last command compiled no crack-core (`grep -c 'Compiling crack-core'` on its output is 0). Counts conserved.
- [ ] **Step 6: Commit:** `refactor: crack-rooms — the voice rooms, the mod log and their queries leave crack-core`
