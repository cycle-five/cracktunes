# Testing

This is the one page for testing: running the suite, the database tests,
coverage, mutation testing and the build-directory sweep.

```bash
cargo nextest run --workspace      # everything that needs no database
cargo test --doc --workspace       # doctests: nextest does not run them
```

That is the default, and it passes on a clean checkout with nothing running.
Install the runner once with `cargo install cargo-nextest --version 0.9.149 --locked`,
the version CI uses.

## Running tests

[cargo-nextest](https://nexte.st) is the runner. It has three profiles, defined
in `.config/nextest.toml`; pick one with `--profile`.

| Profile | For | What it does |
|---|---|---|
| `default` | local runs | A test is flagged slow after 30 s and killed after 120 s, so a hung test cannot stall the run. |
| `ci` | `build.yml`, `coverage.yml` | The same timeouts, plus `fail-fast = false` (one run reports every failure) and failures repeated at the end of the log. |
| `mutants` | cargo-mutants | The same timeouts, plus `fail-fast = true`: most mutants die to an early test, so stopping at the first failure is a large speed-up. `scripts/mutants.sh` selects it. |

🔑 No profile retries a failed test. A retry turns a flaky test into a green
run, and the flake is then never fixed.

nextest runs every test in its own process, so one that crashes or hangs
takes down only itself. It is not faster than `cargo test` on `crack-core`,
which is one big test binary: the full suite with the database tests took
30.0 s for the test phase on a 20-core machine (2224 passed, 22 skipped).
What it buys is the timeouts, the crash isolation and the profiles.

The same run with the database tests, as CI does it:

```bash
cargo nextest run --profile ci --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests
cargo test --doc --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests
```

The dev profile is tuned for rebuilds: `debug = "line-tables-only"` for the
workspace and no debug info for dependencies. A rebuild after a real edit in
`crack-core` dropped from about 18 s to 12-13 s; a cold build is about 164 s.

## Tests that need postgres

Most of the database tests are `crack-core`'s, and they use a macro of ours
instead of `#[sqlx::test]`:

```rust
#[crack_test_macros::db_test(migrator = "MIGRATOR")]
#[cfg_attr(
    not(feature = "db-tests"),
    ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
)]
async fn test_insert_user(pool: PgPool) { ... }
```

The macro takes the same argument and the same function shapes as
`#[sqlx::test]`; moving a test over changes only the attribute's path. 🪤 Only
a crate can use it once it has `crate::test_db`, because the expansion calls it:
`#[cfg(test)] pub(crate) use crack_test_db::test_db;`. Every db test site in
`crack-core`, `crack-economy`, `crack-rooms`, `crack-guild`, `crack-music` and
`crack-test-db` uses it (13, 57, 35, 10, 51 and 7 sites when counted with
`grep -rh '#\[crack_test_macros::db_test' <crate>/src | wc -l`). `crack-voting` and `cracktunes` (the
`crack-cli` package) stay on `#[sqlx::test]`.

To run them, start a postgres and turn the feature on. The feature lives in
each package that has such tests, so a workspace run names all eight
(`crack-core`, `cracktunes`, `crack-voting`, `crack-test-db`, `crack-economy`,
`crack-rooms`, `crack-guild`, `crack-music`),
plus `crack-core/crack-coin` and `crack-economy/crack-coin` for the CrackCoin
tests:

```bash
docker run -d --name crack-pg -p 5432:5432 \
  -e POSTGRES_PASSWORD=mysecretpassword postgres:16

cargo nextest run --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests
```

`DATABASE_URL` defaults to `postgresql://postgres:mysecretpassword@localhost:5432/postgres`.
`crack-test-db`'s runner applies that default itself, for every crate, when
nothing else has set it (`crack-core`'s `#[ctor]` in `src/db/user.rs` remains
for crack-core tests that read the variable directly). Export your own to
point elsewhere.

CI provisions postgres and passes those features, so the database tests do run
on every push. The gate is about the default local experience, not coverage.

### Template databases

`#[sqlx::test]` creates an empty database for every test and runs every
migration into it. `db_test` runs the migrations once, into a template, and
gives each test a copy (`CREATE DATABASE ... TEMPLATE`). The code is
`crack-test-db/src/test_db.rs`.

- **Templates** are named `crack_tpl_` plus a hash of every migration's
  version and checksum. Editing or adding a migration gives a new name, so a
  stale template is never reused and nothing is invalidated by hand.
- **Building** one happens in the first test that needs it, under a Postgres
  advisory lock so parallel processes don't race. It is built under a
  temporary `_build` name and renamed once complete, so no test copies a
  half-migrated template.
- **Sweeping** keeps the 3 newest templates (by build time) and drops the
  rest, since worktrees on different branches can need different templates.
- **Test databases** are named `crack_test_` plus a hash of the checkout's
  `crack-core` directory and the test's path. The same test in the same
  checkout always gets the same name; two checkouts never share one.
- **Leftovers:** a passing test drops its database. A failing test keeps it,
  for inspection, and the next run of that test drops and replaces it. The
  sweep also drops any test database older than 7 days (a test killed by a
  timeout never cleans up). With `CRACK_MUTANTS` set, which `scripts/mutants.sh`
  does, a failing test's database is dropped too, because mutant runs fail
  tests on purpose.
- **🪤 One run per checkout at a time.** Two runs in one checkout
  (`cargo llvm-cov nextest` beside `cargo nextest run`) share database names.
  A leftover is never force-dropped, so where they collide the second run's
  test waits 5 s and fails with "another run of … is using …" instead of
  ending the first's live database. A psql session open on a kept failure
  stops it the same way. Run the second in another worktree.

Measured on `crack-core`'s db suite: copying a template costs about 25 ms,
against about 950 ms to migrate a fresh database. The suite went from about
35.8 s to about 20 s wall, the summed test time from 689 s to about 370 s, and
the mean db test from 1.22 s to 0.62 s. Peak use is 39 of Postgres's 100
connections.

## Why they are off by default

The bot runs without a database. `crack-core/src/config.rs` skips the pool
entirely when `DATABASE_URL` is unset, logs which features that disables, and
carries on; the deployed instance is running that way right now. A test suite
that hard-fails without postgres contradicts the thing it is testing, and in
practice it meant red tests on every checkout that said nothing about the
change under review: noise that trains you to ignore a red suite.

`#[cfg_attr(..., ignore)]` rather than `#[cfg(...)]` on purpose: the tests are
still **compiled** either way, so they cannot rot against a changed query or
schema without someone noticing at build time.

## Tests that reach the network

Some tests in `crack-testing` make live calls (YouTube). Those are marked
`#[ignore]` on their own and are not covered by `db-tests`. They fail for
reasons unrelated to whatever you changed; run them deliberately, by name.
crack-osint's own HIBP live-call test moved with it to
`cycle-five/crack-osint` and is no longer part of this repository.

## Coverage

Coverage is line coverage from [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov),
which uses LLVM source-based coverage and drives nextest, so the tests run as
they do everywhere else. Locally, with a postgres running:

```bash
cargo install cargo-llvm-cov --version 0.9.1 --locked
cargo install cargo-nextest --version 0.9.149 --locked
rustup component add llvm-tools-preview
cargo llvm-cov nextest --profile ci --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin,crack-test-db/db-tests,crack-economy/db-tests,crack-economy/crack-coin,crack-rooms/db-tests,crack-guild/db-tests,crack-music/db-tests \
  --lcov --output-path lcov.info
```

Drop `--lcov --output-path lcov.info` for a table in the terminal. The last
local run measured 78.41%.

`coverage.yml` runs the same command weekly (Mondays, 05:17 UTC) and on
`workflow_dispatch`, and uploads to codecov. Nothing gates on it. The tool
that proves tests can fail is cargo-mutants, below.

## Mutation testing

Coverage says a line ran, not that a test would notice it breaking.
[cargo-mutants](https://mutants.rs) changes the code (flips a `&&`, replaces a
body with a default) and checks that some test fails. A mutant no test fails on
is *missed*.

### The diff check

The check mutation-tests only the Rust lines a change touched, and fails only
on **new** misses. It is a tool, not a gate (owner's call, 2026-10-25: see "In
CI" for why). Run it when a change's tests most need proving:

```bash
scripts/mutants_check.py --base <the commit the task started from>
scripts/mutants_check.py --help        # every option
```

- It reads commits, never the working tree: `--head` defaults to `HEAD`, and
  `--base` to the merge base with `origin/master`. Both are snapshotted with
  `git archive`.
- It lists the mutants on the lines `base..head` touched, plus the head's
  lines that correspond to what the base side marks (see "New or old"), and
  runs them.
- If any are missed, it reruns just those on the base to tell new from old.
- It prints a summary (`--summary-md FILE` also appends it as Markdown) and
  exits 0 (no new misses), 1 (new misses) or 2 (anything else).

🔑 **How the agent uses it (since 2026-10-25):** not on every task. It used to
run after each task's commit, and on the neutral-messages PR (#636) a local
run was going for 70% of the wall-clock. Implementers waiting on it also ran
up token cost. Now:

- **A reviewer lists, rather than runs.** `scripts/mutants.sh --list
  --in-diff <diff>` takes seconds and shows which mutants a change brings in.
  Read them against the tests to spot a likely gap; that is how #636's final
  review found its last two.
- **Run only what review names.** For a function review names, run a targeted
  check: `scripts/mutants.sh -j 1 --in-diff <diff> -f <file> --re '<fn>'`.
  `--re` alone doesn't filter "delete field" mutants; `--in-diff` with `-f`
  scopes them to the touched lines.
- **The full check** (`mutants_check.py`) is for a change whose logic has
  little else proving its tests can fail.

- **Exit 2 means the environment is broken, not that the tests are weak:**
  Postgres down after a run, "No space left on device" in the logs, the
  unmutated tree failing its tests, a cargo-mutants exit code that contradicts
  its own lists, or lists missing. Fix the cause and rerun. (`--summary-md`
  gets a `broken run: <reason>` line.)
- **The Postgres guard** runs `pg_isready` against `DATABASE_URL`, or, when
  that is unset (as it is locally), the tests' own default URL. Without
  `pg_isready` installed it prints one line and is skipped.
- **Cost:** every mutant is a rebuild, so it is minutes, not seconds. When
  there are misses, a second and smaller run goes to the base. Measured
  (2026-10-22): a real `crack-core` change (5 new mutants and 2 old, one
  untested function plus a touched glue function) took 745 s locally end to
  end, most of it two cold `crack-core` builds.
- **What a change brings in:** cargo-mutants' whole-function mutants span the
  function from its first statement to its last. Changing a statement line
  brings in that function's whole-body mutants; changing a comment or a blank
  line outside the statements brings in nothing.

### New or old

A miss is **old** if the same mutant is also missed on the base commit, and
**new** otherwise. This replaced a recorded list of known misses
(2026-10-22): nothing to keep in step, and no whole-tree run to build it.

- **"The same mutant" is the file plus the mutant's description, with no line
  or column,** so lines that an edit shifts don't turn old misses into new
  ones.
- **Misses are matched as a multiset:** two identical descriptions in one file
  count twice.
- A miss is new unless the base misses a twin on its corresponding lines.
  A mutant the change adds (a new operator, a new function, a second copy of
  one) is new. A rewritten line that keeps its operator (`a > 1` to `a > 2`)
  matches the old line's twin, so if that was missed too, the miss stays old:
  the change didn't make it worse.
- A miss in a file the base doesn't have is new. 🪤 So is one in a renamed and
  modified file: the key carries the path.
- 🔑 **The two runs look at corresponding lines.** cargo-mutants' `--in-diff`
  marks an inserted line alone, but a deleted line's neighbours on both sides,
  so the reversed diff on the base would look at other code than the diff on
  the head. An inserted untested line would then pass as old next to an
  untested twin, and a deletion next to an old miss would fail as new. The
  check marks each side by cargo-mutants' rule, maps each onto the other
  through the diff, and gives each run the union as a synthetic diff
  (`head.diff`, `base.diff`; the change itself is `change.diff`). The head run
  so selects a few more mutants than raw `--in-diff` would.
- **What this gives up:** there is no count of the whole tree's misses until
  each module has had its pass (below).

### Waivers

Some mutants no test can or should catch. Waive one in code or in config, and
always write the reason.

- **In code:** `#[cfg_attr(test, mutants::skip)]` with a `//` reason on the
  line directly above. Any other line that mentions `mutants::skip` (an inner
  `#![cfg_attr(…)]`, another cfg predicate, an attribute wrapped over several
  lines) is a waiver too, and needs the same reason directly above the line
  that holds `mutants::skip`. 🪤 For a wrapped attribute that line is inside
  the attribute, so the `//` reason goes inside it too, not above `#[`. The
  first time a crate uses it, add `mutants = "0.0.4"`
  (the current version, `cargo search mutants`) to that crate's
  `[dev-dependencies]`. 🪤 Not a normal dependency: `cfg_attr(test, ...)`
  compiles the attribute only in tests, so the production build never needs
  the crate.
- **In config:** an element of a key that narrows what is mutated in
  `.cargo/mutants.toml` (`exclude_globs`, `exclude_re`, `examine_globs`,
  `examine_re`, `skip_calls`), one per line, each with a `#` reason (with
  text) on the line directly above.
- **Acceptable reasons:**
  - an equivalent mutant: no test can tell it from the original;
  - logging- or display-only code. 🪤 Never the privacy-critical `Debug`
    impls, such as the coin key or anything that could print an address;
  - performance-only knobs, such as capacities and buffer sizes;
  - live-Discord glue that can't be reached offline. Prefer moving the
    decision into a pure function.
- `scripts/check_waivers.py` fails when a waiver has no reason (and on the
  bare `#[mutants::skip]`, which would make `mutants` a release dependency).
  It runs in the `scripts` job of `lint.yml`; run it yourself from the
  repository root.

### In CI

`mutants-diff.yml` runs the same check as the Mutants job, **only on a pull
request labelled `mutants`** (`mutants-pr.yml`).

- **Adding the label starts a run,** and each push while it is on starts
  another.
- **It is skipped** when a PR changes no `.rs` file.
- 🔑 **It is advisory.** Master's required checks are `Build / Build (stable)`
  and `Lint / Fmt and clippy (stable)`. From 2026-10-23 to 10-25, `Mutants /
  Mutants verdict` was the required check, and it ran on every PR.
  - **Why that changed:** each `crack-core` mutant rebuilds `crack-core` and
    runs its whole test binary, about 2 minutes on CI. #636 (79 files) hit
    the 24-shard cap and took about 3.5 hours and 18 runner-hours. Two weeks
    of the per-task and per-PR runs cost about 20M tokens of implementer time
    and 45 runner-hours, and found about 17 real test gaps.
  - **When to reconsider:** after `crack-core` is split into smaller crates,
    where a mutant costs seconds.
- Master's protection requires a branch to be up to date before it merges,
  so a PR behind master is updated and its checks run again.
- Mutants are dealt out at 6 per shard, at most 24 shards, 6 running at once
  (the account's concurrent-job limit is shared by every repo).
- 🔑 **The `Mutants verdict` job gives the verdict, not the shards.** A shard
  sees only its share of the mutants, so two copies of one mutant in two
  shards would each be excused by the same single base copy. Each shard
  uploads its lists (`--shard-result`) and fails only on a broken run; the
  verdict job merges them (`mutants_check.py --collect`), compares once, and
  fails on new misses. A shard with misses also uploads its output
  (`mutants-diff-<n>`) to read the missed mutants' diffs.
- **Cost, measured 2026-10-23** (a throwaway `crack-core` smoke on #628:
  7 mutants in 2 shards, 6 missed; Build, beside it, took 5-10 minutes):
  - a `crack-core` shard first builds and tests the unmutated tree:
    260-290 s of build cold, about 105 s with the dependencies restored by
    rust-cache, plus 51 s of tests. 🔑 A cache a PR saves is scoped to that
    PR, so master saves the one a new PR restores (`mutants-warm.yml`, the
    **Mutants cache** job, crack-core only). A PR's first run is still cold
    when master has no cache for its lockfile and toolchain: a dependency
    PR, the first push after a new stable, or a PR opened before master's
    job finished;
  - each missed `crack-core` mutant then costs 160-170 s (105-120 s build,
    51 s test); a caught one pays the same build and stops at its first
    failing test;
  - a shard with misses also runs the base side: one more unmutated run
    (about 2.6 minutes) plus 160-170 s per miss the base has a twin of;
  - the smoke's 4-mutant shard took 22.8 minutes on the PR's first run
    (cold), 19.6 on a re-run (warm), and 24.5 before the shards shared one
    cached target dir; its 3-mutant shard 12-14 minutes; Plan and the
    verdict under 20 s each.
- 🪤 **So the budget holds for some PRs, not all.** A `crack-core` PR whose
  mutants are mostly caught roughly fits it. That is an estimate, since the
  smoke caught none: a shard of 6 caught `crack-core` mutants should take
  about 15-20 minutes. One whose shards carry several misses (old ones count
  too: they are rerun on the base) takes about 30-40 minutes a shard. A
  `crack-types` shard is a few minutes (2026-10-22: 3.5-4.5 minutes).
- In CI, `CRACK_MUTANTS_SHARED_TARGET=1` makes `scripts/mutants.sh` keep the
  job's absolute `CARGO_TARGET_DIR` (the workspace's `target/`, which
  rust-cache restores) instead of a target per scratch copy. Locally nothing
  changes.
- The local per-task check is the real gate; the job is the backstop for what
  slipped past it.

### Module passes

The check only sees touched code, so the old misses are worked down on
purpose, a module at a time, each in its own PR.

```bash
gh workflow run mutants-full.yml -f files='crack-economy/src/**'
gh workflow run mutants-full.yml -f files='crack-core/src/db/**,crack-core/src/guild/**' -f shards=12
```

- `files` is required: one glob, or several separated by commas. An empty
  value fails the run, because the whole tree takes about 35 hours.
- `shards` (1-256, default 8) and `max_parallel` (default 8, kept low because
  the concurrent-job limit is shared with every other workflow).
- **Timing:** about 2 minutes per mutant on a GitHub runner (2026-10-22), so a
  500-mutant module at 8 shards is about 2 hours. Size it first with
  `scripts/mutants.sh --list -f '<glob>'`, and keep a shard under about 120
  mutants: the job cap is 340 minutes, one timed-out shard throws away a
  multi-hour pass, and runners vary by 25-30%.
- **Artifact:** `mutants-missed`, the combined missed list. Fix what the tests
  should catch; waive the rest, with a reason.
- A run with any failed shard uploads no `mutants-missed` (only
  `mutants-failed-<n>` for debugging), so a partial list is never mistaken for
  a full one. A shard also fails when Postgres is down or the disk filled
  during its run.
- One pass at a time; a second dispatch waits. There is no schedule.

### Tooling notes

Install it with `cargo install cargo-mutants --version 27.1.0 --locked`, the
version CI uses: the new-or-old key is 27.1.0's mutant description, and
another version may word it differently. Run it only through the wrapper:

```bash
scripts/mutants.sh --list                                 # what would be mutated, no tests run
scripts/mutants.sh -j 1 -f crack-music/src/db/user.rs      # one file
scripts/mutants.sh -j 1 -f 'crack-core/src/messaging/**'  # one module
scripts/mutants.sh -j 1 --in-diff my.diff                 # only the lines in a diff
```

`-f` / `--file` takes a file or glob, `-D` / `--in-diff` a diff file, and
`--list` shows the mutants without testing them. Any other cargo-mutants
argument passes through. 🪤 The wrapper runs from the repository root, so a
relative path (`--in-diff my.diff`, `-o out`) is read from there, not from
the directory you called it in.

- **It refuses `--in-place`.** cargo-mutants copies the tree to a scratch
  directory and mutates the copy; `--in-place` would mutate your working tree,
  which another session may be editing. (CI's module pass does use it, on a
  disposable runner.)
- **Scratch location:** `$HOME/.cache/cargo-mutants`, or `$CRACK_MUTANTS_SCRATCH`
  if set. Not `/tmp`, which is a small tmpfs in RAM. Each copy has its own
  `target/` (up to about 5.4 GiB), so disk is the limit, not CPU.
- **Use `-j 1`.** Each mutant's tests already use every core, and each job pays
  its own cold baseline build, which takes longer than a mutant does. (Measured
  before mutants tested only their own crate: 13 mutants took 509 s at `-j 1`,
  536 s at `-j 2` and 603 s at `-j 4`.)
- **Cost:** the workspace has 7,732 mutants (6,598 in `crack-core`), so run a
  file or module, not everything. A fresh copy first builds and tests the
  unmutated crates: for `crack-core` about 170 s of build and 20 s of tests.
  After that a missed `crack-core` mutant took 15 s to build and 24 s to test
  (a caught one stops at its first failing test), and a `crack-types` mutant
  about 2 s in all.
- **Configuration** is `.cargo/mutants.toml`: the nextest runner, the
  `mutation-testing` feature, a lean `mutants` cargo profile and
  `gitignore = true` (so `.env` stays out of the copies). The wrapper sets
  `NEXTEST_PROFILE=mutants`, `CRACK_MUTANTS=1` and `SQLX_OFFLINE=true`.
- **Each mutant runs its own crate's tests,** with `mutation-testing`, which
  every member except `crack-test-macros` defines: `db-tests` and `crack-coin`
  where a crate has them, nothing elsewhere (🪤 a new member needs it too). A
  `crack-core` mutant that only a `cracktunes` or `crack-web` test would catch
  shows as missed.
  Leave the unmutated baseline on (no `--baseline skip`): with a dead
  Postgres, every mutant would look caught.

### What stays manual

cargo-mutants cannot mutate these, so a change to one still needs a hand check:
break the line and watch a test fail.

- SQL text, including `WHERE` guards;
- the order in which locks are taken;
- plain assignments and field writes;
- anything inside macro input, such as `sqlx::query!` strings and format
  strings.

## The target sweep

Incremental compilation state grows without bound, and mutant runs leave
scratch copies behind. `scripts/sweep_target.py` clears both, and
`scripts/mutants.sh` runs it first. (`scripts/mutants_check.py` runs it once
itself and sets `CRACK_MUTANTS_NO_SWEEP=1` for its several `mutants.sh`
calls.)

```bash
scripts/sweep_target.py --dry-run     # list what it would delete
scripts/sweep_target.py
```

It deletes exactly three things:

- children of any `incremental/` directory in the target directory (1 to 3
  levels down) untouched for more than 14 days;
- `cargo-mutants-*` scratch copies under the scratch root untouched for more
  than 24 hours, which a killed run leaves behind;
- `check-*` output directories of `mutants_check.py` under the scratch root
  untouched for more than 14 days.

`--scratch-root DIR` overrides the root. The scratch root is swept even when
the target directory does not exist yet.

🔑 It never deletes the target directory itself and never runs `cargo clean`.
The target directory is shared with other projects (`build.target-dir` in
`~/.cargo/config.toml`), so clearing it is a person's call. It also refuses a
directory without a `CACHEDIR.TAG`, and above 150 GiB it only warns.
