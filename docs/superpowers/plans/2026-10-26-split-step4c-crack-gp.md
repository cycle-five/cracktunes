# Split step 4c: crack-gp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the /gp game out of crack-core into a new crate, `crack-gp`, with no change to what the bot does. The game is its state machine, its cards, its prompt deck, its saved form and writer, and its table. Its Discord delivery (commands, playback) and boot-time resume stay in crack-core.

**Architecture:**
- **Task 1:** a `GpData` handle, as in steps 2–4b. The game's methods move onto it, and `Data` delegates.
- **Task 2:** split the two mixed files: `gp_persist.rs`'s resume goes to a crack-core `gp_resume.rs`, and `gp/test.rs`'s delivery tests go to a crack-core test file.
- **Task 3:** `git mv` the rest into `crack-gp`, with shims, and prove the cut by compiling it.
- **Task 4:** CI, docs and the boundary proof.

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise, songbird, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 4, "4c: crack-gp, the game", and rules 1–9).

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same /gp messages, cards, buttons, timers, votes, scores and saved games;
  - the same commands;
  - the same log lines (text and level). Log targets move to `crack_gp::…`; nothing filters on them;
  - no storage, schema, migration, `.sqlx/` or SQL text change.
- **Function bodies don't change,** except where a task names it. Only signatures, `use` lines and paths, visibility, manifests, test set-up and CI/docs change.
- **crack-gp never depends on poise or crack-core,** not even as a dev-dependency.
  - It may depend on crack-common, crack-model, crack-types, crack-guild, crack-music, crack-testing, serenity, songbird and the external crates the moved files already use.
  - It does not depend on crack-economy or crack-rooms.
- **Dependency versions:** copy lines from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains crack-gp and no second version of anything.
- **Paths keep working** through crack-core re-exports.
- **Typed serde only.** No local mutants runs (`--list` is fine).
- **Memory cap on every local cargo, nextest or clippy run:** `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`, written out each time; zsh won't word-split a variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`, with `SQLX_OFFLINE=true` alongside `DATABASE_URL`.
- **Verification commands** (under the cap, from `/home/lothrop/projects/cracktunes-split`). `FEATURES` is literal text to paste:
  - Tasks 1–2: `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests`
  - Tasks 3–4 add `,crack-gp/db-tests`.
  - suite: `cargo nextest run --workspace --features FEATURES`
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `cargo clippy -p crack-economy --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Test counts are conserved:** record before and after. Tests move; none disappear; the skipped count stays.
- **Commits:**
  - never `git add -A`; use `git mv` for whole-file moves; run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.

## What moves and what stays

From `crack-core/src/`:
- **Moves to crack-gp:**
  - `commands/music/gp/state.rs`, `commands/music/gp/ui.rs`, `commands/music/gp/test.rs` (after Task 2 has taken its delivery tests out);
  - `commands/music/gp_persist.rs` (after Task 2 has taken resume out);
  - `commands/music/gp_prompts.rs` and `commands/music/gp_prompts.json`;
  - `db/gp.rs`.
- **Stays (glue):**
  - `commands/music/gp/mod.rs` (rewritten as a shim in Task 3), `commands/music/gp/commands.rs`, `commands/music/gp/playback.rs` and `commands/music/gp/test_delivery.rs`;
  - the new `commands/music/gp/data_impl.rs` (Task 1), `commands/music/gp/test_glue.rs` (Task 2) and `commands/music/gp_resume.rs` (Task 2).

## Calls this plan makes against the spec

- **`GpData`** (Task 1, in `commands/music/gp/state.rs`, so it moves with the game):
  ```rust
  #[derive(Clone)]
  pub struct GpData {
      pub gp_games: Arc<dashmap::DashMap<GuildId, GpGame>>,
      pub gp_persist: Option<tokio::sync::mpsc::UnboundedSender<crate::commands::music::gp_persist::GpPersist>>,
      pub music: crate::music::MusicData,
  }
  ```
  - `DataInner.gp_games` becomes `Arc<DashMap<…>>`.
  - `Data::gp_data(&self) -> GpData` sits in `lib.rs` next to `music_data()`. It clones `gp_games` (the `Arc`) and `gp_persist` (the sender), and sets `music: self.music_data()`.
  - It has a manual `Debug` and no `Default`.
- **Methods that move onto `GpData`, bodies verbatim:** every method in `state.rs`'s `impl Data` block, and `gp_persist.rs`'s `gp_persist_send`, `gp_snapshot`, `gp_mark_finished` and `gp_shutdown`.
  - `GpData` gets two private methods, `claim_playback(&self, guild_id, owner) -> Result<(), CrackedError>` and `release_playback(&self, guild_id)`, each one line calling `self.music`. That keeps the game's own calls verbatim.
  - `Data` keeps every public and `pub(crate)` game method as a one-line delegation (`self.gp_data().<same>(<same args>)`) in a new `commands/music/gp/data_impl.rs`, with unchanged signatures. Private ones need none.
- **Resume splits out** (Task 2): `gp_resume_guild`, `abandon_resume`, `announce` and any helper only they use move from `gp_persist.rs` to a new `commands/music/gp_resume.rs`. `gp_persist.rs` re-exports `gp_resume_guild` (`pub use super::gp_resume::gp_resume_guild;`) until Task 3, so its callers (`config.rs`, `handlers/serenity.rs`, `lib.rs`, `music/resume.rs`) don't change.
- **`gp/test.rs` splits** (Task 2).
  - Tests that call `playback`'s functions (`gp_played`, `gp_played_at_end`…) or `commands`' (`refuse_gp`…), or that need the courier, move to a new `gp/test_glue.rs` (`#[cfg(test)] mod test_glue;`), keeping their names and assertions.
  - The rest stay in `test.rs`. Where they build `Data`/`DataInner` only to reach game state, they build a `GpData { gp_games: Arc::default(), gp_persist: None, music: test_music_data() }` (crack-music's `test-support` fixture, which crack-core already has as a dev-dependency).
- **crack-gp's layout** (Task 3):
  - `crack-gp/src/lib.rs` declares `pub mod db; pub mod gp; pub mod gp_persist; pub mod gp_prompts;`.
  - `crack-gp/src/gp/mod.rs` holds `mod state; mod ui; pub use state::*; pub use ui::*; #[cfg(test)] mod test;`.
  - `crack-gp/src/db/mod.rs` holds `pub mod gp; pub use gp::*;`.
  - `gp_prompts.json` sits next to `gp_prompts.rs`, so its `include_str!` keeps working.
- **Path rewrites in the moved files:**

  | From | To |
  |---|---|
  | `crate::commands::music::gp::…` | `crate::gp::…` |
  | `crate::commands::music::{gp_persist, gp_prompts}::…` | `crate::{gp_persist, gp_prompts}::…` |
  | `crate::music::…`, `crate::sources::…`, `crate::http_utils::…` | `crack_music::music::…`, `crack_music::sources::…`, `crack_music::http_utils::…` |
  | `crate::errors::…`, `crate::CrackedError`, `crate::CrackedResult`, `crate::Error` | `crack_common::…` (as in 4b-2) |
  | `crate::messaging::{messages, message, format, render, cards, failure, transport, discord, …}` | `crack_common::<same>` |
  | `crate::guild::…`, `crate::db::guild…` | `crack_guild::…` |
  | `crate::db::gp…` | `crate::db::gp…` (stays) |
  | `sqlx::migrate!("./test_migrations")` | `sqlx::migrate!("../crack-core/test_migrations")` |

- **crack-core's shims** (Task 3):
  - `commands/music/gp/mod.rs` keeps `mod commands; mod playback; mod data_impl;`, their `pub use`s, and `#[cfg(test)] mod test_delivery; #[cfg(test)] mod test_glue;`. It adds `pub use crack_gp::gp::*;`.
  - `commands/music/mod.rs`: `pub mod gp_persist;` becomes `pub mod gp_persist { pub use crack_gp::gp_persist::*; pub use super::gp_resume::gp_resume_guild; }`, and `pub mod gp_prompts;` becomes `pub use crack_gp::gp_prompts;`.
  - `db/mod.rs`: `pub mod gp;` becomes `pub use crack_gp::db::gp;`, and `pub use gp::*;` stays.
- **Visibility:** widen to `pub` every crack-gp item that crack-core names, and list each one.
- **Features:**
  - crack-gp: `db-tests = []`, `mutation-testing = ["db-tests"]`, and a `test-support` feature **only if** crack-core's glue tests need a crack-gp fixture. If one is added, add its `.cargo/mutants.toml` exclusion in the same commit (4b-2's review lesson).
  - crack-core: `db-tests` gains `"crack-gp/db-tests"`.
- **One PR, v0.45.0,** bumped by the controller after Task 4.

## Review Focus

1. **A running game keeps working end to end:** submissions, the reveal, votes, skip and full plays, and the end. Every game method reached through `Data` must reach the same `gp_games` map the handle writes. Pinned by Task 1's `gp_data_shares_the_games_map_and_the_lease` (`Arc::ptr_eq`, and a game started through `Data` is visible through the handle) and by the state-machine tests that move.
2. **A game still blocks the dashboard and the commands.** `claim_playback`/`release_playback` go through `self.music`, the shared lease. Pinned by the existing tests that assert a game holds playback.
3. **Saved games still save and resume.** The writer and the saved form move; resume stays. Pinned by `gp_persist`'s and `db/gp.rs`'s tests (they move) and by resume's tests (they stay).
4. **No test of moved code is left in crack-core** (4b-2's lesson). Task 3 greps crack-core's test modules for every moved function's name.
5. **/gp's database tests run in CI, not compile and skip.** Checked by conserved counts and `SQLX_OFFLINE=true` in Task 4.

---

### Task 1: The `GpData` handle; the game's methods move onto it; `Data` delegates

**Files:**
- Modify: `crack-core/src/commands/music/gp/state.rs` (`GpData`, its `Debug`, the `impl GpData`), `crack-core/src/commands/music/gp_persist.rs` (its four methods), `crack-core/src/lib.rs` (`gp_games` becomes an `Arc`, `Data::gp_data`, its test)
- Create: `crack-core/src/commands/music/gp/data_impl.rs` (`mod data_impl;` in `gp/mod.rs`)

**Interfaces:**
- Produces: `GpData` (exactly as in "Calls"), `Data::gp_data(&self) -> GpData`, `impl GpData { <every game method> }`, and `Data` delegations with unchanged signatures.

- [ ] **Step 1: Write the failing test** in `lib.rs`'s `lib_test`, `gp_data_shares_the_games_map_and_the_lease`:
  - `Arc::ptr_eq(&data.gp_data().gp_games, &data.gp_games)`;
  - `Arc::ptr_eq(&data.gp_data().music.playback_owners, &data.playback_owners)`;
  - after `data.gp_start(…)` (use the arguments an existing `gp_start` test uses), `data.gp_data().gp_is_active(guild)` is true;
  - after `data.gp_remove(guild)`, it is false and the lease is free again (`data.playback_owner(guild) == PlaybackOwner::Free`).
  
  Run it: it fails to compile.
- [ ] **Step 2:** Make `gp_games` an `Arc`, define `GpData` and `Data::gp_data`, move the methods onto `GpData`, add the two private lease methods, and write the delegations. Run the test: PASS.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green; the count rises by one.
- [ ] **Step 4: Commit:** `refactor(gp): a GpData handle; the game's methods move onto it, and Data delegates`

### Task 2: Resume and the delivery tests split out

**Files:**
- Create: `crack-core/src/commands/music/gp_resume.rs` (`pub mod gp_resume;` in `commands/music/mod.rs`) and `crack-core/src/commands/music/gp/test_glue.rs`
- Modify: `crack-core/src/commands/music/gp_persist.rs`, `crack-core/src/commands/music/gp/test.rs` and `crack-core/src/commands/music/gp/mod.rs`

**Interfaces:**
- Produces: `crate::commands::music::gp_resume::gp_resume_guild`, re-exported at `crate::commands::music::gp_persist::gp_resume_guild`.

- [ ] **Step 1:** Move resume (as in "Calls") and the delivery tests. Convert the remaining `test.rs` tests that build `Data` to `GpData`, as in "Calls". No test may be deleted; if one can't be placed without changing its assertions, stop and report it.
- [ ] **Step 2: Check the files that will move.** In the "Moves to crack-gp" list, nothing may name `Data`, `DataInner`, poise, `commands::music::gp::{commands, playback, data_impl, test_glue}`, `gp_resume`, `crate::commands::music_utils`, `crate::handlers`, `crate::poise_ext`, or the courier's `post`/`Destination`/`reply`. Check with a grep that sees nested `use crate::{…}` groups: match the module name at any path-segment start, not only after `crate::`. Every hit must be a plain comment. Task 3's compile is the final proof.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green, counts conserved.
- [ ] **Step 4: Commit:** `refactor(gp): resume and the delivery tests split from the game`

### Task 3: Move into `crack-gp`

**Files:**
- Create: `crack-gp/Cargo.toml`, `crack-gp/src/lib.rs`, `crack-gp/src/gp/mod.rs` and `crack-gp/src/db/mod.rs`
- Move (`git mv`): the "Moves to crack-gp" files, to `crack-gp/src/gp/{state,ui,test}.rs`, `crack-gp/src/{gp_persist,gp_prompts}.rs`, `crack-gp/src/gp_prompts.json` and `crack-gp/src/db/gp.rs`
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml`, `crack-core/src/commands/music/{mod.rs, gp/mod.rs}`, `crack-core/src/db/mod.rs`, `crack-core/src/commands/music/gp_persist.rs`'s interim re-export (now in the shim), and `clippy.toml` if any entry names a moved path

**Interfaces:**
- Produces: crack-gp's `gp`, `gp_persist`, `gp_prompts` and `db` modules, and crack-core's shims (as in "Calls").

- [ ] **Step 1:** Create the crate, move the files, apply the path table, write the shims, and widen visibility. `cargo check --workspace --all-features` and `--tests` compile. This compile is the cut's proof.
- [ ] **Step 2: Tests left behind** (4b-2's lesson). For every non-test function that moved, grep crack-core's `#[cfg(test)]` modules for its name. A crack-core test whose subject is a moved function moves to crack-gp with it, unless it needs crack-core (then say why it stays).
- [ ] **Step 3:** Run the suite with Task 3's `FEATURES`, the doctests and the lint. All green, counts conserved.
- [ ] **Step 4: Commit:** `refactor: crack-gp — the /gp game, its saved form, its prompts and its table leave crack-core`

### Task 4: CI, docs and the boundary proof

**Files:**
- Modify: `.github/workflows/{lint,build,coverage}.yml`, `docs/testing.md` (feature lists, the `#[db_test]` crate list and counts), `scripts/sqlx_sync.py` and `.cargo/mutants.toml` (the crate-list comment)

- [ ] **Step 1: Prove the boundary.** Each of these prints nothing:
  - `cargo tree -p crack-gp -e normal --prefix none | grep -E '^(poise|crack-core|crack-economy|crack-rooms) '`
  - `cargo tree -p crack-gp -e normal,dev --prefix none | grep -E '^crack-core '`
- [ ] **Step 2: Prove the offline build:** `env -u DATABASE_URL SQLX_OFFLINE=true cargo check -p crack-gp --tests --features db-tests` compiles.
- [ ] **Step 3: CI.**
  - `lint.yml` gets two crack-gp boundary steps. Their names list all four forbidden crates, as crack-guild's do.
  - `build.yml` (all four lists) and `coverage.yml` get `crack-gp/db-tests`.
  - Update `docs/testing.md`, `scripts/sqlx_sync.py` and the mutants comment.
- [ ] **Step 4:** Run the suite, the doctests, the lint, and `cargo test -p crack-gp --features db-tests`. Its output has no `Compiling crack-core` line. Counts are conserved.
- [ ] **Step 5: Commit:** `ci: crack-gp in the boundary checks, the db-test feature lists and the docs`
