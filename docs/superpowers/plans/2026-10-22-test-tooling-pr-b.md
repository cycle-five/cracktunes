# Test tooling PR B: the mutants gate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every change's diff is mutation-tested; a mutant no test catches blocks the change unless the code already had that miss on the base commit, or it is waived with a written reason.

**Architecture:** One stdlib-Python script, `scripts/mutants_check.py`, snapshots the head and base commits with `git archive`, runs cargo-mutants (through `scripts/mutants.sh`) on the head's diff, reruns only the misses on the base, and compares the two as multisets of "file + description". The same script runs in the agent's task loop and in a sharded PR CI job. A second script checks that every waiver carries a reason. PR A's full-run workflow becomes a dispatch-only tool for deliberate per-module passes, and the baseline script is retired.

**Tech Stack:** Python 3 (stdlib only, `unittest`), bash, cargo-mutants 27.1.0, cargo-nextest 0.9.149, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-10-22-test-tooling-design.md`, as amended 2026-10-22 (the box at the top; section 4 "New or old" and "Module passes"; "Order of work", PR B).

## Global Constraints

**Git and the repo**
- Never `git add -A` or `git add .`. Add files by name.
- Every commit ends with exactly this trailer line, and no other `Co-Authored-By` line:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- No version bump. This PR ships no bot behaviour. Release profiles are not touched.
- No Rust source changes in this PR, apart from Task 4's throwaway smoke commit, which is
  reverted before the task ends.

**Build directories and mutants**
- **Never delete, `cargo clean`, or write code that deletes the shared target directory**
  `/home/lothrop/.cache/cargo-target`.
- **Never pass `--in-place` to cargo-mutants on this machine.** Always go through
  `scripts/mutants.sh`, which refuses it.
- **Scratch lives under `$HOME/.cache/cargo-mutants`** (or `$CRACK_MUTANTS_SCRATCH`), never
  `/tmp`, which is a 16 GB tmpfs in RAM. Small test fixtures (a few KB of source) may use
  `tempfile`; anything cargo builds goes under the scratch root.
- The check works on commits, never on the working tree: another agent may be editing it.

**Docker and the database**
- Never build Docker images locally. For the local Postgres use `docker --context default …`.
- The local test database is the container `pg`, Postgres 16, on `localhost:5432`; tests default
  to `postgresql://postgres:mysecretpassword@localhost:5432/postgres`.

**Scripts**
- Python scripts are stdlib only and run with the runner's `python3`. Tests are `unittest`
  files named `scripts/test_*.py`, run by `python3 -m unittest discover -s scripts -p 'test_*.py'`
  (lint.yml's `scripts` job).
- Exit codes for both new scripts: **0** pass, **1** the thing the script checks failed (new
  misses / missing reasons), **2** anything else (bad arguments, a broken environment, a tool
  error). A broken input must never look like a pass or like an ordinary failure.

**Workflows**
- `permissions: contents: read`. Postgres pinned: `postgres-version: "16"` on
  `ikalnytskyi/action-setup-postgres@v8`. Reuse `actions/checkout@v7`, `Swatinem/rust-cache@v2`,
  `actions/upload-artifact@v7`. Tools via `taiki-e/install-action@v2`:
  `cargo-nextest@0.9.149,cargo-mutants@27.1.0`.

## Review Focus

1. **A miss in a file the base doesn't have** (a new file, or a renamed one) must count as new,
   and the base run must not error on it. → Task 2, `test_new_file_miss_is_new`.
2. **Descriptions full of regex metacharacters** (`->`, `<UserId>`, `&&`, `||`, `vec![]`,
   `Default::default()`, `+=`, `*`) must select exactly their twin on the base, in Rust's regex
   syntax, not Python's. → Task 1 escaping tests, Task 2 `test_regex_selects_twin_on_base`.
3. **A broken environment** (Postgres down, disk full, the unmutated tree failing its tests on
   either commit) must exit 2, never 0 or 1. → Task 2 `test_baseline_failure_exits_2`, and the
   guards.
4. **A diff with no `.rs` changes** (dependabot's lockfile PRs, docs PRs) must succeed in seconds
   without building anything, locally and in CI. → Task 2 `test_no_rust_changes`, Task 4's plan
   job.
5. **Two identical descriptions in one function** (two `replace && with ||`): one missed copy on
   the base and two on the head is exactly one new miss. → Task 1 `test_split_counts_copies`.

---

### Task 1: The check's core — keys, new/old split, Rust regex names, summary

Pure functions only; Task 2 adds the pipeline around them. The key logic moves here from
`scripts/mutants_baseline.py`, which is deleted along with its tests.

**Files:**
- Create: `scripts/mutants_check.py`, `scripts/test_mutants_check.py`
- Delete: `scripts/mutants_baseline.py`, `scripts/test_mutants_baseline.py`

**Interfaces:**
- Produces (Task 2 uses all of these, by these names):
  - `key(name: str) -> str` — `"file:L:C: desc"` → `"file: desc"`; raises `ValueError` on any
    other shape. Same regex as the old script: `^(.+?):\d+:\d+: (.+)$`.
  - `split_new_old(head_missed: list[str], base_missed: list[str]) -> tuple[list[str], list[str]]`
    — returns `(new, old)`, both lists of full head names in head order. Each base key copy
    excuses one head miss with the same key.
  - `rust_escape(text: str) -> str` — backslash before each of `\.+*?()|[]{}^$#&-~`, every other
    character literal (this is the set `regex::escape` escapes in Rust).
  - `name_regex(name: str) -> str` — `"^" + rust_escape(file) + ":[0-9]+:[0-9]+: " + rust_escape(desc) + "$"`.
  - `read_names(path: str) -> list[str]` — the non-blank lines of a cargo-mutants list file
    (`missed.txt`, `caught.txt`, …), each validated with `key()`; a missing file raises
    `FileNotFoundError` (a run that wrote no list is broken, not empty).
  - `summary(counts: dict[str, int], new: list[str], old: list[str], timeouts: list[str]) -> str`
    with `counts` keys `caught`, `missed`, `unviable`, `timeout`.

- [ ] **Step 1: Write the failing tests** in `scripts/test_mutants_check.py`, in the style of the
  old `test_mutants_baseline.py` (fixture names from `cargo mutants --list`). Move the old
  file's `key()` tests over. New tests:
  - `test_split_counts_copies`: head `[AND1, AND2]` (two `replace && with || in voice_earners` at
    different lines), base `[AND_BASE]` (same key, another line) → `new == [AND2]`, `old == [AND1]`.
  - `test_split_line_moves_are_old`: head `["f.rs:90:5: replace g -> bool with true"]`, base
    `["f.rs:12:5: replace g -> bool with true"]` → `new == []`, `old` has the head name.
  - `test_split_other_file_is_new`: same description, different file → new.
  - `test_rust_escape`: `rust_escape("replace voice_earners -> Vec<UserId> with vec![]")` ==
    `r"replace voice_earners \-> Vec<UserId> with vec!\[\]"`;
    `rust_escape("a && b || c += d * e.f()")` == `r"a \&\& b \|\| c \+= d \* e\.f\(\)"`;
    `rust_escape("Default::default()")` == `r"Default::default\(\)"`.
  - `test_name_regex`: `name_regex("crack-core/src/x.rs:48:5: replace f -> u32 with 0")` ==
    `r"^crack\-core/src/x\.rs:[0-9]+:[0-9]+: replace f \-> u32 with 0$"`; and Python's `re`
    (a stand-in that agrees with Rust on this subset) matches the same description at
    `:7:1:` and rejects it with a suffix (`… with 0 in g`) and in another file.
  - `test_read_names_missing_file_raises` and `test_read_names_rejects_garbage` (a line with no
    `:L:C:` → `ValueError`).
  - `test_summary_*`: for new `[N]`, old `[O]`, timeouts `[T]`, counts `{caught: 5, missed: 2,
    unviable: 1, timeout: 1}` the text is exactly:
    ```
    cargo-mutants: 5 caught, 2 missed, 1 unviable, 1 timed out
    NEW misses (catch each with a test, or waive it with a reason):
      N
    Old misses in the touched code (missed on the base commit too; not blocking):
      O
    Timed out (counted as caught):
      T
    FAIL: 1 new miss
    ```
    Empty sections are left out; with no new misses the last line is `PASS: no new misses`;
    with two it is `FAIL: 2 new misses`.
- [ ] **Step 2: Run them and watch them fail** (`ModuleNotFoundError` / missing functions):
  `python3 -m unittest scripts.test_mutants_check -v` from the repo root, or
  `python3 -m unittest discover -s scripts -p 'test_mutants_check.py' -v`.
- [ ] **Step 3: Implement the six functions** in `scripts/mutants_check.py`, with a module
  docstring that says what the check is for (spec section 4, "New or old"). Delete
  `scripts/mutants_baseline.py` and `scripts/test_mutants_baseline.py` with `git rm`.
- [ ] **Step 4: Run the scripts suite:**
  `python3 -m unittest discover -s scripts -p 'test_*.py'` → all OK.
- [ ] **Step 5: Commit** (`git add scripts/mutants_check.py scripts/test_mutants_check.py`, plus
  the `git rm`s): `scripts: mutants_check core (new vs old misses); retire mutants_baseline`.

### Task 2: The check's pipeline and command line

**Files:**
- Modify: `scripts/mutants_check.py`, `scripts/test_mutants_check.py`
- Create: `scripts/test_mutants_check_e2e.py`
- Modify: `.config/nextest.toml` (the `mutants` profile only)

**Interfaces:**
- Consumes: Task 1's functions; `scripts/mutants.sh` unchanged (it `cd`s to this repo's root,
  so every path handed to it must be absolute).
- Produces: the command line Tasks 4 and 6 document and call:
  ```
  scripts/mutants_check.py [--base REV] [--head REV] [--shard K/N]
                           [--output DIR] [--summary-md FILE] [--repo DIR]
  ```
  - `--head` defaults to `HEAD`. `--base` defaults to `git merge-base <head> origin/master`,
    falling back to `master` when there is no `origin/master`.
  - `--shard K/N` passes `--shard K/N --sharding round-robin` to the head run only.
  - `--output` defaults to `<scratch>/check-<head sha, 12 chars>-<unix time>`, where `<scratch>`
    is `$CRACK_MUTANTS_SCRATCH` or `$HOME/.cache/cargo-mutants`. An existing non-empty
    `--output` exits 2.
  - `--summary-md FILE` appends the summary to FILE inside a ```` ```text ```` fence, under
    a `### cargo-mutants diff check` heading (CI passes `$GITHUB_STEP_SUMMARY`).
  - `--repo DIR` (default: the repo that holds the script) is where every `git` command runs;
    the e2e test points it at a fixture repo.
  - `main(argv: list[str] | None = None) -> int`.

The pipeline (each numbered line is one function; names are the implementer's):
1. Resolve `--head` and `--base` to full SHAs (`git rev-parse --verify REV^{commit}`); a bad
   rev exits 2.
2. `git diff --no-ext-diff --no-color <base> <head> -- '*.rs'` → `<output>/head.diff`. If it is
   empty, print `no Rust changes between <base12> and <head12>` and exit 0 without building.
3. Snapshot the head: `git archive <head>` extracted into `<output>/head-tree` (tracked files
   only, exactly that commit).
4. Run `scripts/mutants.sh -d <head-tree> --in-diff <output>/head.diff -j 1 --output <output>/head`
   (+ the shard flags). cargo-mutants exit codes 0, 2 and 3 are results; 4 (the unmutated tree
   fails its tests) and anything else exit 2 with the cargo-mutants exit code in the message.
5. **Environment guards** after every cargo-mutants run, before the exit code is trusted:
   - if `DATABASE_URL` is set and `pg_isready` is on PATH, `pg_isready --dbname="$DATABASE_URL"`
     must succeed;
   - no file under `<out>/mutants.out/log/` may contain `No space left on device`.
   Either failure exits 2, naming which. (🪤 Never grep for SIGKILL: nextest kills timed-out
   tests with it.)
6. Read `missed`, `caught`, `unviable`, `timeout` lists from `<output>/head/mutants.out/`. No
   misses → print the summary and exit 0.
7. **The base run, for the misses only:**
   - `git diff --no-ext-diff --no-color <head> <base> -- '*.rs'` (the reversed diff) →
     `<output>/base.diff`; snapshot the base into `<output>/base-tree`.
   - Keep only the misses whose file exists in `base-tree`; the rest are new without a run.
   - `scripts/mutants.sh -d <base-tree> --in-diff <output>/base.diff --list` with one
     `--re <name_regex(miss)>` per remaining miss. 🔑 `--re` alone is not enough: cargo-mutants
     27.1 doesn't apply it to "delete field" mutants (`visit.rs:747`), but `--in-diff` does
     apply to them, so the reversed diff keeps those extras to the touched code.
   - If the list is empty, the base misses are `[]`. Otherwise run the same selection with
     `-j 1 --output <output>/base`, apply step 4's exit-code rule and step 5's guards, and read
     its `missed.txt`.
8. `split_new_old(head_missed, base_missed)`; print `summary(...)`; write `--summary-md` if given;
   exit 1 if any new miss, else 0.
9. Always delete `head-tree` and `base-tree` on the way out (also on error); keep the
   `mutants.out` directories for inspection. Print the output directory's path.

- [ ] **Step 1: Write the failing unit tests** in `scripts/test_mutants_check.py`, with the git
  and cargo-mutants calls behind one seam the tests can replace (a small runner function or
  class, the implementer's choice; the tests must not need cargo):
  - `test_no_rust_changes`: an empty head diff → exit 0, the message, and no cargo-mutants call.
  - `test_bad_rev_exits_2`.
  - `test_output_not_empty_exits_2`.
  - `test_baseline_failure_exits_2`: the fake head run exits 4 → `main` returns 2.
  - `test_guard_disk_full_exits_2`: the fake head run exits 2 and leaves a log containing
    `No space left on device` → 2.
  - `test_new_file_miss_is_new`: a head miss in a file absent from the base tree → new, and the
    base run is not called when no other miss remains.
  - `test_base_list_empty_skips_base_run`: the fake `--list` prints nothing → no base run,
    every miss new.
  - `test_base_call_args`: the base `--list` call carries `--in-diff <abs base.diff>`, `-d <abs
    base-tree>` and exactly one `--re` per remaining miss, equal to `name_regex(miss)`.
- [ ] **Step 2: Watch them fail.**
- [ ] **Step 3: Implement the pipeline and `main`.** `argparse` with a description that says the
  check reads commits, not the working tree, and the three exit codes.
- [ ] **Step 4: Unit tests pass**, and the whole scripts suite is OK.
- [ ] **Step 5: Write the end-to-end tests** in `scripts/test_mutants_check_e2e.py`. They skip
  (`unittest.skipUnless`) unless `cargo mutants --version` succeeds, so lint.yml's scripts job
  (no Rust) skips them. Set `CRACK_MUTANTS_SCRATCH` to a fresh directory under
  `$HOME/.cache/cargo-mutants/e2e-<pid>` and remove it afterwards. Each test builds a fixture git
  repo (in `tempfile`, a few KB) holding one lib crate `fixture`, no workspace, no
  `.cargo/mutants.toml`, and commits `base`:
  ```rust
  pub fn legacy(x: u32) -> u32 {
      x + 1
  }

  pub fn covered(x: u32) -> u32 {
      x * 2
  }

  #[cfg(test)]
  mod tests {
      #[test]
      fn covered_doubles() {
          assert_eq!(super::covered(3), 6);
      }
  }
  ```
  - `test_touching_legacy_reports_old`: the head commit adds a comment line inside `legacy`'s
    body. Expect exit 0; the output lists `replace legacy -> u32 with 0` (and `with 1`) under
    "Old misses"; `PASS: no new misses`.
  - `test_untested_new_function_is_new`: the head commit also adds
    `pub fn fresh(x: u32) -> bool { x > 3 }` with no test. Expect exit 1, with
    `replace fresh -> bool with true` under `NEW misses`.
  - `test_regex_selects_twin_on_base`: the head commit inserts five blank lines above `legacy`
    and a comment inside it, so every line number moves; the legacy misses are still old.
  - `test_deleting_the_test_makes_new`: the head commit deletes `covered_doubles` and adds a
    comment inside `covered`. `replace covered -> u32 with 0` was caught on the base, so it is
    NEW: exit 1.
- [ ] **Step 6: Run the e2e tests** (`python3 -m unittest scripts.test_mutants_check_e2e -v`);
  they pass. Record their wall time in the report.
- [ ] **Step 7: Run the check for real on this repo**, on a throwaway local commit. (🪤 Not on
  older commits: anything before PR A has no `.cargo/mutants.toml` and no `mutation-testing`
  feature, so its database tests would be ignored and the result meaningless.)
  - `git switch -c tmp/check-smoke`; in `crack-core/src/economy/earn.rs`, add a comment line
    inside the body of `snapshot` (its whole-body mutants were missed in PR A's spike) and add
    `pub fn smoke_untested(x: u32) -> bool { x > 3 }` with no test; commit.
  - `scripts/mutants_check.py --base <this branch's HEAD> --head tmp/check-smoke`.
  - Expected: exit 1; `smoke_untested`'s mutants under NEW; `snapshot`'s whole-body mutants
    under Old. Report the summary, the exit code and the wall time. A 2 is a bug to fix here.
  - `git switch -` back and `git branch -D tmp/check-smoke`. Nothing from it is committed to
    this branch.
- [ ] **Step 8: `.config/nextest.toml`:** in `[profile.mutants]` set `retries = 1`, with a
  comment: under mutation a flaky failure makes a mutant look caught, the dangerous direction;
  a retry means a test must fail twice to catch one. The other profiles keep `retries = 0`, and
  the file's top comment changes to say "No profile except `mutants` retries …" with the reason.
- [ ] **Step 9: Commit** the script, both test files and `.config/nextest.toml`:
  `scripts: mutants_check pipeline — head diff, base rerun of misses, guards`.

### Task 3: The waiver-reason check

**Files:**
- Create: `scripts/check_waivers.py`, `scripts/test_check_waivers.py`
- Modify: `.github/workflows/lint.yml` (the `scripts` job)

**Interfaces:**
- Produces: `scripts/check_waivers.py [--root DIR]` (default: the repo root above the script).
  It checks the files `git ls-files '*.rs'` lists under `--root`, plus `.cargo/mutants.toml`.
  Prints one `path:line: message` per problem, then `N waiver(s) checked, M problem(s)`.
  Exits 0 / 1 / 2 per the Global Constraints. `main(argv=None) -> int`.

Rules (the messages are the implementer's, but must name the rule):
1. **Rust:** a line whose stripped text starts with `#` and matches
   `#\s*\[\s*mutants\s*::\s*skip` (the bare form) is a problem: "write
   `#[cfg_attr(test, mutants::skip)]`" (the bare form makes `mutants` a release dependency).
2. **Rust:** a line matching `#\s*\[\s*cfg_attr\s*\(\s*test\s*,\s*mutants\s*::\s*skip` needs a
   reason: the line directly above it, stripped, must start with `//` but not `///` or `//!`,
   and have text after the `//`. Lines that are themselves comments are never waivers.
3. **Config:** in `.cargo/mutants.toml`, every element of the `exclude_globs` and `exclude_re`
   arrays must sit on its own line, directly below a `#` comment line. A one-line array
   (`exclude_re = ["x"]`) counts as one element and needs the comment directly above its key.
   A missing file is fine (no waivers); unreadable TOML exits 2.

- [ ] **Step 1: Failing tests** (fixture trees under `tempfile`, each initialised as a git repo
  so `git ls-files` works): reasoned `cfg_attr` waiver → 0; no comment → 1; a `///` doc
  comment only → 1; `//` with nothing after → 1; bare `#[mutants::skip]` → 1; a commented-out
  `// #[mutants::skip]` → not a waiver; a `mutants.toml` element without a comment → 1; the
  repo's own current `.cargo/mutants.toml` content → 0; a one-line `exclude_re = ["x"]` with a
  comment above → 0, without → 1.
- [ ] **Step 2: Watch them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Tests pass; the real repo passes:** `python3 scripts/check_waivers.py` → exit 0.
- [ ] **Step 5: lint.yml:** add a step after the unit tests in the `scripts` job,
  `name: Every mutants waiver has a reason`, `run: python3 scripts/check_waivers.py`. Its
  `actions/checkout` must fetch enough for `git ls-files` (the default is fine).
- [ ] **Step 6: Commit:** `scripts: check_waivers — every mutants waiver carries a reason`.

### Task 4: The PR CI job, and a live smoke

**Files:**
- Create: `.github/workflows/mutants-diff.yml` (`on: workflow_call`)
- Modify: `.github/workflows/ci_workflow.yml` (a `mutants` job calling it)

**Interfaces:**
- Consumes: `scripts/mutants_check.py` (Task 2's command line), `scripts/mutants.sh --list`.

The workflow:
- **Caller** (`ci_workflow.yml`): job `mutants`, `name: Mutants`,
  `if: github.event_name == 'pull_request'`, `permissions: contents: read`, `uses:
  ./.github/workflows/mutants-diff.yml`. No `needs`: it runs beside Build and Lint.
- **Job `plan`:**
  - checkout `ref: ${{ github.event.pull_request.head.sha }}`, `fetch-depth: 0` (🔑 not the
    default merge ref: the diff must be the PR's own change);
  - base = `git merge-base ${{ github.event.pull_request.base.sha }} <head sha>`;
  - if `git diff --quiet <base> <head> -- '*.rs'` → outputs `count=0`, and stop: no Rust, no
    cargo install;
  - otherwise install stable Rust and the two tools, then count
    `scripts/mutants.sh --list --in-diff <abs diff>` lines; `shards = ceil(count / 6)`, at
    least 1, at most 24; write the count and shard count to the step summary;
  - outputs: `base`, `head`, `count`, `shards`, `matrix` (`[0..shards-1]`).
- **Job `shard`:** `needs: plan`, `if: needs.plan.outputs.count != '0'`, matrix over the shard
  numbers, `fail-fast: false`, `max-parallel: 6` (🔑 the account's 20 concurrent jobs are
  shared with every repo), `timeout-minutes: 120`. The same setup steps as `mutants-full.yml`'s
  shard job (yt-dlp, stable Rust, Postgres 16, rust-cache with `shared-key: mutants`, the two
  tools), then:
  `python3 scripts/mutants_check.py --base <base> --head <head> --shard <k>/<shards> --summary-md "$GITHUB_STEP_SUMMARY"`
  with `DATABASE_URL` from the Postgres step, `PG_USER`, `PG_PASSWORD` as in
  `mutants-full.yml`, and `--output "$RUNNER_TEMP/mutants-check"`. On failure, upload that
  directory as `mutants-diff-<k>` (`if: failure()`, `if-no-files-found: ignore`).
- A header comment: what the job gates, the budget (a typical PR of up to about 20 mutants
  should finish within about 15 minutes of Build; big feature PRs take longer, accepted), why
  6 per shard, why the PR head and not the merge ref, and that the local per-task check is the
  real gate and this is the backstop.

- [ ] **Step 1: Write both workflow changes.** Check them with
  `python3 -c "import yaml"` if PyYAML is available, else `actionlint` if installed, else by
  careful reading; say which in the report.
- [ ] **Step 2: Commit:** `ci: mutants diff check on pull requests`.
- [ ] **Step 3: Push the branch and open a DRAFT PR** (`gh pr create --draft`, base `master`,
  title `Test tooling, part 2: the mutants gate`, body: one paragraph saying it is in progress).
  Report the PR number. PR B's own diff has no `.rs` change, so check that the run's `Mutants`
  plan job succeeds with `count=0` and the shard job is skipped.
- [ ] **Step 4: Live smoke, with a throwaway commit.**
  - Locally find a function in `crack-types` with a missed whole-body mutant:
    `scripts/mutants.sh -j 1 -f crack-types/src/saved_track.rs` (PR A measured 2 missed of 6
    there).
  - Commit `smoke: throwaway, reverted before merge` that (a) adds a comment line inside that
    function's body and (b) adds `pub fn smoke_untested(x: u32) -> bool { x > 3 }` to the same
    file with no test. Push.
  - Expected: the Mutants job fails; its summary lists the `smoke_untested` mutants under NEW and
    the legacy function's whole-body mutant under Old. Record the run ID, the wall time per job,
    and the summary text in the report.
  - Then `git revert` the smoke commit (message `smoke: revert`), push, and check that the next
    run's Mutants job passes with `count=0`.
  - If the smoke shows a defect, fix it, then repeat the smoke once.
- [ ] **Step 5: Commit** any fixes the smoke needed (named in the report).

### Task 5: The module-pass workflow

**Files:**
- Modify: `.github/workflows/mutants-full.yml`

- `name: Mutants (module pass)`. Inputs:
  - `files` (string, **required**, no default): one glob or several separated by commas, each
    passed as its own `-f`. An empty or whitespace-only value fails the plan job with
    `::error::a module pass needs a file glob (the whole tree takes about 35 hours)`.
  - `shards` default `8`; `max_parallel` default `8`. Validation as now (1–256).
- The shard job passes the `-f` flags to its `cargo mutants` call. Everything else in the
  shard and collect jobs stays (the guards, completeness checks, `mutants-missed` artifact).
- Delete the collect job's "Compare with the baseline" step.
- Rewrite the header comment: what a module pass is (spec, "Module passes"), how to dispatch
  one (`gh workflow run mutants-full.yml -f files='crack-core/src/economy/**'`), and the timing
  arithmetic for a module (about 2 minutes per mutant on CI, so 500 mutants at 8 shards is
  about 2 hours). Remove every mention of the baseline, PR B, the 35-hour full run as a plan,
  and the schedule. Keep the 🔑/🪤 notes that still apply (in place on a disposable runner,
  exit codes, incomplete runs, round-robin, no `--baseline skip`, the guards, the artifact
  🪤).

- [ ] **Step 1: Edit the workflow.**
- [ ] **Step 2: Check it** as in Task 4 Step 1, and `git grep -n "baseline" .github/` shows
  only the cargo-mutants "unmutated baseline" meaning.
- [ ] **Step 3: Commit:** `ci: mutants-full becomes the module-pass tool`.

### Task 6: Docs

**Files:**
- Modify: `docs/testing.md` (the "Mutation testing" section and its subsections), `README.md`
  only if it mentions the baseline or the full run.

`docs/testing.md` "Mutation testing" becomes, in this order:
1. What a mutant and a miss are (keep the opening paragraph).
2. **The diff check:** the command line (Task 2), what it does in four lines, its exit codes,
   its cost (it rebuilds per mutant; misses cost a second, smaller run on the base), and
   **how the agent uses it:** after each task's commit, run
   `scripts/mutants_check.py --base <the commit the task started from>` in the background and
   give its summary to the task review; PR bodies carry the summary in place of the old
   "mutation → caught-by" table.
3. **New or old:** the rule, the key without line and column, the multiset, and why
   (spec section 4).
4. **Waivers:** `#[cfg_attr(test, mutants::skip)]` with a `//` reason directly above, and
   `mutants = "0.0.3"` added to that crate's `[dev-dependencies]` the first time (🪤 not a normal
   dependency); config exclusions in `.cargo/mutants.toml` with a `#` reason above each; the
   acceptable reasons (spec section 4, "Acceptable reasons"); `scripts/check_waivers.py`
   enforces the reasons, in lint.yml.
5. **In CI:** the Mutants job on pull requests, skipped without `.rs` changes, sharded at 6
   mutants per shard, at most 6 at a time.
6. **Module passes:** `mutants-full.yml` with a `files` glob, its timing, and the artifact.
7. The existing tooling notes (wrapper, `-j 1`, scratch, configuration, per-crate tests) stay,
   trimmed of the baseline.
8. **What stays manual** stays as is.

Delete "The full run" and "The baseline" subsections. Keep the page's style: short bullets,
🔑/🪤 markers, measured numbers with their dates.

- [ ] **Step 1: Check the dev-dependency's current version** with
  `cargo search mutants --limit 3` and use the version it prints for `mutants` in place of
  `0.0.3` if they differ.
- [ ] **Step 2: Edit `docs/testing.md`** (and `README.md` if needed).
- [ ] **Step 3: Check:** `git grep -n -i "baseline\|full run\|refresh" docs/testing.md README.md`
  shows only cargo-mutants' unmutated-baseline meaning; every command in the section runs as
  written (`scripts/mutants_check.py --help`, `scripts/check_waivers.py`).
- [ ] **Step 4: Commit:** `docs: testing.md for the mutants gate`.
