# Clippy gate and explicit re-exports Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make clippy the gate for import hygiene and lint the test code too, then replace every glob re-export (`pub use x::*`) with an explicit list. No behaviour change.

**Why (the owner, 2026-10-26):**
- "This also gives another reason why not to do glob imports, exports." In the split's step 4c, a glob re-export (`gp/mod.rs`'s `pub use playback::*`) hid a dependency that grep couldn't see.
- "Clippy is my prefered route, leverage that tool as much as we can and we now gate on it."

**Architecture:**
- **Task 1 (the gate):** one `[workspace.lints]` table that every member inherits, CI's clippy step checking all targets, and the 30 current hits fixed.
- **Tasks 2–3:** the 127 glob re-exports become explicit lists, crack-core first, then the other crates.

**Tech Stack:** Rust 2021 Cargo workspace (17 members), clippy, GitHub Actions.

## What clippy can and can't gate (tested on scratch crates, 2026-10-26)

| Lint | Kind | Catches | Can't catch |
|---|---|---|---|
| `clippy::wildcard_imports` | pedantic | `use x::*` (not `use super::*` in test modules, not preludes) | `pub use x::*` and `pub(crate) use x::*` re-exports, even with `warn-on-all-wildcard-imports` |
| `ambiguous_glob_reexports` | rustc, warn by default | two glob re-exports exporting the same name | a lone glob re-export |
| `hidden_glob_reexports` | rustc, warn by default | a private item shadowing a public glob re-export | — |

So the gate holds plain globs, and the two rustc lints make any re-export glob that does remain fail loudly rather than silently. Re-export globs themselves are converted once (Tasks 2–3), and review keeps them out afterwards. No source-scan test polices them: the owner's rule is enforce with types and lints, not source scans.

## Global Constraints

- **No behaviour change.** Only `use`/`pub use` lines, lint attributes, manifests, CI and test-code fixes change. Production function bodies don't change, except for a clippy fix in production code. Report each one with its lint.
- **A re-export list names exactly what callers use.** Remove the glob, then compile `--workspace --all-targets --all-features`, and add only the names the compiler asks for. Re-exporting a module by name (`pub use crack_music::music::remote;`) is explicit and fine.
- **No new `#[allow]`.** Fix the code. If a fix would change behaviour, use `#[expect(clippy::…, reason = "…")]` and report it. `scripts/check_waivers.py` must stay green.
- **Memory cap on every local cargo or clippy run:** `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`, written out; zsh won't word-split a variable. Use `SQLX_OFFLINE=true`.
- **Verification** (from `/home/lothrop/projects/cracktunes-split`):
  - `FEATURES` = `crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests,crack-gp/db-tests`
  - suite: `cargo nextest run --workspace --features FEATURES` (with `DATABASE_URL=postgresql://postgres:mysecretpassword@localhost:5432/postgres`). It expects 2393 passed and 22 skipped, unchanged.
  - doctests: `cargo test --doc --workspace --features FEATURES`
  - lint, after Task 1:
    - `cargo fmt --all -- --check` and `cargo +nightly fmt --all -- --check`
    - `cargo clippy --all --all-targets -- -D warnings`
    - `cargo clippy -p crack-core --features crack-coin --all-targets --locked -- -D warnings`
    - `cargo clippy -p crack-economy --features crack-coin --all-targets --locked -- -D warnings`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `python3 scripts/check_waivers.py`
- **Commits:**
  - never `git add -A`; run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.

## Calls

- **The lint table** (root `Cargo.toml`):
  ```toml
  [workspace.lints.clippy]
  all = { level = "deny", priority = -1 }
  needless_return = "allow"
  wildcard_imports = "deny"
  enum_glob_use = "deny"

  [workspace.lints.rust]
  ambiguous_glob_reexports = "deny"
  hidden_glob_reexports = "deny"
  ```
  Every one of the 17 members' `Cargo.toml` gains `[lints]` / `workspace = true`.
  - Why the table: CI's flags today (`-D clippy::all --allow clippy::needless_return`) live only in `lint.yml`, so a local `cargo clippy` is a different check. In the table, every `cargo clippy` is the CI check.
- **CI** (`.github/workflows/lint.yml`, "Run clippy"): `cargo clippy --all --all-targets -- -D warnings`. The `-D clippy::all --allow clippy::needless_return` flags go, since the table has them. `-D warnings` stays: it turns every warn-level lint into an error. The crack-coin clippy steps gain `--all-targets` too. Any doc that quotes the old command is updated.
- **`--all-targets`** brings test code under the gate. Its current hits (listed in Task 1) get fixed.
- **One PR, v0.45.1,** a patch bump with no behaviour change, done by the controller after Task 3. The owner merges it: the standing merge go covered the split steps only.

## Review Focus

1. **The gate really gates.** A plain `use x::*` added anywhere must fail CI. Pinned by Task 1's check: add one temporarily, see `cargo clippy --all --all-targets -- -D warnings` fail, remove it.
2. **Local clippy equals CI clippy.** A member missing `[lints] workspace = true` silently escapes the table. Pinned by Task 1's member count: 17 of 17.
3. **Re-export lists add nothing callers didn't use.** A list padded with unused names re-creates the glob's problem. Checked in review against the compiler-driven rule.
4. **crack-web and crack-cli still compile.** They reach crack-core's re-exports. Checked by the workspace build.

---

### Task 1: The lint table, CI on all targets, and today's hits

**Files:**
- Modify: root `Cargo.toml` (the table), each member's `Cargo.toml` (`[lints] workspace = true`), `.github/workflows/lint.yml`, and any doc that quotes the clippy command
- Modify the files with today's hits. A scratch run on 2026-10-26 found 30:
  - 19 `wildcard_imports`;
  - 5 `bool_assert_comparison` (literal-bool asserts);
  - 2 `field_reassign_with_default`;
  - 1 each of `type_complexity`, `question_mark`, a manual `Default`/`range_contains`-style, and `unnecessary_unwrap`.
  - The files: `crack-economy/src/{shop,preflight,theme,effects}.rs`, `crack-types/src/discord.rs`, `crack-rooms/src/{rooms/card,modlog/preflight,modlog/event}.rs`, `crack-music/src/music/ops/{skip,playback,end,edit,add}.rs`, `crack-gp/src/gp/{ui,test_support}.rs`, `crack-core/src/{music/queue_logged,music/query_modes,handlers/event_log,commands/music/vote,commands/music/gp/commands,commands/economy/admin}.rs` and `crack-common/src/message.rs`.
  - `enum_glob_use` may add more; fix those too.

- [ ] **Step 1:** Add the table and the 17 `[lints]` entries. Run `cargo clippy --all --all-targets -- -D warnings` and list every hit.
- [ ] **Step 2:** Fix each hit. For `wildcard_imports`, write the explicit list the lint suggests, trimmed to what's used.
- [ ] **Step 3:** Update `lint.yml` and the docs.
- [ ] **Step 4: Prove the gate.**
  - Temporarily add `use std::collections::*;` to some crate's source; `cargo clippy --all --all-targets -- -D warnings` must fail on `wildcard_imports`. Remove it.
  - Count the members with `[lints] workspace = true`: 17.
- [ ] **Step 5:** Run the suite, the doctests and every lint command. All green, counts unchanged.
- [ ] **Step 6: Commit:** `chore(clippy): one workspace lint table every crate inherits; CI lints all targets; wildcard imports denied`

### Task 2: crack-core's glob re-exports become explicit

**Files:** every `crack-core/src/**/*.rs` with a `pub use …::*;` or `pub(crate) use …::*;` line: about 95 lines, the most in `commands/music/mod.rs` (32), `db/mod.rs`, `commands/utility/mod.rs`, `commands/settings/{set,get}/mod.rs`, `commands/mod.rs`, `music/mod.rs`, `commands/music/gp/mod.rs`, `commands/admin/**/mod.rs`, `guild/mod.rs`, `economy/mod.rs`, `messaging/{courier,discord,test_support}.rs` and `metrics.rs`.

- [ ] **Step 1:** For each file, replace each glob re-export with an explicit `pub use x::{…};`, compiler-driven as the Global Constraints say. A shim of a feature crate (`pub use crack_music::music::*;` and the like) lists the modules and items crack-core, crack-web and crack-cli use.
- [ ] **Step 2:** `grep -rnE '^\s*pub(\([a-z]+\))? use .*::\*;' crack-core/src` prints nothing.
- [ ] **Step 3:** Run the suite, the doctests and every lint command. All green, counts unchanged.
- [ ] **Step 4: Commit:** `refactor(core): crack-core's glob re-exports become explicit lists`

### Task 3: The other crates' glob re-exports become explicit

**Files:** the remaining `pub use …::*;` lines in `crack-types/src/lib.rs` (5), `crack-music/src/music/{ops/mod,mod}.rs` and `crack-music/src/db/mod.rs`, `crack-gp/src/{gp/mod,db/mod}.rs`, `crack-guild/src/db/mod.rs`, `crack-testing/src/lib.rs`, `crack-model/src/lib.rs`, `crack-test-db/src/lib.rs`, and any others the grep finds.

- [ ] **Step 1:** Replace each, compiler-driven. Where a crate's own root re-export is its public API (e.g. `crack-model`'s `lib.rs`), list everything its dependents use.
- [ ] **Step 2:** `grep -rnE '^\s*pub(\([a-z]+\))? use .*::\*;' --include='*.rs' crack-*` prints nothing, workspace-wide.
- [ ] **Step 3:** Run the suite, the doctests and every lint command. All green, counts unchanged.
- [ ] **Step 4: Commit:** `refactor: the remaining glob re-exports become explicit lists`
