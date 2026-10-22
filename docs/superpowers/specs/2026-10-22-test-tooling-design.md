# Test tooling: tarpaulin out, mutation testing in, faster tests

2026-10-22. Agreed with the owner in chat, after a measurement spike the same day.

## Why

Two things, in the owner's words.

- **Mutation testing by hand is slow and costs tokens.** "Right now you break the code
  manually, by hand, on each of these mutation tests. That takes tokens physically to do it,
  and to decide what to do." Proving that a test can fail should be automatic and deliberate.
  It should cost machine time, not an agent's turns.
- **Tarpaulin should leave the repos completely.** A replacement was chosen on 2026-09-12 and
  recorded in `.github/workflows/coverage.yml` ("prefer cargo-llvm-cov"). Nothing was done
  about it.

Build speed and configuration are in scope as well: "I suppose build speed and config are also
in scope for this definitely." The agent does most of its work autonomously, so every minute
of build and test time counts many times over.

## Goals

1. **Tarpaulin is gone** from every repository, and cargo-llvm-cov measures coverage.
2. **cargo-nextest runs the tests** locally, in CI and under cargo-mutants.
3. **cargo-mutants checks every PR's diff.**
   - By default, a missed mutant blocks the merge. Each one is either caught by a new or
     better test, or waived with a written reason.
   - The check replaces the hand-sabotage step in the agent's task loop, and the
     "mutation → caught-by" table in PR bodies.
4. **Legacy misses are worked down on purpose.**
   - A committed baseline lists every known missed mutant, and it may only shrink.
   - A scheduled full run keeps the baseline honest.
5. **Tests and builds are faster:**
   - template test databases;
   - a lighter debug-info profile;
   - a regular sweep of build directories;
   - parallel mutant jobs.

## Non-goals

- **Fixing the legacy misses.** This arc builds the ratchet. The work-down happens in later,
  deliberate per-module passes.
- **Splitting crack-core.** It would help mutation testing, because every mutant rebuilds the
  whole 116k-line crate. But it belongs to the crack-types rethink, which comes after this arc.
- **A coverage target or gate.** Coverage is informational. The mutants check is the gate.
- **Changing what the bot does.** This arc ships no bot behaviour, so it needs no version bump
  and no deploy.

## Anti-goals: it fails if…

- **A slow PR check becomes routine.**
  - The diff check must fit comfortably inside a normal PR cycle.
  - A check that takes longer than the build it gates will get skipped, and then it is noise.
  - The old tarpaulin job, at 9–16 minutes on every push, is the cautionary tale.
- **Waivers become a rubber stamp.** Every waiver carries a reason that a reviewer can
  disagree with.
- **Tests start to depend on hidden machine state,** such as a pre-migrated `template1`, or a
  database that only works on the owner's box.
- **The suite stops passing on a clean checkout with nothing running.** The database tests stay
  ignored unless `db-tests` is on (`docs/testing.md`, "Why they are off by default").

## Current state (measured 2026-10-22)

Machine: 20 cores, 31 GB RAM. `/tmp` is a 16 GB **tmpfs**. Rust 1.99 stable. cargo-nextest
0.9.149 and cargo-mutants 27.1.0 are installed locally.

**Tarpaulin's footprint:**
- **`#[cfg(not(tarpaulin_include))]`:** 329 attributes in 119 files, plus one commented-out
  copy. The cfg is always true in a normal build, so deleting the attributes changes nothing
  that is compiled.
- **`build.rs`:** `cargo:rustc-check-cfg=cfg(tarpaulin_include)` in the root file and in
  `crack-cli`, plus its comment in the root file.
- **`Cargo.toml`:** `unexpected_cfgs = { level = "warn", check-cfg = ['cfg(tarpaulin_include)'] }`
  in crack-core, crack-types, crack-testing, crack-voting and crack-cli.
- **`.github/workflows/coverage.yml`:** a manual-only tarpaulin job (nightly, codecov-action held
  at v4).
- **`.github/dependabot.yml`:** an ignore rule for codecov-action's major version, which "moves
  with tarpaulin's replacement".
- **`README.md`:** the "Coverage is not measured in CI" paragraph.
- **`scripts/lint_test_build.sh` and `.fish`:** a tarpaulin line in each. In the `.sh`, it sits
  after an `exit 0`.
- **`.build.yml`:** a sourcehut manifest last touched 2024-11. It installs tarpaulin and has a
  commented-out tarpaulin line.
- **`cycle-five/catacombs`:** a `tarpaulin-report.html` line in `.gitignore`.

No other local repository mentions tarpaulin. runecast-backend's `dictionary.txt` matches only
as a word.

**Tests:**
- crack-core's suite takes about 36 s with `cargo test` and about 40 s with nextest. Most of it
  is database tests.
- There are 168 `#[sqlx::test(migrator = "MIGRATOR")]` sites in crack-core. Some
  `macro_rules!` wrappers in `economy/*` and `db/economy.rs` expand to the same attribute, so
  about 350 tests are database tests.
- **Every database test builds a fresh database and runs all 37 test migrations into it.**
  - About 350 tests take more than 2 s each, about 964 test-seconds in all.
  - On the local Postgres 16, the migrations take about 950 ms per database. A
    `CREATE DATABASE … TEMPLATE` copy takes about 25 ms, and a drop about 15 ms, with an
    occasional checkpoint at about 1.3 s.
  - sqlx 0.9's test harness has no template option. `sqlx-postgres` `testing/mod.rs` always
    runs a bare `create database` and then the migrator.
- **crack-voting and crack-cli have one database test each.**
- **`crack-core/src/guild/permissions.rs` has a `#[ctor]` that prints**
  `WARNING: DATABASE_URL not set for tests` **to stdout,** and does nothing else: its `set_var`
  is commented out. Two consequences:
  - The stdout line breaks `cargo nextest list` parsing.
  - `docs/testing.md` wrongly says the ctor supplies a default `DATABASE_URL`.
- **`crack-core/test_migrations` is a copy of `migrations/` plus `20990328005454_test_db.sql`,**
  and `20240705083637_crack_voting.sql` has drifted between the two. This is noted here and
  left for the follow-ups, outside this arc.

**Mutation testing (the spike):**
- **PR #622's Rust diff** (8 files) produced 19 mutants: 13 caught, 4 unviable, 2 missed.
- **Both misses were glue:** `earn.rs` `snapshot` (the serenity cache) and `credits.rs`
  `history` (a poise command body).
- **The run took 20 min** in place, one job at a time:
  - baseline: a 135 s build and a 40 s test;
  - each mutant: about a 32 s build, then 40–80 s of tests.
- **The tool did not generate a mutant equivalent to deleting the assignment**
  `c.voice_requires_camera = on`. It doesn't mutate plain assignments, SQL text or lock
  ordering.
- **Mutant counts** (`cargo mutants --list`, with db-tests and crack-coin):

  | Crate | Mutants |
  |---|---|
  | crack-core | 6,598 |
  | crack-web | 335 |
  | crack-types | 244 |
  | crack-testing | 239 |
  | crack-musicreco | 164 |
  | crack-sleevenote | 112 |
  | crack-voting | 25 |
  | crack-cli | 15 |
  | **Total** | **about 7,700** |

**Builds:**
- **A rebuild after a real crack-core edit:**
  - 18 s with the current dev profile;
  - 12–13 s with `debug = "line-tables-only"` for the workspace and `debug = false` for
    dependencies.
- **A cold build with those settings** took 164 s and 3.4 GB.
- **Every project on this machine builds into one shared target directory,**
  `~/.cache/cargo-target`.
  - It's set by `build.target-dir` in `~/.cargo/config.toml`, outside any repo.
  - The owner chose it on 2026-08-14, because per-worktree target directories were filling the
    disk.
  - Cargo locks the directory, so concurrent builds wait for one another.
  - It is shared with RuneCast and every other project, and it has reached 521 GB before.
  - The 316 GB `cracktunes/target` deleted on 2026-10-22 was a leftover from before that
    setting.
- **cargo-mutants 27.1.0 never overrides the target directory,** so its parallel jobs would
  also build into the shared directory and wait on its lock.

**CI:**
- `build.yml` runs `cargo test` with the db-tests and crack-coin features against a pinned
  Postgres 16, on nightly and stable. On master and tags it runs in `--release`.
- Master's branch protection requires no status checks. A red job blocks a merge only because
  the release cadence refuses to merge red.

## The design

### 1. Coverage: tarpaulin out, cargo-llvm-cov in

**Delete every trace listed above**, in this repository and in catacombs. For catacombs, a
one-line PR, with no version bump because it ships no artifact.

**Rewrite `coverage.yml` around cargo-llvm-cov:**
- stable toolchain;
- `cargo llvm-cov nextest` over the workspace with the same db-tests and crack-coin features,
  against the same pinned Postgres;
- lcov output, uploaded with codecov-action at its current major.

Then:
- Drop the dependabot ignore rule.
- Rewrite the README coverage paragraph.
- Remove tarpaulin's lines from `.build.yml` and the two `lint_test_build` scripts.

**My call:** coverage runs weekly and on manual dispatch, not on every push. It is
informational, and an every-push coverage job is what made tarpaulin a burden.

### 2. Test runner: cargo-nextest

- **Commit `.config/nextest.toml` with three profiles:**
  - **`default`, for local runs:** a slow-test warning, and a hard per-test timeout so a hung
    test can't stall a run.
  - **`ci`:** no fail-fast, so one run reports every failure.
  - **`mutants`:** fail-fast, so a caught mutant stops at its first failing test. This is a
    large speed-up, because most mutants are caught early.

  The builder sets the timeout values from measured test times. **No profile retries a failed
  test,** because retries hide flaky tests.
- **CI's test steps switch to `cargo nextest run`,** with the same features, toolchains and
  `--release` rule.
- **nextest doesn't run doctests,** so a separate `cargo test --doc` step keeps them running.
- **Delete the no-op stdout `#[ctor]` in `guild/permissions.rs`.** The database-test runner
  (next section) reports a missing `DATABASE_URL` itself. Remove the `ctor` dependency if
  nothing else uses it.
- **Fix `docs/testing.md`:** it should show the nextest commands, describe the template
  databases, and drop the claim about a default URL.

### 3. Template test databases

A crack-core test attribute replaces `#[sqlx::test(migrator = "MIGRATOR")]`. It takes the same
argument and supports the same function shapes the suite uses today: `(pool: PgPool)`
returning `()`, `sqlx::Result<()>` or `Result<(), E>` for the error types in use. Moving a test
over is then a one-line path change, including inside the `macro_rules!` wrappers.

**Behaviour:**
- **One template per set of migrations.**
  - The template's name comes from a hash of the migrator's versions and checksums, so any
    change to `test_migrations/` gets a new template.
  - The first test that needs a template builds it:
    - under a Postgres advisory lock, so parallel processes don't race;
    - under a temporary name, then renamed once complete with every connection closed, so no
      test ever sees a half-migrated template.
  - nextest runs each test in its own process, so "already exists" is checked against
    `pg_database` on every test. That check is one cheap query, not an in-process flag.
- **One database per test,** copied from the template.
  - Its name is derived from the test's path, so a re-run first drops the database a failed
    run left behind.
  - The database is dropped when the test passes and kept when it fails, for inspection.
    `#[sqlx::test]` behaves the same way.
- **Old templates are swept,** keeping a small fixed number, newest first. Several worktrees
  on different branches can need different templates at the same time.
- **Each test pool is capped at five connections,** as `#[sqlx::test]` caps it.
  - At nextest's default parallelism, the total must stay under the server's connection limit,
    100 on both the local container and CI's Postgres.
  - The builder measures this. If the total goes over, they cap it with a nextest test group or
    `test-threads`.
- **A missing `DATABASE_URL` fails with a clear message.** This applies only when a database
  test actually runs, which needs `db-tests`.
- **The existing `#[cfg_attr(not(feature = "db-tests"), ignore = …)]` lines stay as they are.**
  **My call:** less churn, and the gating stays visible at each test.

**How it's built.** A small proc-macro crate in the workspace provides the attribute, and the
runner lives in crack-core's test support. **My call:** a crate of our own rather than
`sqlx-pg-test-template` 0.1.2. That crate is 161 lines and doesn't run migrators or support
tests that return `Result`; it also assumes the template already exists.

crack-voting's and crack-cli's single database tests stay on `#[sqlx::test]`. Two slow tests
don't justify the wiring.

**Rejected:** pre-migrating `template1` on a dedicated test server. It needs no code changes,
but it's hidden machine state: the anti-goal above. It would also pollute every database the
local server creates, and it breaks the other crates, whose migrators would see versions they
don't have.

### 4. Mutation testing: cargo-mutants

**Configuration** is committed in `.cargo/mutants.toml`:
- nextest as the test tool, with the `mutants` profile;
- the db-tests and crack-coin features, so database code is tested and not reported as missed
  because its tests were ignored;
- a cargo profile for mutant builds that turns debug info off;
- the initial exclusions. **Each exclusion has a comment giving its reason.**

**The diff check** is one script, used both locally and in CI:
1. Take the Rust diff against a base.
   - By default, the base is the merge base with master.
   - The agent's task loop passes the commit the task started from, so each task checks only
     its own change. Comparing every task against master would re-check the whole branch so
     far, and the run would get longer with each task.
2. Run `cargo mutants --in-diff` with parallel jobs.
3. Compare the misses with the baseline and print a short summary:
   - counts of caught, missed, unviable and timed-out mutants;
   - each new miss, with its location;
   - known misses in the touched code, listed separately.
4. Exit non-zero on any **new** miss.

Timeouts count as caught, but they are listed.

**My call: a known miss in a touched function is reported but does not block.**
- Under `--in-diff`, a one-line change exposes the whole-function mutants of the function it
  touches.
- Making those block would put the legacy debt on whichever PR happens to touch it. Working
  that debt down belongs to the deliberate passes ("chip away on purpose").
- The summary still lists those misses, and the reviewer weighs them, which is the "leave it
  better" norm.

**Local runs: each job gets its own build directory, on disk.**
- **Normal builds keep the shared target directory.** The owner's 2026-08-14 decision stands.
- **Each parallel job of a mutants run builds into a private target directory inside its own
  scratch copy.**
  - Otherwise every job waits on the shared directory's lock, and `--jobs N` buys nothing.
  - The intended mechanism is a *relative* `CARGO_TARGET_DIR` (`target`), which cargo resolves
    against each job's working directory. The builder verifies that it works.
- **Scratch copies go on disk, under the user's cache directory, never `/tmp`.** `/tmp` is a
  16 GB tmpfs, and several cold builds would fill RAM. cargo-mutants deletes its scratch copies
  when a run ends.
- **The builder measures** the job count that gives the best wall time, counting each job's
  first cold build, and the disk that count needs.

**PR CI** gets a mutants job:
- **When it runs:** on `pull_request`. It returns success at once when the diff has no `.rs`
  changes, so dependabot's lockfile PRs and docs PRs cost nothing.
- **What it runs on:** the same pinned Postgres, prebuilt tool binaries, and the Rust cache.
- **Budget:** a typical PR, up to about 20 mutants, finishes within about 15 minutes of the
  Build job. PR CI takes 8–12 minutes today.
- **Big feature PRs take longer, and that is accepted. My call.**
  - The economy PRs added 5k–12k lines of Rust, which would be several hundred mutants each.
  - The job lists the diff's mutants first, which is cheap and needs no build, then shards by
    count with `--shard k/n` across a matrix.
  - A small PR runs as one shard. A big PR gets more shards, up to a cap that leaves most of
    the account's concurrent-job slots free (section "The full run").
  - Big feature PRs are a few per arc, and their review takes hours anyway. The anti-goal is a
    slow check on *every* PR, and small PRs stay fast.
  - The real gate is the per-task local check in the agent's loop; CI is the backstop.
- **My call:** it's not made a required status check. The cadence already refuses to merge
  red, and a required check is a repository-settings change that is the owner's to make.

**Waivers:**
- A function-level waiver is `#[mutants::skip]`, written as `#[cfg_attr(test, mutants::skip)]`
  with `mutants` as a dev-dependency, so release builds don't depend on it.
- A category waiver is an exclusion pattern in `.cargo/mutants.toml`.
- **Every waiver carries a written reason:**
  - a comment directly above a `mutants::skip`;
  - a comment on a config exclusion.

  A check script fails CI when a reason is missing.

**Acceptable reasons:**
- **An equivalent mutant:** no test can tell the mutant from the original.
- **Logging- or display-only code.** This never covers the privacy-critical `Debug` impls, such
  as the coin key and anything that could print an address.
- **Performance-only knobs,** such as capacities and buffer sizes.
- **Live-Discord glue that can't be reached offline.** Prefer moving the decision into a pure
  function, which the platform-seam rules
  (`docs/investigations/2026-10-22-matrix-music.md`, "Keep the seam in mind") ask for anyway.

**The baseline:**
- A committed file lists every known missed mutant.
- **Each entry is the file and the mutant's description, without line and column,** so normal
  edits don't churn it. Entries are matched as a multiset, so two identical descriptions in one
  file count twice.
- **It may only shrink:**
  - A full run that finds a miss not in the baseline goes red and lists it. That can happen
    when a test is deleted, or when a waiver is removed.
  - A refresh drops entries that are no longer missed.
  - Nothing adds entries automatically. A new miss is fixed or waived.

**The full run** is a scheduled workflow, sharded across a matrix on GitHub's free runners:
- **When:** weekly, plus manual dispatch.
- **What it produces:** the missed list as an artifact.
- **Refreshing:** a script downloads that artifact and rewrites the baseline. The refresh
  happens in a normal PR.
- **My call:** no workflow write permissions. An agent or the owner runs the refresh. A
  workflow that opens PRs needs `contents: write`, `pull-requests: write` and a repository
  setting, and buys little.
- **Shard count:** set from measured per-mutant times, so that each shard finishes in about
  three hours, well under the six-hour job limit.
- **Concurrency: a cap of 8 jobs at once, with `max-parallel`. My call.**
  - `cycle-five` is a user account. Its concurrent-job limit (20 on Free) is shared by every
    repo it owns: sleevenote, crack-coin, and cracktunes' own PR builds.
  - A full run must not queue their CI behind it for hours.
  - **The schedule:** a weekly window when nobody is usually pushing. The builder picks the
    exact time.
  - **If a measured full run would take more than about 12 hours** at that cap, it rotates
    instead: each week covers a fixed slice of the files, and the whole tree is covered every
    few weeks. The ratchet then checks each slice against the baseline entries for its files.
- **The first baseline is committed in the same PR that turns the diff check on**
  ("Order of work"), so no PR ever runs against a missing baseline.

**What stays manual.** cargo-mutants can't mutate:
- SQL text, including `WHERE` guards;
- the order in which locks are taken;
- plain assignments and field writes;
- anything inside macro input, such as `sqlx::query!` strings and format strings.

For changes to those, the task loop keeps a short manual check: break the line, and watch a
test fail. `docs/testing.md` lists these cases, and they are the only hand sabotage left.

### 5. Build speed and disk

- **`[profile.dev] debug = "line-tables-only"` and `[profile.dev.package."*"] debug = false`.**
  - Backtraces keep their file and line numbers.
  - Debugger variables are lost, and the owner doesn't debug with gdb.
  - The release profiles are untouched.
- **A sweep script for the shared target directory.** It finds the directory with
  `cargo metadata`, so it follows whatever the machine's config says.
  - **It never runs `cargo clean` there, and never deletes the directory.** Other projects build
    into it, possibly at that very moment.
  - **It deletes stale incremental-compilation directories,** those untouched for 14 days.
    Incremental state is what grew without bound before (63 GB of `debug/incremental` in
    RuneCast's case). Deleting it costs only a slower next build of that crate.
  - **It deletes leftover cargo-mutants scratch copies.**
  - **It prints the directory's size and warns loudly above 150 GB.** Deciding to clear the
    whole shared directory stays a person's call, or an agent's with the owner's OK, as it was on
    2026-10-22.
  - **The local mutants script runs it first,** so the sweep happens regularly without a system
    timer.
  - **My call:** these rules, and their thresholds.
- **Parallel mutant jobs** (section 4).

### 6. Process and docs

**`docs/testing.md`** becomes the one page for:
- nextest commands and profiles;
- the template databases;
- coverage;
- the mutants check, waivers, the baseline and its refresh;
- what stays manual;
- how to do a per-module pass:
  1. Run cargo-mutants on one file or module.
  2. Fix tests or waive.
  3. Refresh the baseline.

**PR bodies** carry the mutants summary in place of the hand-made "mutation → caught-by"
table, plus any manual checks from the list above.

**The agent's standing rule "sabotage every test" becomes:**
- Run the diff check.
- Hand-sabotage only the cases cargo-mutants can't mutate.

This lives in the agent's memory, outside the repo.

## Order of work

Two cracktunes PRs, both tooling-only, with no version bump. **My call:** the gate can't arrive
in the same PR as the tarpaulin removal.
- cargo-mutants treats the line after a deleted line as touched.
- 56 of the tarpaulin attributes sit directly above a one-line `fn … {`, so deleting them pulls
  those functions' whole-body mutants into the diff. They are mostly glue that tarpaulin was
  told to skip.
- With no baseline yet, the PR would fail its own check.
- A workflow can only be dispatched once it is on the default branch.

So the work goes in this order.

**PR A: the tooling, with no gate yet.**
1. **Tarpaulin out** across the repo, the README and the scripts.
2. **The stdout ctor fixed.**
3. **nextest:** its config and the CI switch, with doctests kept.
4. **llvm-cov:** `coverage.yml`.
5. **The dev profile.**
6. **Template databases,** then every crack-core site moved over.
7. **cargo-mutants:**
   - its config and initial exclusions;
   - the full-run workflow, which records only while no baseline exists;
   - the baseline format and the refresh script.
8. **The sweep script.**
9. **The docs for all of the above.**

**Between the PRs:** dispatch the full run on master, and wait for its missed list.

**PR B: the gate.**
1. The diff-check script.
2. The gate script, comparing against the baseline.
3. The waiver-reason check.
4. The PR CI job.
5. The first baseline file, from that full run.
6. The mutants sections of `docs/testing.md`.

**Separately:** the catacombs `.gitignore` PR.

## Left to the builder

The builder decides these, measuring where a number is called for:
- **nextest:** the timeout values, and any test group or thread cap for database tests;
- **the test attribute:** the proc-macro crate's name, and the template naming scheme;
- **sweeping:** the number of templates kept, and the sweep's exact rules;
- **local mutant runs:** the job count, and `--copy-target`;
- **CI:**
  - the diff check's mutants-per-shard figure, and its shard cap;
  - the full run's shard count and schedule, and whether it rotates;
  - how tools are installed;
- **cargo-mutants:** the initial exclusion list, each with its reason;
- **scripts:** the summary format, and the script languages (bash or Python, as the repo's
  `scripts/` already uses).

## Follow-ups, not this arc

- **`crack-core/test_migrations` duplicates `migrations/`, and one file has drifted.** Tests
  then run against a schema that production doesn't have.
- **The per-module chip-away passes,** ordered by risk. Money and economy code comes first,
  then music ops.
- **Making the mutants check a required status check.** It's the owner's call, and it can be
  made later.
- **Splitting crack-core,** which makes mutants cheaper. It's part of the crack-types rethink.
