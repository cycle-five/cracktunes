# Split step 2: crack-economy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the economy (`economy/`), CrackCoin's glue (`coin/`) and their queries (`db/economy.rs`, `db/economy/coin.rs`) out of crack-core into a new crate, `crack-economy`, with no change to what the bot does.

**Architecture:** First, while everything still lives in crack-core, the economy's functions stop taking `Data` (Tasks 1–2), and the code that has to stay in crack-core splits out into glue files (Task 3). The template-database test runner moves to its own crate so both crates' db tests can use it (Task 4). Then `git mv` moves the files, and crack-core keeps `pub use` shims so every `crate::economy::…`, `crate::coin::…` and `crate::db::economy::…` path keeps working (Task 5).

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise/songbird, sqlx (offline data in the root `.sqlx/`), nextest, `crack-test-macros`' `#[db_test]`.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 2, rules 1–9).

## Global Constraints

- **No behaviour change** (spec rule 8):
  - the same messages, buttons and Discord actions, byte for byte;
  - the same slash commands registered, with the same options and choices;
  - no storage, schema, migration, `.sqlx/` or SQL text change (spec: the database is unchanged).
- **Function bodies don't change** in Tasks 1–2. Only signatures, call sites and test set-up change. A changed body line needs a stated reason in the report.
- **crack-economy never depends on poise or crack-core,** not even as a dev-dependency (spec rules 1, 4 and 9). It may depend on crack-common, crack-model, crack-types, serenity and the external crates the moved files already use (rule 6).
- **No `Data`, `DataInner` or poise `Context` in a moved file** (rule 2). serenity's `Context`, `Http` and `Cache` are allowed.
- **Dependency versions.** Copy each new dependency line exactly from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains the new packages and no second version of any crate.
- **Paths keep working.** crack-core re-exports what moves at its old path. Call sites elsewhere in crack-core, crack-web and crack-cli change only where this plan says so.
- **Strings stay in crack-common's `messages.rs`** (#537). No new `pub const` strings anywhere else.
- **Typed serde only.** Never `serde_json::json!`, and never index a `serde_json::Value`.
- **No local mutants runs** (owner, 2026-10-25). `cargo mutants --list` is allowed: it builds nothing.
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. Write paths in full, never through a shell variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap, from the worktree root `/home/lothrop/projects/cracktunes-split`). `FEATURES` below is literal text to paste, not a shell variable:
  - Tasks 1–3: `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - Task 4 adds `,crack-test-db/db-tests`; Task 5 adds `,crack-economy/db-tests,crack-economy/crack-coin`.
  - suite: `cargo nextest run --workspace --features FEATURES`
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Test counts are conserved.** Before your first change, record `cargo nextest list --workspace --features FEATURES 2>/dev/null | grep -c '    '` and the suite's passed/skipped line. Report both, before and after. A test may move crate; none may disappear, and the skipped count may not rise.
- **Waiting:** wait on long jobs in the foreground, with a blocking loop under the 10-minute Bash timeout. Never end the turn to wait for a notification.
- **Commits:**
  - never `git add -A`; add files by name, and use `git mv` for moves;
  - run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.

## Calls this plan makes against the spec

- **The handle is a narrow copy of `Data`'s two economy fields, under the same names.** Spec rule 2's call was "each registry holds a clone of the pool". Instead:
  ```rust
  #[derive(Debug, Clone, Default)]
  pub struct EconomyData {
      pub database_pool: Option<sqlx::PgPool>,
      pub economy: Arc<EconomyRegistry>,
  }
  ```
  - `DataInner.economy` becomes `Arc<EconomyRegistry>`, and `Data::economy_data(&self) -> EconomyData` clones the two handles.
  - Why this and not the pool inside the registry: `Data` sets its pool after the registry exists (`with_database_pool`), and ~40 tests build `DataInner { database_pool: Some(pool), ..Default::default() }`. A second copy of the pool could silently disagree with the first, and an economy that sees no pool is quietly off.
  - Same field names, so a converted function keeps its body word for word; only `data: &Data` becomes `data: &EconomyData`.
  - The spawned loops (earning tick, sweeper, coin, crowdfund, jail re-check) hold an `EconomyData` clone. The `Arc` keeps the registry they write the same one `Data` reads.
- **Glue stays in crack-core, in `crack-core/src/economy/`,** next to the shims (spec: "glue is preferred"):
  - `buttons.rs`, whole. It uses `commands::economy::shop` and `messaging::transport`.
  - `walk_on.rs`: theme *playback*, split from `theme.rs` (songbird, the queue, `gp_is_active`, rooms' settings).
  - `facts.rs`: `gather_facts` and `can_join`, split from `target.rs` (rooms' guard and overwrites).
  - Distinct file names, not same-name modules shadowing a glob, so a reader always knows which crate a path is in. Callers of the split-out functions change path: `economy::theme::on_voice_state` becomes `economy::walk_on::on_voice_state`, and `economy::target::gather_facts`/`Subject` become `economy::facts::…`.
- **Two pure permission helpers move down to crack-common's new `perms.rs`:** `rooms::card::perm_names` (with its helpers `permission_names` and `names_text`) and `music::perms::roles_resolvable`. Both stay re-exported where they were. That makes `preflight.rs` and `shop.rs` movable whole (spec step 3 allows exactly this move).
- **`/coinflip`'s choice type leaves the economy.** `economy::gamble::CoinSide` derives `poise::ChoiceParameter`. The command gets its own `CoinCall` enum in `commands/economy/coinflip.rs`, with the derive and the same variant names in the same order, converted with `From<CoinCall> for CoinSide`. `CoinSide` keeps everything else.
- **The template-database runner gets its own crate, `crack-test-db`** (spec: "has to move to where every crate can use it"). The file moves verbatim. Each crate that uses `#[db_test]` re-exports it as `crate::test_db`, which is the path the attribute expands to. So neither `crack-test-macros` nor any test changes.
- **One copy of the test schema.** Moved test modules load `sqlx::migrate!("../crack-core/test_migrations")` (spec rule 4).
- **`coin_build_is_true_here` stays in crack-core.** It tests crack-core's `COIN_BUILD`. It moves to crack-core's `lib.rs` tests under `#[cfg(feature = "crack-coin")]`.
- **One PR,** with the cuts first and the move last, as in step 1. The spec said the cut is "likely its own PR". One PR keeps the step to a single merge, and each commit still builds and passes on its own.
- **v0.40.0** is a minor bump. The controller does it inline after Task 5.

## Review Focus

1. **A bot with no database.** Every converted function must see the same `database_pool` its `Data` has. A handle built with `None` silently turns the economy off. Pinned in Task 1 by `economy_data_carries_the_pool_and_shares_the_registry`.
2. **State a spawned loop writes is the state the commands read.** The earning tick, sweeper, coin loop, crowdfund timer and jail re-check hold a handle. If the handle copied the registry, their writes would go nowhere. Pinned by the same test (`Arc::ptr_eq`).
3. **The economy's db tests still run in CI, not compile-and-skip.** They need `crack-economy/db-tests` and `crack-test-db/db-tests` in `build.yml`'s feature list. Checked by the conserved test and skip counts (Global Constraints).
4. **`/coinflip` registers the same choices.** `CoinCall` must list `Heads`, `Tails` in that order. Pinned in Task 3 by `coin_call_choices_are_unchanged`.
5. **The Docker build has no database.** crack-economy's `query!` macros must compile against the root `.sqlx/` offline. Checked in Task 5 with `SQLX_OFFLINE=true`.

---

### Task 1: The `EconomyData` handle; `mod.rs`, `earn`, `bounty`, `crowdfund` and `coin` stop taking `Data`

**Files:**
- Modify: `crack-core/src/economy/mod.rs` (define `EconomyData`; convert), `crack-core/src/lib.rs` (`DataInner.economy`, `Data::economy_data`, its test), `crack-core/src/economy/{earn,bounty,crowdfund}.rs`, `crack-core/src/coin/mod.rs`
- Modify call sites: wherever the compiler points, outside `economy/` and `coin/` (expect `commands/economy/*`, `handlers/*`, `main`/startup in `crack-cli` or `lib.rs`)

**Interfaces:**
- Produces: `crate::economy::EconomyData` (exactly the struct in "Calls"), `Data::economy_data(&self) -> EconomyData`, `DataInner.economy: Arc<EconomyRegistry>`.
- Functions converted here (`&Data` → `&EconomyData`, `Data` → `EconomyData`):
  - `economy`: `load_settings`, `record_process_start`, `save_settings`, `opt_out`, `protection`, `mod_grant`, `end_pranks_on`, `mod_cancel`, `mod_lift`
  - `economy::earn`: `on_message`, `start`, `tick_with`
  - `economy::bounty`: `place`
  - `economy::crowdfund`: `pledge`, `execute`, `start`
  - `coin`: `start`

- [ ] **Step 1: Write the failing test** in `crack-core/src/lib.rs`'s `lib_test` module:
  ```rust
  #[tokio::test]
  async fn economy_data_carries_the_pool_and_shares_the_registry() {
      let pool = sqlx::postgres::PgPoolOptions::new()
          .connect_lazy("postgres://localhost/unused")
          .unwrap();
      let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
      let handle = data.economy_data();
      assert!(handle.database_pool.is_some());
      assert!(Arc::ptr_eq(&handle.economy, &data.economy));
      assert!(Data::default().economy_data().database_pool.is_none());
  }
  ```
- [ ] **Step 2: Run it and see it fail to compile** (`economy_data` doesn't exist).
- [ ] **Step 3:** Define `EconomyData` in `economy/mod.rs`, change `DataInner.economy` to `Arc<EconomyRegistry>`, and add `Data::economy_data`. Run the test: PASS.
- [ ] **Step 4:** Convert the functions listed above. Keep their bodies; fix each call site to pass `&data.economy_data()` (or `data.economy_data()` where the function takes it by value).
  - Their tests build an `EconomyData { database_pool: Some(pool), ..Default::default() }` (or `EconomyData::default()`) where they built a `Data`.
  - After this task, `grep -nE '\bData\b|DataInner' crack-core/src/economy/{mod,earn,bounty,crowdfund}.rs crack-core/src/coin/mod.rs` prints nothing, tests included.
- [ ] **Step 5:** Run the suite, the doctests and the lint. All green, test counts conserved.
- [ ] **Step 6: Commit:** `refactor(economy): an EconomyData handle; settings, earning, bounties, crowdfunds and coin stop taking Data`

### Task 2: `effects`, `lock`, `shop` and theme purchase stop taking `Data`

**Files:**
- Modify: `crack-core/src/economy/{effects,lock,shop,theme}.rs`, and call sites where the compiler points

**Interfaces:**
- Consumes: `EconomyData`, `Data::economy_data` (Task 1).
- Functions converted here:
  - `economy::effects`: `end_running_pranks`, `finish`, `let_away_renames_stand`, `on_voice_join`, `reconcile`, `resolve_pending`, `start_sweeper`, `sweep`
  - `economy::lock`: `end_without_restore`, `jail_recheck`, `on_audit_entry`, `on_nick_change`, `on_voice_move`, `running`, `spawn_jail_recheck`
  - `economy::shop`: `buy`, `buy_immunity`, `charge_and_cleanse`, `cleanse`, `reveal`, `reveal_choices`
  - `economy::theme`: `buy_theme`, `claim_theme`
- **Not converted:** theme's playback functions (`on_voice_state`, `on_voice_join`, `play_on_join`, `join_for_theme`, `play`, `leave_if_idle`), `target::gather_facts`, and `buttons`' four. They keep `&Data`, because they read songbird, rooms or gp state. Task 3 moves them out of the way.

- [ ] **Step 1:** Convert the functions above, as in Task 1 Step 4.
- [ ] **Step 2:** Verify that `grep -nE '\bData\b|DataInner' crack-core/src/economy/{effects,lock,shop}.rs` prints nothing. In `theme.rs`, `Data` appears only in the playback functions and their tests.
- [ ] **Step 3:** Run the suite, the doctests and the lint. All green, test counts conserved.
- [ ] **Step 4: Commit:** `refactor(economy): pranks, the jail, the shop and theme purchase stop taking Data`

### Task 3: Split the glue out; permission helpers down; `/coinflip`'s choice type

**Files:**
- Create: `crack-core/src/economy/walk_on.rs` and `crack-core/src/economy/facts.rs`, from `theme.rs` and `target.rs`
- Create: `crack-common/src/perms.rs`. Modify `crack-common/src/lib.rs`, `crack-core/src/rooms/card.rs` and `crack-core/src/music/perms.rs`
- Modify: `crack-core/src/economy/{mod,theme,target,preflight,shop,gamble}.rs`, `crack-core/src/commands/economy/coinflip.rs`, `crack-core/src/coin/chain.rs`, `crack-core/src/lib.rs`, and callers where the compiler points (`handlers/serenity.rs`, `commands/economy/{bounty,crowdfund,shop}.rs`)

**Interfaces:**
- Produces:
  - `crate::economy::walk_on::on_voice_state`, with the rest of theme playback: `on_voice_join`, `channel_of`, `play_on_join`, `join_for_theme`, `play`, `post_card`, `leave_if_idle`, `stop_theme`, `ClipSignal`, plus any private helper only they use.
  - `crate::economy::facts::{gather_facts, can_join, Subject}`, plus any private helper only they use.
  - `crack_common::perms::{perm_names, roles_resolvable}` (and `permission_names`, `names_text` as `pub`). Re-exported at `rooms::card::perm_names` and `music::perms::roles_resolvable`.
  - `commands::economy::coinflip::CoinCall` with `impl From<CoinCall> for CoinSide`.
- Rule: an item that `walk_on.rs` or `facts.rs` needs from `theme.rs` or `target.rs` becomes `pub`, not `pub(crate)`, because it crosses a crate boundary in Task 5.

- [ ] **Step 1: Write the failing test** in `coinflip.rs`:
  ```rust
  #[test]
  fn coin_call_choices_are_unchanged() {
      let names: Vec<String> = <CoinCall as poise::ChoiceParameter>::list()
          .into_iter()
          .map(|c| c.name)
          .collect();
      assert_eq!(names, ["Heads", "Tails"]);
  }
  ```
  Run it: fails to compile.
- [ ] **Step 2:** Add `CoinCall` and the `From`, take `call: CoinCall` in the command and convert at its first use. Then remove `poise::ChoiceParameter` from `CoinSide`. Run the test: PASS.
- [ ] **Step 3:** Move `perm_names` (with `permission_names` and `names_text`) and `roles_resolvable` into `crack-common/src/perms.rs`, with their tests, and add `pub use` at the old paths.
- [ ] **Step 4:** Cut theme playback into `walk_on.rs` and `gather_facts`/`can_join`/`Subject` into `facts.rs`, each with the tests that exercise them. Declare both in `economy/mod.rs` and update the callers.
- [ ] **Step 5:** Move `coin_build_is_true_here` from `coin/chain.rs` into `lib.rs`'s `lib_test` module, under `#[cfg(feature = "crack-coin")]`.
- [ ] **Step 6: Verify the cut.** Over the files that move in Task 5, this prints nothing, tests included:
  ```
  grep -nE 'crate::(Data|DataInner|Error|music|rooms|commands|handlers|guild|http_utils|utils|connection|poise_ext|COIN_BUILD)\b|crate::messaging::(transport|courier|buttons|discord|test_support|interface|status|throttle)|\bpoise\b|\bData\b|DataInner' \
    crack-core/src/economy/{mod,bounty,cards,crowdfund,discord,earn,effects,gamble,immunity,lock,preflight,registry,settings,shop,target,theme,wallet}.rs \
    crack-core/src/coin/*.rs crack-core/src/db/economy.rs crack-core/src/db/economy/coin.rs
  ```
  The files that move are `economy/{mod,bounty,cards,crowdfund,discord,earn,effects,gamble,immunity,lock,preflight,registry,settings,shop,target,theme,wallet}.rs`, `coin/*.rs`, `db/economy.rs` and `db/economy/coin.rs`.
  A match inside a plain comment is fine (say so in the report). An intra-doc link (`` [`Data`] ``) is not, because it breaks once the file leaves crack-core: reword it.
- [ ] **Step 7:** Run the suite, the doctests and the lint. All green, test counts conserved.
- [ ] **Step 8: Commit:** `refactor(economy): theme playback and target facts split into glue; permission helpers to crack-common; /coinflip's own choice type`

### Task 4: `crack-test-db`, the template-database runner as a crate

**Files:**
- Create: `crack-test-db/Cargo.toml` and `crack-test-db/src/lib.rs`
- Move: `git mv crack-core/src/test_db.rs crack-test-db/src/test_db.rs`
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml` (dev-dependency), `crack-core/src/lib.rs` (`test_db` becomes a re-export), `crack-test-macros/src/lib.rs` (its "only crack-core can use it" doc paragraph), `.github/workflows/build.yml` (both feature lists), `docs/testing.md` where it names `crack-core/src/test_db.rs`

**Interfaces:**
- Produces: crate `crack-test-db` with `pub mod test_db;` and `pub use test_db::*;` in its `lib.rs`, so `crate::test_db::run` resolves inside it too. Each user crate declares `#[cfg(test)] pub(crate) use crack_test_db::test_db;`.
- Features: `db-tests = []` (gates its own tests, as in crack-core), and `mutation-testing = ["db-tests"]`.
- Dependencies: what `test_db.rs` imports (`futures`, `sqlx`, `tokio` as crack-core has them). Dev-dependencies: `crack-test-macros`, plus `crack-common` for the one test that returns a `CrackedError` (its path becomes `crack_common::errors::CrackedError`).
- Its own tests load `sqlx::migrate!("../crack-core/test_migrations")`.

- [ ] **Step 1:** Create the crate, move the file and add the re-exports.
- [ ] **Step 2:** Add `crack-test-db/db-tests` to both feature lists in `build.yml` (the `nextest run` and `test --doc` lines, both jobs).
- [ ] **Step 3:** Run the suite with the Task 4 `FEATURES`. The test_db tests now run under `crack-test-db`, with the same count.
- [ ] **Step 4:** Run the lint, and `cargo tree -p crack-test-db -e normal --prefix none | grep -E '^(poise|crack-core) '`, which prints nothing.
- [ ] **Step 5: Commit:** `test: the template-database runner moves to its own crate, crack-test-db`

### Task 5: Move into `crack-economy`

**Files:**
- Create: `crack-economy/Cargo.toml`, `crack-economy/src/db/mod.rs` (`pub mod economy;`), and a new `crack-core/src/economy/mod.rs`
- Move (`git mv`):
  - `crack-core/src/economy/mod.rs` → `crack-economy/src/lib.rs`
  - `crack-core/src/economy/{bounty,cards,crowdfund,discord,earn,effects,gamble,immunity,lock,preflight,registry,settings,shop,target,theme,wallet}.rs` → `crack-economy/src/`
  - `crack-core/src/coin/` → `crack-economy/src/coin/`
  - `crack-core/src/db/economy.rs` → `crack-economy/src/db/economy.rs`
  - `crack-core/src/db/economy/coin.rs` → `crack-economy/src/db/economy/coin.rs`
- Modify: root `Cargo.toml` (members), `crack-core/Cargo.toml`, `crack-core/src/lib.rs`, `crack-core/src/db/mod.rs`, `.github/workflows/{lint,build}.yml`

**Interfaces:**
- crack-economy's `lib.rs` is the old `economy/mod.rs`, with:
  - `buttons`, `walk_on` and `facts` removed from its module list;
  - `pub mod db;` and `#[cfg(feature = "crack-coin")] pub mod coin;` added;
  - `#[cfg(test)] pub(crate) use crack_test_db::test_db;`.
- crack-economy's features: `crack-coin = ["dep:crack-coin"]`, `db-tests = []`, `mutation-testing = ["db-tests", "crack-coin"]`.
- crack-economy's dependencies are what the moved files import. Its dev-dependencies are what their tests import, including `crack-test-db` and `crack-test-macros` (for `#[db_test]`).
- crack-core:
  - `crack-economy = { path = "../crack-economy" }`;
  - features `crack-coin` gains `"crack-economy/crack-coin"`, and `db-tests` gains `"crack-economy/db-tests"`;
  - `lib.rs`: `#[cfg(feature = "crack-coin")] pub use crack_economy::coin;`;
  - `db/mod.rs`: `pub use crack_economy::db::economy;`;
  - the new `economy/mod.rs`: a doc comment, `pub use crack_economy::*;`, and `pub mod buttons; pub mod facts; pub mod walk_on;`.
- Path rewrites, inside the moved files only:

  | From | To |
  |---|---|
  | `crate::economy::` | `crate::` |
  | `crate::messaging::{messages,format,render,cards,message,failure}` | `crack_common::…` (same module) |
  | `crate::errors` | `crack_common::errors` |
  | `crate::rooms::card::perm_names`, `crate::music::perms::roles_resolvable` | `crack_common::perms::…` |
  | `sqlx::migrate!("./test_migrations")` | `sqlx::migrate!("../crack-core/test_migrations")` |

  `crate::coin::…`, `crate::db::economy…` and `super::…` stay as they are.
- An item crack-core uses that was `pub(crate)` becomes `pub`.

- [ ] **Step 1:** Create the crate, move the files, apply the rewrites and write the shims. `cargo check --workspace --all-features` compiles.
- [ ] **Step 2:** Prove the boundary. Each of these prints nothing:
  - `cargo tree -p crack-economy -e normal --prefix none | grep -E '^(poise|crack-core) '`
  - `cargo tree -p crack-economy -e normal,dev --prefix none | grep -E '^crack-core '`
- [ ] **Step 3:** Prove the offline build: `SQLX_OFFLINE=true cargo check -p crack-economy --features crack-coin --tests` compiles. Run it with `DATABASE_URL` unset, as `env -u DATABASE_URL SQLX_OFFLINE=true …`.
- [ ] **Step 4: CI.**
  - `lint.yml`: add "crack-economy depends on no poise or crack-core" and "crack-economy has no crack-core dev-dependency", modelled on crack-common's two steps.
  - `lint.yml`: add `cargo clippy -p crack-economy --features crack-coin --locked -- -D warnings` beside crack-core's.
  - `build.yml`: add `crack-economy/db-tests,crack-economy/crack-coin` to all four feature lists.
- [ ] **Step 5:** Run the suite with the Task 5 `FEATURES`, the doctests, the lint and `cargo test -p crack-economy --features db-tests,crack-coin`. Then confirm that last command compiled no `crack-core` (`grep -c 'Compiling crack-core'` on its output is 0). Test counts are conserved.
- [ ] **Step 6: Commit:** `refactor: crack-economy — the economy, CrackCoin glue and their queries leave crack-core`
