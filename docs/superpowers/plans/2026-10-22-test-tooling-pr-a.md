# Test tooling, PR A: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove tarpaulin, move testing to nextest and coverage to cargo-llvm-cov, make the
database tests fast with template databases, and put cargo-mutants' configuration, full-run
workflow, baseline tooling and a target sweep in place. **There is no merge gate yet.** That is
PR B, which comes after the first full run.

**Architecture:**
- **Template databases.** A proc-macro attribute in a new workspace crate,
  `crack-test-macros`, replaces `#[sqlx::test(migrator = "MIGRATOR")]` across crack-core. It
  expands to a plain `#[test]` that calls a runner in crack-core's test support. The runner
  clones each test's database from a once-migrated template.
- **cargo-mutants** runs through one wrapper script. The wrapper sweeps stale build state, keeps
  scratch copies on disk, and gives each job a private target directory. A dispatch-only GitHub
  workflow shards a full run and collects one list of missed mutants.
- **Baseline and sweep logic** is small, unit-tested Python in `scripts/`.

**Tech Stack:**
- Rust 1.99 stable, sqlx 0.9 (Postgres 16), tokio;
- `syn` 2, `quote` 1 and `proc-macro2` 1, all already in `Cargo.lock`;
- cargo-nextest 0.9.149, cargo-mutants 27.1.0, cargo-llvm-cov;
- GitHub Actions;
- Python 3 standard library only, with `unittest`.

**Spec:** `docs/superpowers/specs/2026-10-22-test-tooling-design.md`. Read it. This plan covers
its "PR A" list.

## Global Constraints

**Git and the repo**
- Never `git add -A` or `git add .`. Add files by name.
- Every commit ends with exactly this trailer line, and no other `Co-Authored-By` line:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- No version bump. This PR ships no bot behaviour, so `Cargo.toml`'s `version` stays where it is.
- The release profiles (`release`, `release-with-debug`, `release-with-performance`, `dist`)
  are not touched.

**Build directories**
- **Never delete, `cargo clean`, or write a script that deletes the shared target directory.**
  - It is `build.target-dir = "/home/lothrop/.cache/cargo-target"` in `~/.cargo/config.toml`.
  - Other projects build into it, possibly at that very moment.
  - The one thing allowed is deleting stale incremental directories under it (Task 7).
- **Never pass `--in-place` to cargo-mutants on this machine.** Another agent may be editing the
  same worktree, and in-place runs mutate its files. CI may run in place.
- **cargo-mutants scratch copies live under `$HOME/.cache/cargo-mutants`,** never `/tmp`.
  `/tmp` is a 16 GB tmpfs in RAM.

**Docker and the database**
- Never build Docker images locally.
- Bare `docker` targets a remote host. For the local Postgres, use `docker --context default …`.
- **The local test database** is the container `pg`, Postgres 16, on `localhost:5432`.
  - The tests default to `postgresql://postgres:mysecretpassword@localhost:5432/postgres`, set
    by the ctor in `crack-core/src/db/user.rs`.
  - Database tests need `--features crack-core/db-tests` (plus `cracktunes/db-tests` and
    `crack-voting/db-tests` for a workspace run). CI also passes `crack-core/crack-coin`.

**SQL and Rust**
- Don't change any SQL query. If one changed, `.sqlx/` would have to be regenerated with
  `python3 scripts/sqlx_sync.py`, never with `cargo sqlx prepare` by hand.
- Rust that builds wire data uses typed serde, never `serde_json::json!` or `Value`.

**Workflows**
- Declare least privilege: `permissions: contents: read`.
- Pin Postgres to 16: `postgres-version: "16"` on `ikalnytskyi/action-setup-postgres@v8`.
- Reuse the actions the repo already uses: `actions/checkout@v7`, `Swatinem/rust-cache@v2`.
- Install prebuilt tools with `taiki-e/install-action@v2` at pinned versions:
  `cargo-nextest@0.9.149` and `cargo-mutants@27.1.0`.

**Style**
- Match the repo's comment style. Workflow and config files explain *why* in full sentences,
  with 🔑 for a key reason and 🪤 for a trap.

**The `mutants` nextest profile**
- cargo-mutants passes its own arguments to nextest, so tell it which nextest profile to use
  through the environment: `NEXTEST_PROFILE=mutants`. A command-line flag could clash with
  cargo-mutants' own `--profile` (its cargo profile).

## Review Focus

These are the failure modes most likely to bite, and the test that pins each one.
1. **Two nextest processes need a template that doesn't exist yet.**
   - Expected: exactly one builds it, and the other waits and then uses it. Neither may see a
     half-migrated template.
   - Pinned by `two_runners_racing_build_one_template` (Task 4).
2. **A test database left behind by a failed run.**
   - Expected: the next run of that test drops it and starts clean.
   - Pinned by `a_leftover_test_database_is_replaced` (Task 4).
3. **Line numbers shift between runs, and two identical mutant descriptions sit in one file.**
   - Expected: no false "new miss", and duplicates are counted, not merged.
   - Pinned by `test_shifted_lines_are_not_new` and `test_duplicates_count_twice` (Task 6).
4. **The sweep is pointed at the wrong directory,** or the directory holds fresh incremental
   state.
   - Expected: it refuses anything without a `CACHEDIR.TAG`, and it touches only stale
     `incremental/` children.
   - Pinned by `test_refuses_a_directory_without_cachedir_tag` and
     `test_keeps_recent_and_non_incremental` (Task 7).
5. **Parallel mutant jobs build into the shared target directory and wait on its lock.**
   - Expected: each job builds in its own copy's `target/`.
   - Verified in Task 8, step "private target dirs".

---

### Task 1: Tarpaulin out

**Files:**
- Modify:
  - every `.rs` file containing `#[cfg(not(tarpaulin_include))]` (329 lines in 119 files), plus
    the commented copy `// #[cfg(not(tarpaulin_include))]`;
  - `build.rs` and `crack-cli/build.rs`;
  - `crack-core/Cargo.toml`, `crack-types/Cargo.toml`, `crack-testing/Cargo.toml`,
    `crack-voting/Cargo.toml` and `crack-cli/Cargo.toml`;
  - `.build.yml`, `scripts/lint_test_build.sh`, `scripts/lint_test_build.fish` and
    `.github/dependabot.yml`.

**Interfaces:**
- Produces: a tree where `git grep -i tarpaulin` matches only:
  - `.github/workflows/coverage.yml` (Task 3);
  - `README.md` (Task 10);
  - the spec and plan docs under `docs/superpowers/`, which are history and stay.

- [ ] **Step 1: Record the baseline.** Run `cargo check --workspace --all-targets 2>&1 | grep -c '^warning'`
  and note the count. The result must not increase.

- [ ] **Step 2: Delete the attributes.**
  - Remove each line that is exactly `#[cfg(not(tarpaulin_include))]`, ignoring leading
    whitespace, and the one `// #[cfg(not(tarpaulin_include))]`.
  - Use one scripted pass over `git grep -l tarpaulin_include -- '*.rs'`, and
    `git diff --stat` afterwards.
  - Check that `git diff` shows only `-` lines, and that the number deleted is 330.

- [ ] **Step 3: Delete the build and lint configuration.**
  - In both `build.rs` files, delete the `cargo:rustc-check-cfg=cfg(tarpaulin_include)` line,
    and in the root file its comment, "make sure tarpaulin is included in the build".
  - In each of the 5 `Cargo.toml` files, delete the `unexpected_cfgs` line, and the
    `[lints.rust]` header if the table is then empty.

- [ ] **Step 4: Delete the script and CI references.**
  - In `.build.yml`, delete `cargo install cargo-tarpaulin` and the commented tarpaulin line.
  - In both `lint_test_build` scripts:
    - delete the tarpaulin line;
    - change the test line to `cargo nextest run --workspace` (keep the script's `--profile`
      use as it is: nextest takes `--cargo-profile`).
  - In `.github/dependabot.yml`, delete the `codecov/codecov-action` ignore entry and its
    comment.

- [ ] **Step 5: Verify.**
  - `cargo check --workspace --all-targets` compiles, with no more warnings than step 1.
  - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return` passes.
  - `git grep -il tarpaulin` lists only the files named under Interfaces.

- [ ] **Step 6: Commit** with the message `chore: remove tarpaulin from the tree`. Every change
  is a modification of a tracked file, so `git add -u` is the right command here. It never
  stages untracked files.

### Task 2: nextest as the runner, and the stdout ctor

**Files:**
- Create: `.config/nextest.toml`.
- Modify:
  - `crack-core/src/guild/permissions.rs:543-550`: delete the ctor;
  - `.github/workflows/build.yml`: the two "Run … Unit Tests" steps.

**Interfaces:**
- Produces:
  - nextest profiles `default`, `ci` and `mutants`;
  - CI runs `cargo nextest run --profile ci`;
  - Task 8 uses the `mutants` profile through `NEXTEST_PROFILE=mutants`.

- [ ] **Step 1: Show the failure.** With `DATABASE_URL` unset,
  `cargo nextest list -p crack-core` fails while parsing the list. The cause is the line
  `WARNING: DATABASE_URL not set for tests` on stdout. Save the error text for the report.

- [ ] **Step 2: Delete the ctor.** Delete the whole `#[ctor::ctor(unsafe)] fn set_env()` block
  in `guild/permissions.rs`'s test module. Leave the other ctors alone (`db/user.rs`,
  `commands/music/vote.rs`, crack-cli's `test/playlist.rs`), and keep the `ctor` dependency.

- [ ] **Step 3: Run the list again.** With `DATABASE_URL` unset, the same command now succeeds.

- [ ] **Step 4: Write `.config/nextest.toml`.**
  - `[profile.default]`: `slow-timeout = { period = "30s", terminate-after = 4 }`, and no
    retries.
  - `[profile.ci]`: `fail-fast = false`, `failure-output = "immediate-final"`.
  - `[profile.mutants]`: `fail-fast = true`.
  - Each block gets a one-line *why*, matching the spec, section 2.

- [ ] **Step 5: Measure, and check the connection count.**
  - Run `cargo nextest run --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
    against local `pg`. Every test must pass.
  - Check that no test hits the slow timeout. A test that legitimately takes longer gets a
    `[[profile.default.overrides]]` entry for that test alone, with a comment saying why.
  - Grep the output for `too many clients` or `remaining connection slots`. If either appears,
    add a test group capping the crack-core database tests, and record its numbers in a
    comment.
  - Record the wall time and the test count.

- [ ] **Step 6: Switch CI.** In `build.yml`:
  - Install nextest with `taiki-e/install-action@v2`, `tool: cargo-nextest@0.9.149`, before
    the test steps.
  - Replace both `cargo test …` lines with `cargo nextest run --profile ci` plus the same
    `--workspace` and features, keeping `--release --locked` on the master/tag step.
  - Add `cargo test --doc --workspace` with the same features after each, because nextest
    doesn't run doctests. Add a comment saying so.
  - Keep the step names and `if:` conditions as they are.

- [ ] **Step 7: Verify locally.** `cargo test --doc --workspace` with the same features passes.

- [ ] **Step 8: Commit.** Commit `.config/nextest.toml`, `crack-core/src/guild/permissions.rs`
  and `.github/workflows/build.yml` with the message
  `test: run the suite under cargo-nextest; drop the stdout ctor that broke its listing`.

### Task 3: Coverage with cargo-llvm-cov, and the dev profile

**Files:**
- Modify:
  - `.github/workflows/coverage.yml`: a full rewrite;
  - `Cargo.toml` (root): add the dev profile settings.

**Interfaces:**
- Produces: a coverage workflow that Task 10's docs describe.

- [ ] **Step 1: Rewrite `coverage.yml`.**
  - **Triggers:** `workflow_dispatch`, plus `schedule` weekly.
  - **Toolchain:** stable, plus `rustup component add llvm-tools-preview`.
  - **Tools:** `taiki-e/install-action@v2` for `cargo-llvm-cov` at a pinned version (the
    current release) and `cargo-nextest@0.9.149`.
  - **Database:** the pinned Postgres 16 step as in `build.yml`, with `SQLX_OFFLINE=true` for
    the build.
  - **Run:**
    `cargo llvm-cov nextest --profile ci --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin --lcov --output-path lcov.info`
  - **Upload:** `codecov/codecov-action@v5`, with `files: lcov.info`,
    `token: ${{ secrets.CODECOV_TOKEN }}` and `fail_ci_if_error: false`.
  - **The header comment** replaces the tarpaulin history. It says why the job is weekly
    rather than on every push (it's informational, and the gate is cargo-mutants), and keeps
    the "PIN THIS" Postgres note.

- [ ] **Step 2: Check the coverage command locally.**
  - `cargo install cargo-llvm-cov --locked` if it's missing.
  - Run the same `cargo llvm-cov nextest` command against local `pg`. It writes `lcov.info`.
  - Print the line-coverage total for the report, then delete `lcov.info`.

- [ ] **Step 3: Add the dev profile to the root `Cargo.toml`, above `[profile.release]`.**

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

  Add a comment: backtraces keep their file and line numbers, debugger variables are gone, and
  a rebuild after an edit measured 18 s, then 12–13 s.

- [ ] **Step 4: Verify** that `cargo build -p crack-core --tests` and `cargo nextest run -p crack-core`
  succeed.

- [ ] **Step 5: Commit.** Commit `.github/workflows/coverage.yml` and `Cargo.toml` with the
  message `ci: coverage with cargo-llvm-cov; dev builds carry line tables only`.

### Task 4: The template-database runner and its attribute

**Files:**
- Create:
  - `crack-test-macros/Cargo.toml` and `crack-test-macros/src/lib.rs`;
  - `crack-core/src/test_db.rs`.
- Modify:
  - the root `Cargo.toml`: add `"crack-test-macros"` to `[workspace] members`;
  - `crack-core/Cargo.toml`: add the `[dev-dependencies]`
    `crack-test-macros = { path = "../crack-test-macros" }`;
  - `crack-core/src/lib.rs`: add `#[cfg(test)] pub(crate) mod test_db;`;
  - `Cargo.lock`.

**Interfaces:**
- **Produces, in `crack-test-macros`** (`proc-macro = true`, dependencies `syn = { version = "2", features = ["full"] }`,
  `quote = "1"`, `proc-macro2 = "1"`): the attribute `db_test(migrator = "PATH")`.
  - **Accepts:** `async fn NAME(PAT: TYPE) [-> RET] BODY`, with any other attributes.
  - **Emits:**

```rust
#[test]
<every other attribute, unchanged: #[cfg_attr(.., ignore = ..)], lint attributes, doc comments>
fn NAME() [-> RET] {
    async fn NAME(PAT: TYPE) [-> RET] BODY
    crate::test_db::run(&PATH, concat!(module_path!(), "::", stringify!(NAME)), NAME)
}
```

  Lint levels are inherited by nested items, so an `#[expect(..)]` on the outer fn covers the
  inner body. Nothing needs moving.

  - **Rejects** a non-`async` fn, or a fn without exactly one argument, with `compile_error!`.
  - The macro may only be used inside crack-core, because it calls `crate::test_db`. Say so in
    the crate's doc comment.
- **Produces, in `crack-core::test_db`** (`#[cfg(test)]`):
  - `pub(crate) trait Outcome { fn passed(&self) -> bool; }`, implemented for `()` (true) and
    `Result<T, E>` (`is_ok`);
  - `pub(crate) fn template_name(migrator: &sqlx::migrate::Migrator) -> String`: `crack_tpl_`
    plus 16 lowercase hex digits of an FNV-1a 64 hash over each migration's
    `version.to_le_bytes()` and `checksum` bytes, in order. Write FNV-1a inline. Don't use
    `DefaultHasher`, which isn't stable across Rust versions;
  - `pub(crate) fn test_db_name(test_path: &str) -> String`: `crack_test_` plus 16 hex digits of
    FNV-1a 64 over the path;
  - `pub(crate) async fn ensure_template(admin: &mut sqlx::PgConnection, migrator: &'static sqlx::migrate::Migrator) -> sqlx::Result<String>`;
  - `pub(crate) fn run<F, Fut>(migrator: &'static sqlx::migrate::Migrator, test_path: &str, test_fn: F) -> Fut::Output`
    where `F: FnOnce(sqlx::PgPool) -> Fut`, `Fut: Future`, and `Fut::Output: Outcome`.
  - `run` uses a `tokio` current-thread runtime with `enable_all`, which is what `#[sqlx::test]`
    uses.

**Behaviour that `run` and `ensure_template` must have** (spec, section 3):
- **Connections.**
  - The admin connection uses `DATABASE_URL`'s database.
  - The test pool is capped at 5 connections, with `idle_timeout` 1 s.
  - If `DATABASE_URL` is unset, panic with `"DATABASE_URL must be set for db-tests"`.
  - If the connection fails, panic with the URL's host and port (never the password) and the
    hint `"is postgres running?"`.
- **`ensure_template`:**
  1. Take `pg_advisory_lock(<a fixed i64, defined as a named const>)`.
  2. If the template exists in `pg_database`, unlock and return its name.
  3. Otherwise, drop any leftover `<name>_build`, then create `<name>_build`.
  4. Run `migrator.run` on a connection to it, and close that connection.
  5. `ALTER DATABASE <name>_build RENAME TO <name>`.
  6. `COMMENT ON DATABASE <name> IS '<unix seconds>'`.
  7. Drop every other `crack_tpl_%` except the 3 newest by comment, ignoring "being accessed"
     errors.
  8. Unlock.
- **`run`:**
  - Drops `test_db_name(test_path)` `WITH (FORCE)` if it exists, then creates it with
    `CREATE DATABASE … TEMPLATE <template>`.
  - Calls the test.
  - If `passed()`, closes the pool and drops the database `WITH (FORCE)`. On `Err`, or a panic
    unwinding through `run`, the database is left in place.
- Quote every identifier, as `"name"`. The names are generated, but quote them anyway.

- [ ] **Step 1: Write the failing pure tests** in `crack-core/src/test_db.rs`:
  - `template_name_is_stable_and_prefixed`: two calls on the same migrator are equal, the
    result starts with `crack_tpl_`, and its length is 26.
  - `template_name_changes_when_a_checksum_changes`: clone the migrations of
    `sqlx::migrate!("./test_migrations")` into a new `Migrator`, change one byte of the last
    checksum, and the names differ.
  - `test_db_name_fits_a_postgres_identifier`: for a 300-character path, the length is at most
    63, it is all lowercase ASCII, two different paths differ, and it starts with
    `crack_test_`.
  - `outcome_passed`: `().passed()`, `Ok::<(), ()>(()).passed()` and `!Err::<(), ()>(()).passed()`.

- [ ] **Step 2: Run them, and see them fail to compile** because the functions don't exist:
  `cargo nextest run -p crack-core test_db::`

- [ ] **Step 3: Implement** `Outcome`, `template_name` and `test_db_name`. Run step 2's command:
  PASS.

- [ ] **Step 4: Write the failing database tests**, each a plain `#[test]` that calls `run` or
  `ensure_template` directly, gated with the usual `cfg_attr(not(feature = "db-tests"), ignore = …)`:
  - `run_gives_a_migrated_database`: inside the closure,
    `select count(*) from _sqlx_migrations` equals the migrator's non-down migration count.
  - `run_drops_the_database_after_a_pass`: after `run` returns `()`, `test_db_name(path)` is
    absent from `pg_database`.
  - `run_keeps_the_database_after_a_failure`: the closure returns `Err(())`; afterwards the
    database exists. Then drop it in the test.
  - `a_leftover_test_database_is_replaced`:
    1. Create `test_db_name(path)` by hand, with a marker table.
    2. Call `run` for that path.
    3. Inside it, the marker table is absent.
  - `two_runners_racing_build_one_template`:
    1. Use a migrator with a unique fake checksum (a cloned migrator, so the template doesn't
       exist yet).
    2. Call `ensure_template` from two threads, each with its own connection and runtime.
    3. Both return the same name; exactly one database by that name exists; no `_build`
       database remains.
    4. Clean up the template afterwards.

- [ ] **Step 5: Run them against local `pg`, and see them fail:**
  `cargo nextest run -p crack-core --features db-tests test_db::`

- [ ] **Step 6: Implement** `ensure_template` and `run`. Run step 5's command: PASS.

- [ ] **Step 7: Write the macro crate.**
  - Implement the attribute exactly as in Interfaces.
  - Add attribute-shape tests in `crack-core/src/test_db.rs` that use
    `#[crack_test_macros::db_test(migrator = "MIGRATOR")]`, one for each return type the suite
    uses:
    - `()`;
    - `sqlx::Result<()>`;
    - `Result<(), crate::errors::CrackedError>`;
    - `()` with `#[expect(unused_variables)]` and an unused `let` in the body, which proves
      that attributes reach the body.

    Each one runs a trivial query.

- [ ] **Step 8: Verify.**
  - `cargo nextest run -p crack-core --features db-tests test_db::` passes.
  - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return` passes.
  - `cargo clippy -p crack-core --all-targets --features db-tests -- -D warnings` passes.

- [ ] **Step 9: Commit** `crack-test-macros/`, `crack-core/src/test_db.rs`,
  `crack-core/src/lib.rs`, `crack-core/Cargo.toml`, `Cargo.toml` and `Cargo.lock` with the
  message `test: template databases for crack-core's db tests (#[db_test])`.

### Task 5: Move crack-core's database tests to `#[db_test]`

**Files:**
- Modify: every crack-core `.rs` file containing `#[sqlx::test(migrator = "MIGRATOR")]`. That's
  168 lines, including the `macro_rules!` wrappers in `economy/*.rs` and `db/economy.rs`.
- Leave alone: crack-voting's and crack-cli's `#[sqlx::test]` sites (spec, section 3).

**Interfaces:**
- Consumes: Task 4's attribute.

- [ ] **Step 1: Record "before".** Run
  `cargo nextest run -p crack-core --features db-tests,crack-coin` against local `pg`, and
  note the wall time, the test count, and the summed test time (nextest prints the per-test
  times).

- [ ] **Step 2: Replace the attribute.** In crack-core only, replace every
  `#[sqlx::test(migrator = "MIGRATOR")]` with
  `#[crack_test_macros::db_test(migrator = "MIGRATOR")]`. The 3 commented-out
  `// #[sqlx::test(…)]` lines stay as they are.

- [ ] **Step 3: Check the result.**
  - `git grep -n 'sqlx::test(migrator' -- crack-core` shows only the commented-out lines.
  - `cargo check -p crack-core --tests --features db-tests,crack-coin` is clean.

- [ ] **Step 4: Record "after".** Run step 1's command:
  - It passes, with **the same test count** as step 1.
  - Note the wall time and the summed time.
  - `docker --context default exec pg psql -U postgres -tAc "select count(*) from pg_database where datname like 'crack_test_%' or datname like '%_build'"`
    prints `0`.

- [ ] **Step 5: Check the parallelism.** Run step 1's command 3 times. Each run passes, with no
  `too many clients` errors. If there are any, add a test group to `.config/nextest.toml` and
  justify its numbers in a comment.

- [ ] **Step 6: Check the default suite.** Without `db-tests`, `cargo nextest run -p crack-core`
  passes and reports the database tests as skipped or ignored.

- [ ] **Step 7: Commit** the changed files by name (list them with
  `git diff --name-only | xargs git add --`). Use the message
  `test: crack-core's db tests clone a template database`, and put the before and after
  numbers in the body.

### Task 6: Baseline tooling

**Files:**
- Create: `scripts/mutants_baseline.py` and `scripts/test_mutants_baseline.py`.

**Interfaces:**
- **Line formats.**
  - **Input:** cargo-mutants' missed-list format, one mutant per line, as `--list` prints it:
    `crack-core/src/economy/earn.rs:48:5: replace voice_earners -> Vec<UserId> with vec![]`.
  - **Key:** the line with `:LINE:COL` removed:
    `crack-core/src/economy/earn.rs: replace voice_earners -> Vec<UserId> with vec![]`.
- **The baseline file** is `.cargo/mutants-baseline.txt`. It holds one key per line, sorted.
  Lines starting with `#` and blank lines are ignored.
- **Command-line interface,** used by Task 9 and by PR B:
  - `python3 scripts/mutants_baseline.py compare --baseline FILE --missed FILE`
    - Prints a summary: the count of new misses with each new line in full, then the count of
      known misses.
    - Exit codes: 1 on any new miss, 0 otherwise. A missing baseline file means everything is
      new.
  - `python3 scripts/mutants_baseline.py refresh --baseline FILE --missed FILE [--init]`
    - Rewrites the baseline as the multiset intersection, sorted with its header comment kept.
    - Prints the number removed.
    - Exit codes: 1, without writing, if `missed` has keys the baseline doesn't. `--init`, which
      is valid only when the file is absent, writes every key.
- **Functions:**
  - `key(line: str) -> str`: raises `ValueError` on a line that doesn't match
    `^(.+?):\d+:\d+: (.+)$`;
  - `load_keys(path) -> collections.Counter[str]`.

- [ ] **Step 1: Write the failing tests** in `scripts/test_mutants_baseline.py`, using stdlib
  `unittest` and temporary files:
  - `test_key_strips_line_and_column`;
  - `test_key_rejects_malformed`;
  - `test_shifted_lines_are_not_new`: the same descriptions at different lines;
  - `test_duplicates_count_twice`: a baseline with one copy and a missed list with two copies
    gives one new miss;
  - `test_new_miss_fails_compare`: exit code 1, and the full line is printed;
  - `test_missing_baseline_means_all_new`;
  - `test_refresh_shrinks`;
  - `test_refresh_refuses_growth`: exit code 1, and the file is unchanged;
  - `test_init_writes_sorted_keys`;
  - `test_init_refuses_when_baseline_exists`;
  - `test_comments_and_blanks_ignored`.

  Take the fixture lines from a real run:
  `cargo mutants --list -f crack-core/src/economy/earn.rs | head`.

- [ ] **Step 2: Run them, and see them fail:**
  `python3 -m unittest scripts/test_mutants_baseline.py -v`

- [ ] **Step 3: Implement `scripts/mutants_baseline.py`** with `argparse` and the stdlib only.

- [ ] **Step 4: Run step 2's command:** PASS.

- [ ] **Step 5: Commit** both files with the message
  `tooling: mutants baseline compare/refresh (shrink-only ratchet)`.

### Task 7: The target sweep

**Files:**
- Create: `scripts/sweep_target.py` and `scripts/test_sweep_target.py`.

**Interfaces:**
- `target_dir() -> pathlib.Path`: from
  `cargo metadata --format-version 1 --no-deps`'s `target_directory`.
- `stale_incremental(target: Path, now: float, max_age_days: int = 14) -> list[Path]`
  - Finds every directory named `incremental` at depth 1–3 under `target`, such as
    `debug/incremental`, `mutants/incremental` and `<triple>/debug/incremental`.
  - Returns those directories' direct children whose mtime is older than `max_age_days`.
- `stale_scratch(root: Path, now: float, max_age_hours: int = 24) -> list[Path]`: children of
  `root` named `cargo-mutants-*` whose mtime is older than `max_age_hours`.
- `main(argv) -> int`
  - Flags: `--dry-run`, and `--scratch-root` (default: `$CRACK_MUTANTS_SCRATCH`, or else
    `$HOME/.cache/cargo-mutants`).
  - **It refuses, with exit code 2, if the target directory has no `CACHEDIR.TAG`.** Cargo
    writes one in every target directory.
  - It deletes the stale paths and prints each one, then prints the target size using
    `du -sb`.
  - Above 150 GiB, it prints a loud warning: the directory is shared, and clearing it is a
    person's call. It never deletes anything else.

- [ ] **Step 1: Write the failing tests** with `tempfile.TemporaryDirectory` and `os.utime`:
  - `test_finds_stale_incremental_children`;
  - `test_keeps_recent_and_non_incremental`: a stale file in `debug/deps` and a fresh
    incremental child both survive;
  - `test_refuses_a_directory_without_cachedir_tag`: exit code 2, and nothing deleted;
  - `test_stale_scratch_only_matches_cargo_mutants_prefix`;
  - `test_dry_run_deletes_nothing`.

- [ ] **Step 2: Run them, and see them fail:**
  `python3 -m unittest scripts/test_sweep_target.py -v`

- [ ] **Step 3: Implement,** with the stdlib only. `main` takes an optional target override so
  the tests never touch the real shared directory.

- [ ] **Step 4: Run step 2's command:** PASS.

- [ ] **Step 5: Do a dry run on the real directory** with `python3 scripts/sweep_target.py --dry-run`.
  Put its output in the report, and check it lists only `incremental/` children and scratch
  directories.

- [ ] **Step 6: Commit** both files with the message
  `tooling: sweep stale incremental state from the shared target dir`.

### Task 8: cargo-mutants configuration and the local wrapper

**Files:**
- Create: `.cargo/mutants.toml` and `scripts/mutants.sh`.
- Modify: the root `Cargo.toml`: add `[profile.mutants]`.

**Interfaces:**
- Consumes:
  - `scripts/sweep_target.py` (Task 7);
  - nextest's `mutants` profile (Task 2).
- **Produces `scripts/mutants.sh [cargo-mutants args…]`**, used by PR B's diff check and for
  manual passes.
  - It runs `bash` with `set -euo pipefail`.
  - It refuses `--in-place`, with exit code 2.
  - It runs `python3 scripts/sweep_target.py`.
  - It creates `${CRACK_MUTANTS_SCRATCH:-$HOME/.cache/cargo-mutants}`.
  - It exports:
    - `TMPDIR=` that directory;
    - `CARGO_TARGET_DIR=target`: relative, so each job's scratch copy builds into its own
      `target/`;
    - `NEXTEST_PROFILE=mutants`.
  - Then it runs `exec cargo mutants "$@"`.
- **Produces `.cargo/mutants.toml`.** Check every key against `cargo mutants --help` and the
  27.1 docs.
  - `test_tool = "nextest"`;
  - the features `crack-core/db-tests`, `crack-core/crack-coin`, `cracktunes/db-tests` and
    `crack-voting/db-tests`;
  - `profile = "mutants"`;
  - the initial exclusions:
    - **Always** `crack-test-macros/**`. Its reason: test tooling exercised by every crack-core
      database test, which cargo-mutants doesn't run when it mutates this package.
    - **Anything else** only as a category that is never worth testing, each with a `#` reason
      comment above it. **Legacy misses go to the baseline, not here.**
    - Inspect `crack-testing`. If its tests are all network-`#[ignore]`d, exclude it, with the
      reason given.
    - Never exclude `crack-core/src/coin/**`, or any `Debug` impl that redacts.
- **`[profile.mutants]`:** `inherits = "test"` and `debug = false`, with a one-line comment.

- [ ] **Step 1: Write the three files** as specified.

- [ ] **Step 2: Check the configuration is read:**
  `scripts/mutants.sh --list | wc -l`. The count is about 7,700, minus whatever the exclusions
  remove; record it. Then `scripts/mutants.sh --list -f crack-test-macros/src/lib.rs` prints
  nothing.

- [ ] **Step 3: Check that `--in-place` is refused:** `scripts/mutants.sh --in-place --list`
  exits 2.

- [ ] **Step 4: Check the private target dirs.** Run
  `scripts/mutants.sh -f crack-core/src/economy/earn.rs -j 2 --leak-dirs -o "$HOME/.cache/cargo-mutants/earn-run"`
  against local `pg`.
  - The run completes. Record caught, missed and unviable, and the wall time.
  - Each leaked scratch copy under `$HOME/.cache/cargo-mutants/` contains its own `target/`
    directory.
  - Delete the leaked copies and the output afterwards.

- [ ] **Step 5: Measure the job count.** Time the same file with `-j 1` and with `-j 4`, each
  timing including the cold builds. Record both, and the peak free-disk drop
  (`df -h $HOME` before and during). Put the recommended `-j` for a roughly 20-mutant diff in
  the report. Task 10 documents it.

- [ ] **Step 6: Commit** `.cargo/mutants.toml`, `scripts/mutants.sh` and `Cargo.toml` with the
  message `tooling: cargo-mutants config and a safe local wrapper`.

### Task 9: The full-run workflow (dispatch only)

**Files:**
- Create: `.github/workflows/mutants-full.yml`.

**Interfaces:**
- Consumes:
  - `.cargo/mutants.toml` (Task 8);
  - `scripts/mutants_baseline.py compare` (Task 6);
  - nextest's `mutants` profile.
- **Produces an artifact named `mutants-missed`,** containing `missed-all.txt`. PR B builds the
  first baseline from it.

**The workflow's shape:**
- **Trigger:** `on: workflow_dispatch` only, with these inputs:
  - `shards`, a number, default `40`;
  - `max_parallel`, a number, default `8`.

  The schedule arrives in PR B, once a run's duration is known (spec, section 4, "The full
  run").
- **Permissions:** `contents: read`.
- **Job `plan`:** outputs `matrix` as a JSON array `[0 … shards-1]`.
- **Job `shard`:**
  - `needs: plan`;
  - `strategy: { fail-fast: false, max-parallel: ${{ fromJSON(inputs.max_parallel) }}, matrix: { shard: ${{ fromJSON(needs.plan.outputs.matrix) }} } }`;
  - `timeout-minutes: 340`.
  - **Steps:**
    1. Check out.
    2. Install yt-dlp, as `build.yml` does.
    3. Install stable Rust.
    4. Pinned Postgres 16.
    5. `Swatinem/rust-cache@v2`, with `shared-key: mutants`.
    6. `taiki-e/install-action` for `cargo-nextest@0.9.149,cargo-mutants@27.1.0`.
    7. With `SQLX_OFFLINE=true`, `DATABASE_URL` from the Postgres step, and
       `NEXTEST_PROFILE=mutants`, run:
       `cargo mutants --in-place --shard ${{ matrix.shard }}/${{ inputs.shards }} --output out`

       CI may run in place, because the runner is disposable.
    8. Treat cargo-mutants exit codes 0, 2 (missed) and 3 (timeouts) as success, and anything
       else as failure.
    9. Upload `out/mutants.out/{missed,caught,unviable,timeout}.txt` and `outcomes.json` as
       `mutants-shard-<n>`.
- **Job `collect`:**
  - `needs: shard`, `if: always()`.
  - Download every `mutants-shard-*` artifact.
  - **If fewer artifacts arrived than `shards`,** fail with the message "incomplete run: N of M
    shards".
  - Write `cat */missed.txt | sort > missed-all.txt`.
  - Write the counts (caught, missed, unviable, timeout) to `$GITHUB_STEP_SUMMARY`.
  - If `.cargo/mutants-baseline.txt` exists, run `compare`, which goes red on new misses.
    Otherwise write "no baseline yet: recording only".
  - Upload `missed-all.txt` as `mutants-missed`.

- [ ] **Step 1: Write the workflow.** The header comment explains:
  - why it is dispatch-only for now;
  - why the cap is 8 (the account's concurrent-job limit is shared);
  - why it runs in place;
  - why exit codes 2 and 3 are not failures.

- [ ] **Step 2: Check the syntax.** It parses as YAML:
  `python3 -c 'import yaml,sys; yaml.safe_load(open(sys.argv[1]))' .github/workflows/mutants-full.yml`.
  If `actionlint` is installed, it must pass too.

- [ ] **Step 3: Smoke-test it end to end.** Workflows run from the pushed commit on `push`.
  1. Temporarily add `push: branches: [chore/test-tooling]`.
  2. For push events only, default the shard count to `400` and limit the matrix to shard `0`,
     about 19 mutants.
  3. Push the branch. The controller does the push, so just commit and report.
  4. The run must reach `collect` and upload `mutants-missed`.
  5. Then remove the temporary trigger, and commit that removal separately.

  The controller watches the run and reports its result back to you.

- [ ] **Step 4: Commit** the workflow with the message
  `ci: dispatch-only sharded full cargo-mutants run`.

### Task 10: Docs

**Files:**
- Modify: `docs/testing.md` and `README.md` (the Testing, coverage and Linting sections).

**Interfaces:**
- Consumes the numbers recorded in Tasks 2, 3, 5 and 8, which the controller passes in.

- [ ] **Step 1: Rewrite `docs/testing.md`** as the one page for testing. Keep its existing "Why
  they are off by default" and "Tests that reach the network" sections, and keep
  `#[cfg_attr(..., ignore)]` with its reason. It gains:
  - **Running tests:** `cargo nextest run` with the three profiles. Doctests run with
    `cargo test --doc`.
  - **The database tests:**
    - `#[crack_test_macros::db_test(migrator = "MIGRATOR")]`;
    - how templates are named, built and swept;
    - the leftover-database rule (kept on failure, replaced on re-run);
    - the default URL coming from the `db/user.rs` ctor;
    - the before and after numbers from Task 5;
    - crack-voting and crack-cli remaining on `#[sqlx::test]`.
  - **Coverage:** the `cargo llvm-cov nextest …` command, and that CI runs it weekly.
  - **Mutation testing,** as far as PR A goes:
    - `scripts/mutants.sh` and why it refuses `--in-place`;
    - the scratch location;
    - the recommended `-j` from Task 8;
    - how to run one file or module;
    - the full-run workflow and how to dispatch it (`gh workflow run mutants-full.yml`);
    - the baseline file and `mutants_baseline.py refresh`.
  - **What stays manual,** listed from the spec: SQL text, lock order, plain assignments, and
    macro input.
  - **The target sweep:** what it deletes, and what it never deletes.
- [ ] **Step 2: Update `README.md`.**
  - Testing points to `docs/testing.md`, with the nextest command.
  - The coverage paragraph becomes two sentences: cargo-llvm-cov, weekly in CI, and the local
    command.
- [ ] **Step 3: Verify.** `git grep -il tarpaulin` lists only the spec and plan docs under
  `docs/superpowers/`.
- [ ] **Step 4: Commit** both files with the message
  `docs: testing with nextest, template databases, coverage and cargo-mutants`.

---

## Outside this plan (controller steps)

**After merge:**
1. Dispatch `coverage.yml` once and confirm it uploads.
2. Dispatch `mutants-full.yml` with the defaults, and watch it.
3. Its `mutants-missed` artifact seeds PR B.

**Then:**
- Plan PR B: the diff check, the gate, the waivers and their reason check, the PR job, the
  first baseline, the schedule and rotation from measured duration, and the mutants docs.
- The catacombs `.gitignore` one-liner, as its own small PR.
