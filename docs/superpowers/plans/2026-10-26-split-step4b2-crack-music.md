# Split step 4b-2: crack-music Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the music player out of crack-core into a new crate, `crack-music`, with no change to what the bot does. The player is the files 4b-1 cut free: `music/` minus the glue, `sources/`, `http_utils.rs`, and the player's tables.

**Architecture:**
- **Task 1:** `git mv` the moving set into `crack-music/src/`, under the same module layout (`music/`, `sources/`, `http_utils.rs`, `db/`), so intra-crate `crate::music::…` paths stay verbatim. crack-core keeps shims, so every old path still resolves.
- **Task 2:** CI, docs and scripts learn about the crate, the boundary is proved, and the 4b-1 review's leftover minors are closed.

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise, songbird, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`, `crack-test-db`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 4, "4b: crack-music, the player", and rules 1–9). 4b-1 (ct#644) cut the seams. Its final review's "Recommendations" are this plan's inputs, and each is answered under "Calls".

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same messages, buttons, embeds, refreshes and Discord actions;
  - the same commands;
  - the same log lines (text and level). Log *targets* change from `crack_core::music::…` to `crack_music::music::…`, and nothing filters on them;
  - no storage, schema, migration, `.sqlx/` or SQL text change.
- **Function bodies don't change.** Only `use` lines and paths, visibility, manifests, the `production_source()` path and CI/docs change.
- **crack-music never depends on poise or crack-core,** not even as a dev-dependency (spec rules 1, 4 and 9).
  - It may depend on crack-common, crack-model, crack-types, crack-guild, crack-testing, crack-musicreco, crack-sleevenote, serenity, songbird and the external crates the moved files already use.
  - It does not depend on crack-economy or crack-rooms.
- **Dependency versions:** copy each new dependency line exactly from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains crack-music and no second version of any crate.
- **Paths keep working.** crack-core re-exports what moves at its old path. Call sites elsewhere in crack-core, crack-web and crack-cli change only where this plan says so or the compiler points.
- **Typed serde only.** No local mutants runs (`--list` is fine).
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. zsh won't word-split a variable, so write it out.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`, with `SQLX_OFFLINE=true` alongside `DATABASE_URL`.
- **Verification commands** (under the cap, from `/home/lothrop/projects/cracktunes-split`). `FEATURES` is literal text to paste:
  - Task 1 `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests`
  - Task 2: the same.
  - suite: `cargo nextest run --workspace --features FEATURES`
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `cargo clippy -p crack-economy --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Test counts are conserved:** record before and after, as in earlier plans. Tests move crate; none disappear, and the skipped count stays.
- **Commits:**
  - never `git add -A`; use `git mv` for moves; run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.

## The moving set

From `crack-core/src/` to `crack-music/src/`, same relative paths:
- `music/`: `mod.rs`, `audit.rs`, `audit_view.rs`, `autoplay.rs`, `autoplay_probe.rs`, `connected.rs`, `disconnect.rs`, `lease.rs`, `perms.rs`, `query.rs`, `queue.rs`, `remote.rs`, `surface.rs`, `track_data.rs`, `ops/*.rs` and `search/*.rs`;
- `sources/*.rs` and `http_utils.rs`;
- `db/`: `queue_audit.rs`, `play_log.rs`, `play_history.rs`, `queue_snapshot.rs`, `playlist.rs`, `metadata.rs`, `musicreco.rs`, `track_reaction.rs`, `worker_pool.rs` and `user.rs`.

These stay in `crack-core/src/music/` as glue: `context.rs`, `resume.rs`, `core_surface.rs`, `data_impl.rs`, `from_ctx.rs`, `query_modes.rs` and `queue_logged.rs`.

## Calls this plan makes against the spec and the 4b-1 review

- **The crate name is `crack-music`.** crack-core has a *feature* `crack-music = []`. Cargo accepts a feature and a non-optional dependency of the same name (checked 2026-10-26 with a scratch workspace), so the spec's "pick a name that doesn't collide" is satisfied. The feature stays as it is.
- **Layout:**
  - `crack-music/src/lib.rs` is a doc comment, `pub mod db; pub mod http_utils; pub mod music; pub mod sources;`, plus `#[cfg(test)] pub(crate) use crack_test_db::test_db;` if the moved db tests need it.
  - `crack-music/src/db/mod.rs` is `pub mod <each moved table>;` plus the same `pub use X::*;` globs crack-core's `db/mod.rs` has for them today (`metadata`, `musicreco`, `play_log`, `playlist`, `track_reaction`, `user`, `worker_pool`).
- **crack-core's shims:**
  - `lib.rs`: `pub mod http_utils; pub mod sources;` become `pub use crack_music::{http_utils, sources};`.
  - `db/mod.rs`: each moved `pub mod X;` becomes `pub use crack_music::db::X;`. The `pub use X::*;` globs stay.
  - A **new** `music/mod.rs` (the old one moves):
    ```rust
    pub use crack_music::music::*;
    pub mod context;
    pub mod core_surface;
    mod data_impl;
    pub mod from_ctx;
    pub mod query_modes;
    pub(crate) mod queue_logged;
    pub mod resume;
    pub use context::QueryContext;
    pub mod queue {
        pub use crack_music::music::queue::*;
        pub use super::queue_logged::*;
    }
    pub mod query {
        pub use crack_music::music::query::*;
        pub use super::query_modes::query_type_from_url;
    }
    pub(crate) use query::*;
    pub(crate) use queue::*;
    ```
    The local `queue`/`query` modules shadow the glob's. crack-music's `queue.rs` and `query.rs` lose their `pub use super::queue_logged::*;` and query-modes re-export lines, which 4b-1 ruled interim. `queue_logged.rs` imports crack-music's queue items through `crate::music::queue` (the shim) or `crack_music::music::queue` directly.
  - crack-music's `music/mod.rs` drops the glue `mod` lines and `pub use context::QueryContext;`.
- **Root names in the moved files:**

  | From | To |
  |---|---|
  | `crate::errors::…`, `crate::CrackedError` | `crack_common::errors::…`, `crack_common::errors::CrackedError` |
  | `crate::CrackedResult` | `crack_common::CrackedResult` |
  | `crate::Error` | `crack_common::errors::Error` |
  | `crate::messaging::{messages, message, format, render, cards, failure, transport, throttle, discord, courier}::…` | `crack_common::<same>::…` |
  | `crate::guild::…` | `crack_guild::…` (`cache`, `settings`, `permissions`, `plan`, `operations`) |
  | `crate::db::guild…`, `crate::db::GuildEntity` (and other crack-guild db items) | `crack_guild::db::…` |
  | `crate::music::perms::roles_resolvable` or `crate::…::perms` from crack-common | `crack_common::perms::…` |
  | `sqlx::migrate!("./test_migrations")` | `sqlx::migrate!("../crack-core/test_migrations")` |

  `crate::music::…`, `crate::sources::…`, `crate::http_utils::…`, `crate::db::<moved table>…` and `super::…` stay.
- **Visibility.** Items crack-core names that were `pub(crate)` become `pub`; the 4b-1 review lists them:
  - `query.rs`: `PlayAll`, `play_all_plan`, `spotify_query`, `unresolved_spotify_link`;
  - `queue.rs`: `build_track`, `enqueue`, `record_add`, `preload_time`, `new_track`, `play_beside_queue`, `preload_from_metadata`;
  - `track_data`'s items;
  - `ops`: `TRACK_INFO_TIMEOUT`, `pause_on`, `repeat_on`, `remove_on`;
  - `remote::playback_flags`;
  - `connected::connected_call`;
  - `search::gated`.
  
  Widen any other item the compiler names the same way, and list every one in the report. Don't move code to avoid widening.
- **Test fixtures cross the crate line through a feature,** as crack-common's `test-support` does.
  - crack-music declares `test-support = []`. Its fixtures (`music/ops/test_support.rs`, and any other `#[cfg(test)]` module crack-core's tests import) become `#[cfg(any(test, feature = "test-support"))]`, and their items `pub`.
  - crack-core's dev-dependencies gain `crack-music = { path = "../crack-music", features = ["test-support"] }`.
- **`production_source()`** (the `SURFACE` tests, in `queue_logged.rs`) reads `../crack-music/src/music/queue.rs` and `src/music/queue_logged.rs`, both relative to crack-core's manifest dir. `every_queue_entry_point_is_in_one_of_the_two_files` is the guard.
- **Intra-doc links** that spell `crate::music::MusicData` still resolve inside crack-music (`crate::music` is a module there too), so they stay.
- **Features:**
  - crack-music: `db-tests = []`, `mutation-testing = ["db-tests"]`, `test-support = []`;
  - crack-core: `db-tests` gains `"crack-music/db-tests"`.
- **One PR, v0.44.0,** a minor bump by the controller after Task 2.

## Review Focus

1. **Every refresh still happens.** The settle path crosses the crate line: crack-music's `Settle::after` calls the `Surface` that crack-core's `Data::music_data()` supplies. Pinned by the crack-core tests that assert what a settle rendered, and by crack-music's `settle_goes_through_the_surface`.
2. **History guarantees survive.** The four `SURFACE` tests now read a file in another crate. If the path were wrong, they'd fail loudly (not pass vacuously), and `every_queue_entry_point_is_in_one_of_the_two_files` pins that.
3. **The player's database tests run in CI, not compile and skip,** and the offline Docker build compiles crack-music's `query!` macros. Checked by conserved counts and `SQLX_OFFLINE=true` in Task 2.
4. **`cargo test -p crack-music` builds no crack-core.** Checked in Task 2.
5. **The dashboard still works.** crack-web reaches `remote`, `search` and `audit_view` through crack-core's re-exports. Checked by crack-web compiling and its tests passing.

---

### Task 1: Move into `crack-music`

**Files:**
- Create: `crack-music/Cargo.toml`, `crack-music/src/lib.rs`, `crack-music/src/db/mod.rs`, and a new `crack-core/src/music/mod.rs`
- Move (`git mv`): the moving set above
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml`, `crack-core/src/lib.rs`, `crack-core/src/db/mod.rs`, the glue files in `crack-core/src/music/` (imports only), `crack-music/src/music/{mod,queue,query}.rs` (drop the interim re-exports), and `crack-core/src/music/queue_logged.rs` (`production_source()`)

**Interfaces:**
- Produces:
  - crack-music's `music`, `sources`, `http_utils` and `db` modules;
  - crack-core's shims (exactly as in "Calls");
  - the `test-support` feature.

- [ ] **Step 1:** Record the test count and the passed/skipped line.
- [ ] **Step 2:** Create the crate, move the files, write both `mod.rs` files and the shims, apply the path table, and widen visibility. `cargo check --workspace --all-features` and `cargo check --workspace --all-features --tests` compile.
- [ ] **Step 3:** Run the suite with `FEATURES`, the doctests and the lint. All green, and counts are conserved. The `SURFACE` tests must run and pass, not skip.
- [ ] **Step 4: Commit:** `refactor: crack-music — the music player, its sources and its tables leave crack-core`

### Task 2: CI, docs and the boundary proof

**Files:**
- Modify: `.github/workflows/{lint,build,coverage}.yml`, `docs/testing.md` (feature lists, the `#[db_test]` crate list and per-crate counts), `scripts/sqlx_sync.py` (build crack-music's tests, like crack-guild's line), `.cargo/mutants.toml` (the crate-list comment), and `crack-music/src/music/ops/test_support.rs` (below)

**Interfaces:** none new.

- [ ] **Step 1: Prove the boundary.** Each of these prints nothing:
  - `cargo tree -p crack-music -e normal --prefix none | grep -E '^(poise|crack-core|crack-economy|crack-rooms) '`
  - `cargo tree -p crack-music -e normal,dev --prefix none | grep -E '^crack-core '`
- [ ] **Step 2: Prove the offline build:** `env -u DATABASE_URL SQLX_OFFLINE=true cargo check -p crack-music --tests --features db-tests,test-support` compiles.
- [ ] **Step 3: CI.**
  - `lint.yml` gets "crack-music depends on no poise or crack-core" and "crack-music has no crack-core dev-dependency", modelled on crack-guild's two steps.
  - `build.yml` (all four lists) and `coverage.yml` get `crack-music/db-tests`.
  - Update `docs/testing.md`, `scripts/sqlx_sync.py` and the mutants comment.
- [ ] **Step 4: The 4b-1 review's leftover minor.** `RecordingSurface.echoes` is written and never read. Remove the field and its writes; `echo` keeps returning `None`. Test-only.
- [ ] **Step 5:** Run the suite, the doctests, the lint, and `cargo test -p crack-music --features db-tests`. Confirm that last command compiled no crack-core (`grep -c 'Compiling crack-core'` on its output is 0). Counts are conserved.
- [ ] **Step 6: Commit:** `ci: crack-music in the boundary checks, the db-test feature lists and the docs`
