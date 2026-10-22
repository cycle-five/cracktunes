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
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin
cargo test --doc --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin
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
`crack-core` can use it, because the expansion calls `crate::test_db`. 167 test
sites there use it. `crack-voting` and `cracktunes` (the `crack-cli` package)
stay on `#[sqlx::test]`.

To run them, start a postgres and turn the feature on. The feature lives in
each package that has such tests, so a workspace run names all three, plus
`crack-core/crack-coin` for the CrackCoin tests:

```bash
docker run -d --name crack-pg -p 5432:5432 \
  -e POSTGRES_PASSWORD=mysecretpassword postgres:16

cargo nextest run --workspace \
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin
```

`DATABASE_URL` defaults to `postgresql://postgres:mysecretpassword@localhost:5432/postgres`,
set by a `#[ctor]` in `crack-core/src/db/user.rs` when nothing else has set
it; export your own to point elsewhere.

CI provisions postgres and passes those features, so the database tests do run
on every push. The gate is about the default local experience, not coverage.

### Template databases

`#[sqlx::test]` creates an empty database for every test and runs every
migration into it. `db_test` runs the migrations once, into a template, and
gives each test a copy (`CREATE DATABASE ... TEMPLATE`). The code is
`crack-core/src/test_db.rs`.

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
  --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin \
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

🔑 There is no gate yet. The check on each PR's diff, the baseline and the
waivers arrive in a follow-up PR; this section covers the tooling that exists.

Install it with `cargo install cargo-mutants --version 27.1.0 --locked`, the
version CI uses: the baseline's keys are 27.1.0's mutant descriptions, and
another version may word them differently. Run it only through the wrapper:

```bash
scripts/mutants.sh --list                                 # what would be mutated, no tests run
scripts/mutants.sh -j 1 -f crack-core/src/db/user.rs      # one file
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
  which another session may be editing. (CI's full run does use it, on a
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

### The full run

`mutants-full.yml` runs every mutant across shards and uploads the missed ones
as the `mutants-missed` artifact. It is dispatch-only and exists on master once
this is merged:

```bash
gh workflow run mutants-full.yml                          # 40 shards, 8 at a time
gh workflow run mutants-full.yml -f shards=60 -f max_parallel=8
```

`shards` is 1-256 (default 40); `max_parallel` is how many run at once
(default 8), kept low because the concurrent-job limit is shared with every
other workflow. The shard count will be chosen from the first measured run.
A run with any failed shard uploads no `mutants-missed`, so a partial list can
never become a baseline. A shard also fails when Postgres is down or the disk
filled during its run, since either would quietly shorten its missed list.
Only one full run goes at a time; a second dispatch waits.

### The baseline

`.cargo/mutants-baseline.txt` will list the mutants the tests are known to
miss. It only shrinks. It does not exist yet: the first one is built from the
full run's artifact in the follow-up PR. Entries carry the file and the
description but no line or column, so an unrelated edit above a mutant does not
make a known miss look new.

```bash
scripts/mutants_baseline.py compare --baseline .cargo/mutants-baseline.txt --missed missed.txt
scripts/mutants_baseline.py refresh --baseline .cargo/mutants-baseline.txt --missed missed.txt
```

`compare` exits 1 when the run missed something the baseline does not list.
`refresh` rewrites the baseline from a missed list and refuses to add entries;
`--init` creates the file the first time and refuses if it exists. 🪤 Give
`refresh` the missed list of a COMPLETE full run (the `mutants-missed`
artifact): a list from one file, one module, an `--in-diff` run or one shard
drops every baseline entry outside it.

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
`scripts/mutants.sh` runs it first:

```bash
scripts/sweep_target.py --dry-run     # list what it would delete
scripts/sweep_target.py
```

It deletes exactly two things:

- children of any `incremental/` directory in the target directory (1 to 3
  levels down) untouched for more than 14 days;
- `cargo-mutants-*` scratch copies under the scratch root untouched for more
  than 24 hours, which a killed run leaves behind. `--scratch-root DIR`
  overrides the root.

🔑 It never deletes the target directory itself and never runs `cargo clean`.
The target directory is shared with other projects (`build.target-dir` in
`~/.cargo/config.toml`), so clearing it is a person's call. It also refuses a
directory without a `CACHEDIR.TAG`, and above 150 GiB it only warns.
