# Split step 4b-1: the music seams Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inside crack-core, cut every reference from the music code that will move to `crack-music` in 4b-2 to `Data`, `DataInner`, poise's `Context` and crack-core's Discord layer, with no change to what the bot does.

**Architecture:**
- **Task 1:** a `MusicData` handle (step 2–4a's pattern), with the lease and autoplay methods on it and `Data` delegating.
- **Task 2:**
  - a `Surface` trait carries the two things the player asks of the Discord layer, settling and echoing;
  - crack-core implements it;
  - `OpCx` holds a `MusicData`.
- **Task 3:** poise's constructors and `query.rs`'s `mode_*` become crack-core extension traits, as step 3 did with `ToPoiseReply`. `queue.rs`'s logged entry points split into a crack-core file.
- **Task 4:** the clean helpers the player calls move under `music/`, crack-testing drops its unused poise, and a grep proves the cut.

No crate is created here; 4b-2 does the move.

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise, songbird, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 4, "4b: crack-music, the player", and rules 1–9).

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same messages, buttons, embeds and Discord actions, byte for byte;
  - the same refreshes after every op (queue messages, now playing, finished), in the same order;
  - the same slash commands registered, with the same options and choices;
  - the same log lines (text and level);
  - no storage, schema, migration, `.sqlx/` or SQL text change.
- **Function bodies don't change.** The only changed body lines are the ones a task names:
  - Task 2: the volume op's two settings calls, `Settle::after`'s body becoming a `Surface::settle` call, and `remote`'s two `courier::post(…, Destination::Echo(…), …)` calls becoming `surface.echo(…)`;
  - delegations and moved bodies elsewhere are verbatim.
  
  Only signatures, imports, call sites and test set-up change otherwise. Any other changed body line needs a stated reason in the report.
- **Paths keep working.** Code that moves between files is re-exported at its old path (`pub use`), so callers elsewhere in crack-core, crack-web and crack-cli change only where this plan says so or the compiler points.
- **Strings stay in crack-common's `messages.rs`** (#537).
- **Typed serde only.** Never `serde_json::json!`, and never index a `serde_json::Value` in new code.
- **No local mutants runs** (owner, 2026-10-25). `cargo mutants --list` is allowed.
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. Write paths in full, never through a shell variable (zsh won't word-split one).
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`, with `SQLX_OFFLINE=true` alongside `DATABASE_URL`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap, from the worktree root `/home/lothrop/projects/cracktunes-split`). `FEATURES` is literal text to paste: `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests`
  - suite: `cargo nextest run --workspace --features FEATURES`
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `cargo clippy -p crack-economy --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Test counts are conserved.** Before your first change, record the count from `cargo nextest list --workspace --features FEATURES --message-format json 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["test-count"])'`, and the suite's passed/skipped line. Report both, before and after. None may disappear, and the skipped count may not rise. A new test raises the count by one; say which.
- **Waiting:** wait on long jobs in the foreground, with a blocking loop under the 10-minute Bash timeout. Never end the turn to wait for a notification.
- **Commits:**
  - never `git add -A`; add files by name, and use `git mv` for whole-file moves;
  - run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.

## The moving set and the glue

4b-2 moves these files. After this plan, they name nothing in the glue or above it:
- `crack-core/src/music/`: `mod.rs`, `audit.rs`, `audit_view.rs`, `autoplay.rs`, `autoplay_probe.rs`, `connected.rs` (new), `disconnect.rs`, `lease.rs`, `perms.rs`, `query.rs`, `queue.rs`, `remote.rs`, `surface.rs` (new), `track_data.rs` (new), `ops/*.rs`, `search/*.rs`;
- `crack-core/src/sources/*.rs` and `crack-core/src/http_utils.rs`;
- `crack-core/src/db/`: `queue_audit.rs`, `play_log.rs`, `play_history.rs`, `queue_snapshot.rs`, `playlist.rs`, `metadata.rs`, `musicreco.rs`, `track_reaction.rs`, `worker_pool.rs`, `user.rs`.

These crack-core files are the glue. They stay, and only they may name `Data`, poise or the Discord layer:
- `music/context.rs`, `music/resume.rs`;
- `music/data_impl.rs`, `music/core_surface.rs`, `music/from_ctx.rs`, `music/query_modes.rs` and `music/queue_logged.rs` (all new).

## Calls this plan makes against the spec

- **`MusicData`** (Task 1, in `music/mod.rs`; Task 2 adds `surface`):
  ```rust
  #[derive(Clone)]
  pub struct MusicData {
      pub database_pool: Option<sqlx::PgPool>,
      pub songbird: Arc<songbird::Songbird>,
      pub ct_client: crack_testing::CrackTrackClient<'static>,
      pub http_client: reqwest::Client,
      pub audit_tx: Option<tokio::sync::mpsc::Sender<audit::AuditEvent>>,
      pub db_channel: Option<tokio::sync::mpsc::Sender<crate::db::MetadataMsg>>,
      pub autoplay_buffer: Arc<autoplay::AutoplayBuffer>,
      pub guild_cache_map: Arc<tokio::sync::Mutex<HashMap<GuildId, crate::guild::cache::GuildCache>>>,
      pub playback_owners: Arc<dashmap::DashMap<GuildId, lease::PlaybackOwner>>,
      pub queue_locks: Arc<dashmap::DashMap<GuildId, Arc<tokio::sync::Mutex<()>>>>,
      pub guild: crate::guild::operations::GuildData,
      pub surface: Arc<dyn surface::Surface>, // Task 2
  }
  ```
  - Field types are exactly `DataInner`'s, apart from the two `Arc`s. `DataInner.playback_owners` and `DataInner.queue_locks` become `Arc<DashMap<…>>`.
  - It has a manual `Debug` that names the handle and skips `surface`. It has no `Default`. Tests build one through `Data::default().music_data()`, or with Task 2's recording surface.
  - `Data::music_data(&self) -> MusicData` sits in `lib.rs` next to `guild_data()`. It clones each field, sets `guild: self.guild_data()`, and from Task 2 sets `surface: Arc::new(CoreSurface { data: self.clone() })`.
- **Methods that move onto `MusicData`, bodies verbatim:**
  - `lease.rs`: `playback_owner`, `claim_playback`, `release_playback`, `lock_queue`;
  - `autoplay.rs`: `get_autoplay`, `set_autoplay`.
  
  `Data` keeps each as a one-line delegation (`self.music_data().<same>(<same args>)`), in a new `impl Data` block in `music/data_impl.rs`. That file is glue, so it never moves.
- **`Surface`** (Task 2, `music/surface.rs`):
  ```rust
  #[serenity::async_trait]
  pub trait Surface: Send + Sync {
      async fn settle(&self, settle: Settle, cx: &OpCx, call: Option<&Arc<Mutex<Call>>>,
                      anchor: Option<(GenericChannelId, MessageId)>);
      async fn echo(&self, guild: GuildId, transport: &dyn Transport, msg: CrackedMessage)
                    -> Option<(GenericChannelId, MessageId)>;
  }
  ```
  - Match `remote.rs`'s current `courier::post` arguments exactly for `echo`'s parameter and return types. If they differ from the sketch, follow the code and say so.
  - `CoreSurface` in `music/core_surface.rs` implements it.
    - It holds the `Data`, or an `Arc<Data>` if that keeps `Settle::after`'s body verbatim; today the body passes `cx.data.clone()` as an `Arc<Data>`.
    - `settle`'s body is today's `Settle::after` body, with `cx.data` read as `self.data`. `OpCx::transport` becomes `pub` so the body can still call it.
    - `echo` is `courier::post(&self.data, Destination::Echo(guild), transport, msg)`.
  - `Settle::after` becomes `cx.data.surface.settle(self, cx, call, anchor).await`. `Settle::now`, `Done::settle_now` and `settle_after` keep their bodies.
  - `music/ops/test_support.rs` gains `RecordingSurface`, which records each `(Settle, GuildId, anchor)` and each echoed message, and a `music_data_recording()` helper.
- **`OpCx.data: Arc<Data>` becomes `OpCx.data: MusicData`.**
  - `remote`'s public functions, `search::search`, `disconnect::disconnect` and the volume op take `&MusicData` where they took `&Data`/`Arc<Data>`.
  - Callers pass `&data.music_data()`; crack-web's seven calls pass `&self.deps.data.music_data()`.
  - The volume op's `data.set_volume(..)`/`data.get_volume(..)` become `data.guild.set_volume(..)`/`data.guild.get_volume(..)`. This covers both the `volume_on` parameter and `cx.data`.
- **Extension traits** (Task 3), each with bodies verbatim and implemented once.
  - Declare each method as `fn …(…) -> impl Future<Output = …>`, as `GuildSettingsOperations` does. A bare `async fn` in a public trait trips rustc's `async_fn_in_trait` warning, and CI runs with `-D warnings`. Implementations may use `async fn`. `Surface` is used as `dyn`, so it uses `#[serenity::async_trait]` instead.
  - In `music/from_ctx.rs`: `OpCxFromCtx { fn from_ctx(ctx: &Context<'_>) -> Result<OpCx, CrackedError> }` (it sets `data: ctx.data().music_data()`), `ActorFromCtx { fn from_ctx(ctx: &Context<'_>) -> Actor }` and `ReplyThenSettle { async fn reply_then_settle(self, ctx, cx, msg) -> Result<T, CrackedError> }` for `Done<T>`.
  - In `music/query_modes.rs`: `QueryModes`, holding the eight `mode_*` methods with their exact signatures, implemented for `NewQueryType`. `query_type_from_url` moves there too, as a free function re-exported from `query.rs`'s old path.
  - Private `query.rs` helpers that the modes call become `pub`.
- **`queue.rs` splits** (Task 3).
  - The `Logs` and `Delegates` rows of `SURFACE`, plus `enqueue_resolved_and_snapshot`, `ready_query`, `progress_transport` and any helper only they use, move to `music/queue_logged.rs`. `queue.rs` ends with `pub use super::queue_logged::*;`, so `crate::music::queue::queue_track_back` and friends still resolve.
  - The `SURFACE` table and its four source-scanning tests move to `queue_logged.rs`'s tests. `production_source()` returns the non-test part of both files, concatenated (`src/music/queue.rs`, then `src/music/queue_logged.rs`).
- **Helpers move under `music/`** (Task 4), with old paths re-exported:
  - `utils::{TrackData, track_data, get_track_handle_metadata, get_requesting_user, MUSIC_SEARCH_SUFFIX}` go to `music/track_data.rs`;
  - `commands::music_utils::connected_call` and `connection::get_voice_channel_for_user` go to `music/connected.rs` (`connected_call` keeps its `#[allow(clippy::disallowed_methods)]` and comment).
- **crack-testing drops `poise = { workspace = true }`** (Task 4). No file in it names poise. If `cargo check -p crack-testing` then fails, report it rather than re-adding.
- **One PR, v0.43.0,** a minor bump done by the controller after Task 4.

## Review Focus

1. **Every op still refreshes Discord the same way.** Commands, buttons and the dashboard all settle through `CoreSurface` now. A surface that dropped a settle would leave a stale queue or now-playing message with no error. Pinned by the existing crack-core tests that assert what a settle rendered (they keep running against `Data::music_data()`), and by Task 2's `settle_goes_through_the_surface` with the recording surface.
2. **A /gp game still blocks the dashboard and the commands.** `playback_owners` and `queue_locks` must be the same maps through `Data` and through every `MusicData`. Pinned by Task 1's `music_data_shares_the_lease_maps_and_the_pool` (`Arc::ptr_eq`) and the existing lease tests.
3. **`/volume` still saves the server's volume.** It now goes through `data.guild`. Pinned by `ops/playback.rs`'s existing volume tests.
4. **Dashboard echoes still honour "control echoes off".** Pinned by `remote`'s existing echo tests running through `CoreSurface`, or by a new one if none asserts it.
5. **The queue's history guarantees survive the split.** Every user entry point logs, delegations end in a logger, primitives have no context, and every user entry point clears autoplay's buffer. Pinned by the four `SURFACE` tests reading both files, plus `every_queue_entry_point_is_in_one_of_the_two_files` (Task 3), which fails if a `SURFACE` name is in neither file.

---

### Task 1: The `MusicData` handle; the lease and autoplay move onto it; `Data` delegates

**Files:**
- Modify: `crack-core/src/music/mod.rs` (`MusicData`, its `Debug`), `crack-core/src/lib.rs` (the two `Arc` fields, `Data::music_data`, its test), `crack-core/src/music/lease.rs`, `crack-core/src/music/autoplay.rs`
- Create: `crack-core/src/music/data_impl.rs` (`mod data_impl;` in `music/mod.rs`), holding the six `Data` delegations
- Modify where the compiler points: places that build `DataInner` with `playback_owners`/`queue_locks`

**Interfaces:**
- Produces: `crate::music::MusicData` (fields as in "Calls", without `surface`), `Data::music_data(&self) -> MusicData`, `impl MusicData { playback_owner, claim_playback, release_playback, lock_queue, get_autoplay, set_autoplay }`, plus `Data` delegations with unchanged signatures.

- [ ] **Step 1: Write the failing test** in `lib.rs`'s `lib_test` module:
  ```rust
  #[tokio::test]
  async fn music_data_shares_the_lease_maps_and_the_pool() {
      let pool = sqlx::postgres::PgPoolOptions::new()
          .connect_lazy("postgres://localhost/unused")
          .unwrap();
      let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
      let music = data.music_data();
      assert!(music.database_pool.is_some());
      assert!(Arc::ptr_eq(&music.playback_owners, &data.playback_owners));
      assert!(Arc::ptr_eq(&music.queue_locks, &data.queue_locks));
      assert!(Arc::ptr_eq(&music.autoplay_buffer, &data.autoplay_buffer));
      assert!(Arc::ptr_eq(&music.guild_cache_map, &data.guild_cache_map));
      assert!(Arc::ptr_eq(&music.songbird, &data.songbird));
      assert!(Arc::ptr_eq(&music.guild.guild_settings_map, &data.guild_settings_map));

      let guild = serenity::GuildId::new(9);
      data.claim_playback(guild, crate::music::PlaybackOwner::Game).unwrap();
      assert_eq!(music.playback_owner(guild), crate::music::PlaybackOwner::Game);
  }
  ```
  Run it: it fails to compile.
- [ ] **Step 2:** Make the two fields `Arc`, define `MusicData` and `Data::music_data`, move the six methods onto `MusicData`, and write the delegations. Run the test: PASS.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green; the count rises by exactly one.
- [ ] **Step 4: Commit:** `refactor(music): a MusicData handle; the lease and autoplay's switch move onto it, and Data delegates`

### Task 2: The `Surface` trait; `OpCx` holds a `MusicData`

**Files:**
- Create: `crack-core/src/music/surface.rs` (`pub mod surface;`) and `crack-core/src/music/core_surface.rs` (`pub mod core_surface;`), both declared in `music/mod.rs`
- Modify: `crack-core/src/music/mod.rs` (the `surface` field and its `Debug`), `crack-core/src/lib.rs` (`music_data` sets the surface), `crack-core/src/music/ops/mod.rs` (`OpCx.data`, `Settle::after`), `crack-core/src/music/ops/*.rs`, `crack-core/src/music/ops/test_support.rs` (`RecordingSurface`), `crack-core/src/music/remote.rs`, `crack-core/src/music/search/*.rs`, `crack-core/src/music/disconnect.rs`
- Modify call sites where the compiler points: expect commands, buttons, handlers, `economy/walk_on.rs`, `/gp`, and `crack-web/src/lib.rs`

**Interfaces:**
- Consumes: `MusicData`, `Data::music_data` (Task 1).
- Produces: `crate::music::surface::Surface`, `crate::music::core_surface::CoreSurface`, `MusicData.surface`, `OpCx { data: MusicData, http, cache, guild_id, actor }`, `RecordingSurface` (test-only), and `remote::*`/`search::search`/`disconnect::disconnect` taking `&MusicData`.

- [ ] **Step 1: Write the failing test** in `music/ops/mod.rs`'s tests, `settle_goes_through_the_surface`:
  - build an `OpCx` whose `MusicData` has a `RecordingSurface`;
  - call `Settle::NowPlaying.after(&cx, Some(&call), Some(anchor)).await` and `Settle::Finished.now(&cx, None).await`;
  - assert the surface recorded exactly `[(Settle::NowPlaying, GUILD, Some(anchor)), (Settle::Finished, GUILD, None)]`, in order.
  
  Run it: it fails to compile.
- [ ] **Step 2:**
  - Define `Surface` and `CoreSurface`, add the field, and route `Settle::after` and `remote`'s two echoes through it.
  - Change `OpCx.data` and the functions in "Calls" to `MusicData`, and fix each call site.
  - Op tests that assert what a settle *rendered* keep a `Data::default().music_data()` (the real surface). Op tests that only need some surface may use the recording one.
  
  Run the new test: PASS.
- [ ] **Step 3:** If no existing test asserts that a dashboard control posts nothing with control echoes off, add `remote_echo_respects_control_echoes_off` in `remote.rs`'s tests, through `CoreSurface`. Say which test pins Review Focus 4.
- [ ] **Step 4:** Run the suite, the doctests and the lint. All green; the count rises by the new tests only.
- [ ] **Step 5: Commit:** `refactor(music): a Surface trait for settling and echoing; OpCx holds a MusicData`

### Task 3: poise's constructors and the query modes become extension traits; `queue.rs` splits

**Files:**
- Create: `crack-core/src/music/from_ctx.rs`, `crack-core/src/music/query_modes.rs`, `crack-core/src/music/queue_logged.rs` (each `mod`'d in `music/mod.rs`, `pub` where callers need it)
- Modify: `crack-core/src/music/{audit,query,queue}.rs`, `crack-core/src/music/ops/mod.rs`
- Modify call sites where the compiler points (imports only): `commands/music/*`, `commands/music/doplay.rs`, `commands/music/get_metadata.rs`, buttons, `/gp`

**Interfaces:**
- Produces: the traits `OpCxFromCtx`, `ActorFromCtx`, `ReplyThenSettle` (in `crate::music::from_ctx`) and `QueryModes` (in `crate::music::query_modes`), with method names and signatures unchanged. Also `crate::music::query::query_type_from_url` (re-exported), and every queue entry point still at `crate::music::queue::<name>`.

- [ ] **Step 1: Write the failing test** in `queue_logged.rs`'s tests:
  ```rust
  #[test]
  fn every_queue_entry_point_is_in_one_of_the_two_files() {
      let src = production_source();
      for (name, _) in SURFACE {
          assert!(
              src.contains(&format!("fn {name}")),
              "{name} is in SURFACE but defined in neither queue.rs nor queue_logged.rs"
          );
      }
  }
  ```
  It fails until the split exists.
- [ ] **Step 2:** Move the constructors and modes into the extension traits, split `queue.rs`, and move the `SURFACE` tests. Add `use` lines where the compiler asks. Run the new test and the four moved `SURFACE` tests: PASS.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green; the count rises by one.
- [ ] **Step 4: Commit:** `refactor(music): poise's constructors and the query modes become crack-core extension traits; queue.rs's logged entry points split out`

### Task 4: The helpers move under `music/`; the last poise names leave the moving set

*(Amended during execution, 2026-10-26: a pre-Task-4 run of the cut grep found what the original Task 4 didn't foresee. The test fixtures moved to a new Task 5.)*

**Files:**
- Create: `crack-core/src/music/track_data.rs`, `crack-core/src/music/connected.rs`
- Modify:
  - `crack-core/src/utils.rs`, `crack-core/src/commands/music_utils.rs` and `crack-core/src/connection.rs`, each re-exporting what left;
  - `crack-core/src/music/mod.rs`, `crack-core/src/music/audit_view.rs` and `crack-core/src/commands/music/auditlog.rs`;
  - the moving files that import `poise::serenity_prelude` or `poise::futures_util`: `music/lease.rs`, `music/mod.rs`, `music/perms.rs` and `db/play_log.rs`;
  - `music/remote.rs` (the `DiscordTransport` import), `crack-testing/Cargo.toml` and `Cargo.lock`.

**Interfaces:**
- Produces: the helpers in "Calls" at their new paths, still reachable at the old ones.

**Rulings for this task:**
- **`ActionChoice` and `SourceChoice`** (`music/audit_view.rs`, ~255-345) derive `poise::ChoiceParameter`, and only the `/auditlog` command uses them. They move into `crack-core/src/commands/music/auditlog.rs` with their two `impl` blocks and their tests (`audit_view.rs`'s tests that name them, ~810-901). auditlog.rs's import changes to match.
  - Why: once they are in crack-music, crack-core can't implement poise's trait for them, because the orphan rule forbids it.
- **`poise::serenity_prelude`** becomes `serenity` (the `serenity` crate or `::serenity::all`, as crack-core's other files do). **`poise::futures_util::StreamExt`** becomes `futures::StreamExt`; add `futures` to nothing new if crack-core already depends on it, otherwise say so.
- **`remote.rs`'s `crate::messaging::status::DiscordTransport`** becomes `crate::messaging::transport::DiscordTransport` (crack-common's, re-exported).

- [ ] **Step 1:** Move the helpers and add the re-exports, move the two choice enums, rewrite the poise imports, and drop crack-testing's poise. Checks:
  - `cargo check --workspace --all-features` compiles;
  - `cargo tree -p crack-testing -e normal --prefix none | grep -E '^poise '` prints nothing;
  - `grep -n 'poise' <the moving set's non-test code>` prints only comments.
- [ ] **Step 2:** Run the suite, the doctests and the lint. All green, counts conserved (the choice enums' tests move, they don't disappear).
- [ ] **Step 3: Commit:** `refactor(music): the player's helpers move under music/; /auditlog's choice enums move to the command; the moving set stops naming poise`

### Task 5: The moving set's tests stop building `Data`; the cut is proved

**Files:**
- Modify:
  - `crack-core/src/music/ops/test_support.rs`;
  - the tests in `music/{disconnect,lease,autoplay,queue,remote}.rs`, `music/ops/{mod,add,skip}.rs` and `music/search/{mod,history,youtube}.rs`;
  - `crack-core/src/music/core_surface.rs` and `music/data_impl.rs`, which receive tests that must stay in crack-core.

**Interfaces:**
- Produces: in `music/ops/test_support.rs` (test-only), `pub fn test_music_data() -> MusicData`. It builds every field without `Data`:
  - each field the way `DataInner::default()` builds the same field;
  - `guild: GuildData::default()`;
  - `surface: Arc::new(RecordingSurface::default())`.
  
  Tests adjust it with struct-update syntax (`MusicData { audit_tx: Some(tx), ..test_music_data() }`).

**Rulings for this task:**
- **Tests in the moving set that build `Data`/`DataInner` only to reach music state** switch to `test_music_data()`, keeping the same assertions.
  - Where a test called a `Data` delegation (`claim_playback`, `lock_queue`, `get_autoplay`…), it calls the same method on the `MusicData`.
  - Where it set a guild setting through `Data`, it sets it through `music.guild`.
- **Tests that assert what crack-core's `CoreSurface` rendered or sent** move to crack-core glue test modules with the same names, keeping `Data`:
  - status or queue-message renders after a settle;
  - an echo landing below the status message;
  - anything using `messaging::{status, interface}`, e.g. `queue.rs`'s test near line 1290 and `remote.rs`'s near 604.
  
  Put each in `music/core_surface.rs`'s tests (settle/echo rendering) or `music/queue_logged.rs`'s tests (queue embeds), and say which.
- A test may not be deleted. If one can't be converted or moved, report it.

- [ ] **Step 1:** Add `test_music_data()`, convert or move each test as above.
- [ ] **Step 2: Verify the cut.** Over the moving set, this prints nothing, tests included. Run it from `crack-core/src`:
  ```
  grep -nP 'crate::(Data|DataInner|Context|commands|handlers|poise_ext|config|BotConfig|utils|connection|owners|metrics|economy|coin|rooms|modlog)\b|crate::guild::data_impl|crate::db::gp\b|crate::music::(context|resume|data_impl|core_surface|from_ctx|query_modes|queue_logged)\b|crate::messaging::(status|interface|buttons|placeholder|track_failed|pins)\b|courier::(post\b|Destination|reply\b|locate|ReplySink|PoiseReplies)|\bpoise\b|\bData\b|DataInner' \
    music/mod.rs music/audit.rs music/audit_view.rs music/autoplay.rs music/autoplay_probe.rs music/connected.rs music/disconnect.rs music/lease.rs music/perms.rs music/query.rs music/queue.rs music/remote.rs music/surface.rs music/track_data.rs music/ops/*.rs music/search/*.rs sources/*.rs http_utils.rs \
    db/queue_audit.rs db/play_log.rs db/play_history.rs db/queue_snapshot.rs db/playlist.rs db/metadata.rs db/musicreco.rs db/track_reaction.rs db/worker_pool.rs db/user.rs
  ```
  - A match in a plain comment is fine (say so in the report). An intra-doc link (`` [`Data`] ``) is not: reword it.
  - `music/mod.rs` is in the list because `MusicData` lives there. Its `mod` lines for the glue files and its `pub use context::QueryContext;` are expected: 4b-2 splits it. The grep's `crate::` patterns don't match them.
  - A match this plan didn't foresee is a reference to cut. Report it if it needs a design call.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green, counts conserved (tests move, none disappear).
- [ ] **Step 4: Commit:** `test(music): the player's tests build a MusicData, not Data; the cut is complete`
