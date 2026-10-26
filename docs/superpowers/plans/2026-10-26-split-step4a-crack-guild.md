# Split step 4a: crack-guild Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move guild settings (`guild/settings.rs`, `permissions.rs`, `plan.rs`, `operations.rs`), the per-guild session cache (`guild/cache.rs`) and their queries (`db/guild.rs`) out of crack-core into a new crate, `crack-guild`, with no change to what the bot does.

**Architecture:** Still inside crack-core, the guild settings trait stops being implemented by `Data`'s fields and is implemented by a narrow `GuildData` handle instead, as steps 2 and 3 did. `Data` keeps implementing the trait by delegating to `self.guild_data()`, so no call site changes. Autoplay's session switch, which reads music state, leaves the trait for crack-core's music code (Task 1). Then `git mv` moves the files and crack-core keeps `pub use` shims, so every `crate::guild::…` and `crate::db::guild…` path keeps working (Task 2).

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`, `crack-test-db`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 4, "4a: crack-guild", and rules 1–9).

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same messages, replies and Discord actions, byte for byte;
  - the same slash commands registered, with the same options and choices;
  - the same log lines (text and level);
  - no storage, schema, migration, `.sqlx/` or SQL text change.
- **Function bodies don't change.** The only changed body lines are the four `self.bot_settings.get_prefix()` reads in the trait's implementation, which become reads of `self.bot_prefix` (see "Calls"), and the new one-line `Data` delegations. Only signatures, imports, call sites and test set-up change otherwise. Any other changed body line needs a stated reason in the report.
- **crack-guild never depends on poise or crack-core,** not even as a dev-dependency (spec rules 1, 4 and 9). It may depend on crack-common, crack-model, crack-types, serenity and the external crates the moved files already use (rule 6). It does not depend on crack-economy or crack-rooms.
- **No `Data`, `DataInner` or poise `Context` in a moved file** (rule 2). serenity's `Context` and `Http` are allowed.
- **Dependency versions.** Copy each new dependency line exactly from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains the new crate and no second version of any crate.
- **Paths keep working.** crack-core re-exports what moves at its old path. Call sites elsewhere in crack-core, crack-web and crack-cli change only where this plan says so or the compiler points.
- **Strings stay in crack-common's `messages.rs`** (#537). No new `pub const` strings anywhere else.
- **Typed serde only.** Never `serde_json::json!`, and never index a `serde_json::Value` in new code. Moved test code keeps what it has.
- **No local mutants runs** (owner, 2026-10-25). `cargo mutants --list` is allowed: it builds nothing.
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. Write paths in full, never through a shell variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`, with `SQLX_OFFLINE=true` alongside `DATABASE_URL`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap, from the worktree root `/home/lothrop/projects/cracktunes-split`). `FEATURES` below is literal text to paste, not a shell variable:
  - Task 1: `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests`
  - Task 2 adds `,crack-guild/db-tests`.
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

- **The handle** (spec 4a, "The handle"), defined in `crack-core/src/guild/operations.rs` so it moves with the trait:
  ```rust
  /// The guild settings' state, as a handle that does not name `Data`.
  #[derive(Debug, Clone)]
  pub struct GuildData {
      pub database_pool: Option<sqlx::PgPool>,
      pub guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>,
      pub bot_prefix: String,
  }
  ```
  - The map type is exactly `DataInner.guild_settings_map`'s (tokio's `RwLock`, serenity's `GuildId`).
  - `impl Default for GuildData`: no pool, an empty map, `bot_prefix: DEFAULT_PREFIX.to_string()` (`DEFAULT_PREFIX` is in `guild/settings.rs`).
  - `Data::guild_data(&self) -> GuildData` sits in `crack-core/src/lib.rs` next to `rooms_data()`: it clones the pool and the map's `Arc`, and sets `bot_prefix: self.bot_settings.get_prefix()`.
- **The trait's implementation moves from `Data` to `GuildData`,** bodies verbatim, except:
  - `let prefix = self.bot_settings.get_prefix();` becomes `let prefix = self.bot_prefix.clone();`;
  - `Some(&self.bot_settings.get_prefix())` becomes `Some(&self.bot_prefix)` (three sites).
- **`get_autoplay` and `set_autoplay` leave the trait.** They become inherent methods in crack-core's `music/autoplay.rs`, in an `impl crate::Data` block below `AutoplayBuffer`, with the same doc comments and bodies. Their three tests (`test_get_set_autoplay`, `autoplay_is_off_by_default_and_the_setter_works_without_a_cache_entry`, `turning_autoplay_off_drops_the_buffer`) move into that file's test module unchanged. Callers keep calling `data.get_autoplay(..)`. Where a file imported the trait only for these two methods, the import goes.
- **`Data` delegates.** `impl GuildSettingsOperations for Data` moves to a new crack-core file, `crack-core/src/guild/data_impl.rs` (`mod data_impl;` in `guild/mod.rs`). Each method is one line, `self.guild_data().<same method>(<same args>).await`, so its file never moves.
- **Tests that stay in crack-core,** in `data_impl.rs`'s test module:
  - `every_command_that_saves_settings_also_guards_first`, because it reads crack-core's `commands/` directory. Moved there unchanged, apart from any path it builds from `CARGO_MANIFEST_DIR`, which must still point at crack-core.
  - the new delegation test (Task 1 Step 1).
- **The trait's other tests stay with it** in `operations.rs`. They build a `GuildData` where they built a `Data`:
  - `Data(Arc::new(DataInner { guild_settings_map: X, ..Default::default() }))` becomes `GuildData { guild_settings_map: X, ..Default::default() }`;
  - `crate::Data::default()` becomes `GuildData::default()`, and an `Arc::new(Data(…))` wrapper becomes the bare `GuildData`;
  - their assertions don't change.
- **`cache.rs`'s `use poise::serenity_prelude as serenity;`** becomes a plain serenity import, the only change to that file.
- **One PR,** as in steps 1–3. **v0.42.0**, a minor bump, done by the controller after Task 2. `guild/cam_rules.rs` (empty, unreferenced) stays where it is.

## Review Focus

1. **Writes through `Data` and through the handle land in the same map.** A handle holding a copy would make every settings command write into a map nobody reads. Pinned in Task 1 by `guild_data_carries_the_pool_shares_the_map_and_takes_the_bot_prefix` (`Arc::ptr_eq`) and by the delegation test.
2. **A server with a configured prefix keeps it.** The settings created when a guild has none (toggles and `ensure_settings_loaded`) use the bot's configured prefix, not `DEFAULT_PREFIX`. Pinned by the same test with a non-default prefix.
3. **A delegation wired to the wrong method.** `get_autopause` and `get_autoplay_setting` are both `bool`, so the compiler wouldn't catch the swap. Pinned by `data_delegates_every_setting_to_the_shared_map`, which uses values that differ from every default.
4. **Turning autoplay off still drops the recommender's buffer.** Pinned by `turning_autoplay_off_drops_the_buffer`, which keeps running against `Data`.
5. **Guild db tests still run in CI, not compile-and-skip,** and the offline Docker build compiles crack-guild's `query!` macros. Checked by the conserved test and skip counts, and in Task 2 with `SQLX_OFFLINE=true`.

---

### Task 1: The `GuildData` handle; `Data` delegates; autoplay's switch leaves the trait

**Files:**
- Modify: `crack-core/src/guild/operations.rs` (define `GuildData`, implement the trait for it, convert its tests), `crack-core/src/guild/mod.rs` (`mod data_impl;`), `crack-core/src/guild/cache.rs` (the import), `crack-core/src/lib.rs` (`Data::guild_data` and its test), `crack-core/src/music/autoplay.rs` (the two inherent methods and their three tests)
- Create: `crack-core/src/guild/data_impl.rs`
- Modify where the compiler or clippy points: imports of `GuildSettingsOperations` that become unused

**Interfaces:**
- Produces: `crate::guild::operations::GuildData` (exactly the struct in "Calls"), `impl Default for GuildData`, `impl GuildSettingsOperations for GuildData`, `Data::guild_data(&self) -> GuildData`, inherent `Data::get_autoplay(&self, GuildId) -> bool` and `Data::set_autoplay(&self, GuildId, bool)`.
- The trait keeps every other method and signature as it is. `get_autoplay_setting` and `set_autoplay_setting` stay.

- [ ] **Step 1: Write the failing tests.**
  In `crack-core/src/lib.rs`'s `lib_test` module:
  ```rust
  #[tokio::test]
  async fn guild_data_carries_the_pool_shares_the_map_and_takes_the_bot_prefix() {
      let pool = sqlx::postgres::PgPoolOptions::new()
          .connect_lazy("postgres://localhost/unused")
          .unwrap();
      let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
      let handle = data.guild_data();
      assert!(handle.database_pool.is_some());
      assert!(Arc::ptr_eq(&handle.guild_settings_map, &data.guild_settings_map));
      assert!(Data::default().guild_data().database_pool.is_none());
      assert_eq!(Data::default().guild_data().bot_prefix, GuildData::default().bot_prefix);

      let custom = Data(Arc::new(DataInner {
          bot_settings: BotConfig { prefix: Some("x!".to_string()), ..Default::default() },
          ..Default::default()
      }));
      assert_eq!(custom.guild_data().bot_prefix, "x!");
  }
  ```
  In `data_impl.rs`'s test module, `data_delegates_every_setting_to_the_shared_map`:
  - Store, through `data.guild_data().set_guild_settings(..)`, a `GuildSettings` where every field a getter reads differs from its default. Then assert each `Data` getter returns that literal value: one `assert_eq!` per getter, 16 in all (`get_guild_settings`, `get_music_channel`, `get_timeout`, `get_premium`, `get_prefix`, `get_additional_prefixes`, `get_autopause`, `get_auto_role`, `get_autoplay_setting`, `get_volume`, `get_reply_with_embed`, `get_ephemeral_replies`, `get_control_echoes`, `get_now_playing_buttons`, plus `get_or_create_guild_settings` returning the stored settings).
  - Then call each `Data` setter and toggle once and read the result back through `data.guild_data()`'s getter, one `assert_eq!` each. `toggle_*` methods that need `ensure_settings_loaded` work without a pool.
  - `Data::default()` has no pool, so nothing touches a database.

  Run both tests: they fail to compile.
- [ ] **Step 2:** Define `GuildData` and its `Default`, add `Data::guild_data`, move the trait's implementation onto `GuildData` (the "Calls" edits only), and write `data_impl.rs`'s delegations. Move `get_autoplay`/`set_autoplay` to `music/autoplay.rs` with their three tests. Convert `operations.rs`'s remaining tests, and move the source-scanning test into `data_impl.rs`. Run the two new tests: PASS.
- [ ] **Step 3: Verify the cut.** Over the files that move in Task 2, this prints nothing, tests included:
  ```
  grep -nP 'crate::(Data|DataInner|Context|commands|handlers|music|http_utils|utils|connection|poise_ext|rooms|modlog|economy|config|BotConfig)\b|crate::messaging::(?!message\b)|\bpoise\b|\bData\b|DataInner|bot_settings|guild_cache_map|autoplay_buffer' \
    crack-core/src/guild/settings.rs crack-core/src/guild/permissions.rs crack-core/src/guild/plan.rs crack-core/src/guild/cache.rs crack-core/src/guild/operations.rs crack-core/src/db/guild.rs
  ```
  A match in a plain comment is fine (say so in the report). An intra-doc link (`` [`Data`] ``) is not: reword it.
- [ ] **Step 4:** Run the suite, the doctests and the lint. All green; the count rises by exactly the two new tests.
- [ ] **Step 5: Commit:** `refactor(guild): a GuildData handle; Data delegates the settings trait to it, and autoplay's session switch moves to music`

### Task 2: Move into `crack-guild`

**Files:**
- Create: `crack-guild/Cargo.toml`, `crack-guild/src/lib.rs`, `crack-guild/src/db/mod.rs` (`pub mod guild; pub use guild::*;`)
- Move (`git mv`):
  - `crack-core/src/guild/{settings,permissions,plan,cache,operations}.rs` → `crack-guild/src/`
  - `crack-core/src/db/guild.rs` → `crack-guild/src/db/guild.rs`
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml`, `crack-core/src/guild/mod.rs`, `crack-core/src/db/mod.rs`, `.github/workflows/{lint,build,coverage}.yml`, `.cargo/mutants.toml` (the crate-list comment), `docs/testing.md` (the feature lists, the `#[db_test]` crate list and per-crate counts), `scripts/sqlx_sync.py` (build crack-guild's tests too, like crack-rooms' line)

**Interfaces:**
- crack-guild's `lib.rs`: a doc comment, `pub mod cache; pub mod db; pub mod operations; pub mod permissions; pub mod plan; pub mod settings;`, and `#[cfg(test)] pub(crate) use crack_test_db::test_db;` if the moved db tests need it.
- crack-guild's features: `db-tests = []`, `mutation-testing = ["db-tests"]`.
- crack-guild's dependencies are what the moved files import. Its dev-dependencies are what their tests import, including `crack-test-db` and `crack-test-macros`.
- crack-core:
  - `crack-guild = { path = "../crack-guild" }`; feature `db-tests` gains `"crack-guild/db-tests"`;
  - `guild/mod.rs` becomes:
    ```rust
    pub use crack_guild::{cache, permissions, plan, settings};
    pub mod operations {
        pub use crack_guild::operations::*;
    }
    mod data_impl;
    ```
  - `db/mod.rs`: `pub mod guild;` becomes `pub use crack_guild::db::guild;` (`pub use guild::*;` stays).
- Path rewrites, inside the moved files only:

  | From | To |
  |---|---|
  | `crate::errors::…`, `crate::CrackedError` | `crack_common::errors::…`, `crack_common::errors::CrackedError` |
  | `crate::CrackedResult` | `crack_common::CrackedResult` |
  | `crate::Error` (also `Error as SerenityError`) | `crack_common::errors::Error` |
  | `crate::messaging::message::CrackedMessage` | `crack_common::message::CrackedMessage` |
  | `crate::guild::X`, `crate::GuildSettings` | `crate::X`, `crate::settings::GuildSettings` |
  | `sqlx::migrate!("./test_migrations")` | `sqlx::migrate!("../crack-core/test_migrations")` |

  `crate::db::…` and `super::…` stay as they are.
- An item crack-core or crack-web uses that was `pub(crate)` becomes `pub`.

- [ ] **Step 1:** Create the crate, move the files, apply the rewrites and write the shims. `cargo check --workspace --all-features` compiles.
- [ ] **Step 2:** Prove the boundary. Each of these prints nothing:
  - `cargo tree -p crack-guild -e normal --prefix none | grep -E '^(poise|crack-core|crack-economy|crack-rooms) '`
  - `cargo tree -p crack-guild -e normal,dev --prefix none | grep -E '^crack-core '`
- [ ] **Step 3:** Prove the offline build: `env -u DATABASE_URL SQLX_OFFLINE=true cargo check -p crack-guild --tests` compiles.
- [ ] **Step 4: CI.**
  - `lint.yml`: add "crack-guild depends on no poise or crack-core" and "crack-guild has no crack-core dev-dependency", modelled on crack-rooms' two steps.
  - `build.yml` (all four lists) and `coverage.yml`: add `crack-guild/db-tests`.
- [ ] **Step 5:** Run the suite with the Task 2 `FEATURES`, the doctests, the lint and `cargo test -p crack-guild --features db-tests`. Confirm that last command compiled no crack-core (`grep -c 'Compiling crack-core'` on its output is 0). Counts conserved.
- [ ] **Step 6: Commit:** `refactor: crack-guild — guild settings, the per-guild cache and their queries leave crack-core`
