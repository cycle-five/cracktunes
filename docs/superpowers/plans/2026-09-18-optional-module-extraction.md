# Optional Module Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete `crack-gpt`, and move `crack-bf` and `crack-osint` into their own
public GitHub repositories consumed as optional git dependencies, so each carries
its own version instead of inheriting cracktunes'.

**Architecture:** Each extracted crate must first satisfy the five-rule contract
in the spec — no workspace dependencies, no Discord types in its public API, its
own version line, its own tests, no shared-schema coupling. Extraction then goes
in three moves per crate: make it standalone *in place* (so the gate still
proves it), carry it out with `git subtree split` to preserve history, and
repoint cracktunes at the new repository by tag. The cargo feature names never
change, so `cargo build --features crack-osint` keeps working for anyone who
clones the repo.

**Tech Stack:** Rust 2021 (toolchain `stable`, pinned by `rust-toolchain.toml`),
cargo workspaces, git subtree, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-18-optional-module-extraction-design.md`

## Global Constraints

- **NO version bump anywhere in this arc.** Owner decision. Nothing here reaches
  the shipped binary, so the cadence's "skip the bump when nothing ships" applies.
  Do not touch `[workspace.package] version` in the root `Cargo.toml`.
- **Extracted crates start at `version = "0.1.0"`** with their own version line.
  Never `version.workspace = true`.
- **The extracted repositories must be PUBLIC.** cracktunes is public and MIT; a
  private git dependency fails authentication for everyone but the owner and
  silently breaks the host-it-yourself story.
- **Creating a GitHub repository and pushing to it are outward actions.** Stop
  and get the owner's explicit go-ahead at every step marked **OWNER GATE**.
  Never create or push to a remote repository without it.
- **Commit trailer, exactly this line and no other `Co-Authored-By`:**
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`
- **Stage explicit paths.** Never `git add -A`, never `git add .`.
- **The gate** (must pass before every commit in the cracktunes repo):
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  SQLX_OFFLINE=true cargo test --workspace
  ```
  🪤 **Every task in this arc changes `Cargo.toml`, so `Cargo.lock` goes stale
  and the `--locked` step fails first time with `cannot update the lock file …
  because --locked was passed`.** That is the flag doing its job, not a problem
  with your change. Regenerate the lock once, *read the diff to confirm it holds
  only what your change should have caused*, then run the gate as written:
  ```bash
  cargo check --workspace          # regenerates Cargo.lock
  git diff --stat Cargo.lock       # confirm the delta is yours and nothing else
  ```
  Never skip `--locked` on the real gate run — that step passing is what proves
  the committed lock agrees with the committed manifests. (Found in Task 1.)
- **Never build cracktunes Docker images locally.** Container DNS has been broken
  for months; image builds happen in CI.
- **The tag push IS the release** for cracktunes. Not relevant to this arc — no
  cracktunes release is cut here — but the extracted repos' tags are ordinary
  git tags with no release automation behind them.
- **Do not add tests that scan source text.** The owner vetoed that approach
  (2026-09-14). Verification here is cargo-based: `cargo tree`, `cargo check`
  with and without features, and the existing suites.

### A note on testing this arc

This is a refactor that adds no behaviour, so most tasks have no new unit test to
write — inventing one would be theatre. What each task does have is a
**falsifiable cargo assertion** with an explicit expected result, plus the full
gate. Where a step says "Expected: FAILS with …", run it and read the error; a
different error means something else is wrong and the task is not done.

The one genuine code change in the arc is Task 5 (crack-osint's `Error` alias),
and it is covered by the crate's existing suite.

## File Structure

**Deleted**
- `crack-gpt/` — the whole crate (268 lines)
- `crack-core/src/commands/chatgpt.rs` — its poise wrapper

**Modified in cracktunes**
- `Cargo.toml` — workspace members list
- `crack-core/Cargo.toml` — features and optional dependencies
- `crack-core/src/lib.rs` — `DataInner` field, import, Display, builder, construction
- `crack-core/src/commands/mod.rs` — module, re-export, `all_commands()`
- `crack-core/src/errors.rs` — error variant and its Display arm
- `scripts/run_one_test.sh` — feature flags

**Created in cracktunes**
- `docs/module-development.md` — the two-repo workflow and the path override

**Created in each new repository** (`cycle-five/crack-bf`, `cycle-five/crack-osint`)
- `.github/workflows/ci.yml` — fmt, clippy, test
- `.github/dependabot.yml` — weekly grouped
- `rust-toolchain.toml` — `stable`, matching cracktunes
- `rustfmt.toml` — copied from the root; without it the crate fails its own fmt gate
- `.gitignore` — the root's rules are root-anchored and do not survive the split
- `LICENSE` — MIT, carried from the template commit the split overwrites
- `README.md`

---

### Task 1: Delete crack-gpt, and crack-core's false claim on crack-voting

`crack-gpt` is a thin OpenAI wrapper that is not wired in and needs the operator
to spend their own tokens (owner decision: delete). Every site is behind
`#[cfg(feature = "crack-gpt")]`, which is off, so nothing that ships changes.

Riding along: crack-core declares an optional dependency on `crack-voting` and a
feature for it, but **never imports it** — zero matches for `crack_voting` in
`crack-core/src`. That is dead configuration. The crate itself stays a workspace
member and keeps building; only crack-core's false claim on it goes.

**Files:**
- Delete: `crack-gpt/` (directory), `crack-core/src/commands/chatgpt.rs`
- Modify: `Cargo.toml:6` (member), `Cargo.toml:123` (commented line)
- Modify: `crack-core/Cargo.toml:30` (gpt feature), `:79` (gpt dep), and the
  `crack-voting` feature and optional dependency lines
- Modify: `crack-core/src/commands/mod.rs:4-5, 20-21, 60`
- Modify: `crack-core/src/lib.rs:26-27, 410-411, 440-441, 474-477, 607-608`
- Modify: `crack-core/src/errors.rs:56, 169`
- Modify: `scripts/run_one_test.sh:4, 22`

**Interfaces:**
- Consumes: nothing.
- Produces: `DataInner` no longer has a `gpt_ctx` field; `Data::with_gpt_ctx` no
  longer exists; `all_commands()` no longer offers `chat()`. Later tasks do not
  depend on any of this.

- [ ] **Step 1: Record the "before" so the deletion can be proved**

```bash
cargo tree -p crack-core -e normal --features crack-gpt 2>&1 | grep -c crack-gpt
```
Expected: a non-zero count — the dependency currently resolves.

- [ ] **Step 2: Delete the crate and its wrapper**

```bash
git rm -r crack-gpt
git rm crack-core/src/commands/chatgpt.rs
```

- [ ] **Step 3: Remove the workspace member entry**

In `Cargo.toml`, delete the `"crack-gpt",` line from `[workspace] members` and
the commented `# crack-gpt = { path = "../crack-gpt", ... }` line from
`[workspace.dependencies]`.

- [ ] **Step 4: Remove crack-core's features and dependencies**

In `crack-core/Cargo.toml` delete all four lines:

```toml
crack-gpt = ["dep:crack-gpt"]
crack-voting = ["dep:crack-voting"]
crack-gpt = { path = "../crack-gpt", optional = true }
crack-voting = { path = "../crack-voting", optional = true }
```

- [ ] **Step 5: Remove the command wiring**

In `crack-core/src/commands/mod.rs` delete the three `#[cfg(feature =
"crack-gpt")]` blocks: the `pub mod chatgpt;` declaration, the `pub use
chatgpt::*;` re-export, and the `chat(),` entry in `all_commands()`.

- [ ] **Step 6: Remove the Data plumbing**

In `crack-core/src/lib.rs` delete, each with its `#[cfg(feature = "crack-gpt")]`
attribute: the `use crack_gpt::GptContext;` import, the `pub gpt_ctx:
Arc<RwLock<Option<GptContext>>>` field, the `gpt_context: {:?}` line in the
Display impl, the whole `with_gpt_ctx` method, and the `gpt_ctx:
Arc::new(RwLock::new(None)),` line in the `DataInner` construction.

- [ ] **Step 7: Remove the error variant**

In `crack-core/src/errors.rs` delete the `#[cfg(feature = "crack-gpt")]` variant
at line 56 and its match arm at line 169.

- [ ] **Step 8: Remove the feature flags from the test script**

In `scripts/run_one_test.sh` lines 4 and 22, delete `--features crack-gpt` from
each, leaving the other flags intact.

- [ ] **Step 9: Prove the feature is gone**

```bash
cargo check -p crack-core --features crack-gpt
```
Expected: FAILS with `none of the selected packages contains these features:
crack-gpt`. If it succeeds, a feature declaration was missed.

```bash
cargo check -p crack-core --features crack-voting
```
Expected: FAILS the same way.

- [ ] **Step 10: Prove nothing else broke**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all three pass. `Cargo.lock` will have changed — `async-openai` and its
tree leave it. Stage it.

- [ ] **Step 11: Commit**

```bash
git add Cargo.toml Cargo.lock crack-core/Cargo.toml crack-core/src/lib.rs \
  crack-core/src/commands/mod.rs crack-core/src/errors.rs scripts/run_one_test.sh
git commit
```

Message body: say that crack-gpt was a thin unwired OpenAI wrapper requiring the
operator's own tokens, that every site was behind a feature that was off so the
shipped binary is unchanged, and that crack-core's unused optional dependency on
crack-voting went with it. Trailer per Global Constraints.

---

### Task 2: Make crack-bf standalone, in place

Prepare the crate to live outside the workspace **without moving it yet**, so the
full gate still proves it compiles. Only two things tie it to the workspace: its
inherited version and its inherited `tokio` spec.

**Files:**
- Modify: `crack-bf/Cargo.toml`
- Create: `crack-bf/rust-toolchain.toml`, `crack-bf/README.md`,
  `crack-bf/.github/workflows/ci.yml`, `crack-bf/.github/dependabot.yml`

**Interfaces:**
- Consumes: nothing.
- Produces: `crack-bf` at version `0.1.0` with no `workspace = true` keys. Task 3
  carries this directory out by `git subtree split`, so every file it needs in the
  new repository must exist here first.

- [ ] **Step 1: Confirm what ties it to the workspace**

```bash
grep -n "workspace" crack-bf/Cargo.toml
```
Expected: exactly three lines — `version.workspace = true`, `workspace = "../"`,
and `tokio = { workspace = true }`.

- [ ] **Step 2: Materialise the manifest**

Replace those three in `crack-bf/Cargo.toml`. `version.workspace = true` becomes
`version = "0.1.0"`; delete the `workspace = "../"` line entirely; and
`tokio = { workspace = true }` becomes the concrete spec copied from the root
`[workspace.dependencies.tokio]` table:

```toml
tokio = { version = "1.34.0", features = ["full"] }
```

Also update `repository` from `https://github.com/cycle-five/cracktunes` to
`https://github.com/cycle-five/crack-bf`, and drop the stale `v0.1.0` suffix from
the `description` string — the version lives in the `version` field now.

- [ ] **Step 3: Pin the toolchain**

Create `crack-bf/rust-toolchain.toml` with exactly what cracktunes pins:

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 4: Give it CI**

Create `crack-bf/.github/workflows/ci.yml`. A crate that leaves the workspace
leaves `cargo clippy --workspace` behind, so it must arrive with its own gate or
it rots — which is the failure this whole arc exists to fix.

```yaml
name: CI

on:
  push:
    branches: [master]
  pull_request:
    types: [opened, synchronize, reopened]

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

permissions:
  contents: read

jobs:
  check:
    name: Fmt, clippy and test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7

      - name: Install Rust
        run: |
          rustup toolchain install stable --profile minimal --no-self-update
          rustup component add rustfmt clippy --toolchain stable
          rustup default stable
        shell: bash

      - uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Test
        run: cargo test
```

- [ ] **Step 5: Give it dependabot**

Create `crack-bf/.github/dependabot.yml`, on the weekly grouped schedule
cracktunes adopted in #542:

```yaml
version: 2
updates:
  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
    groups:
      cargo-minor-and-patch:
        patterns: ["*"]
        update-types: ["minor", "patch"]

  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "weekly"
    groups:
      actions:
        patterns: ["*"]
```

- [ ] **Step 6: Write the README**

Create `crack-bf/README.md`. It must say four things:

1. What the crate is: a Brainfuck interpreter, used by cracktunes' `/bf` command.
2. **The poise command wrapper lives in cracktunes**, at
   `crack-core/src/commands/bf.rs`, and must never move here — a command needs
   `crack_core::Context`, and crack-core depends on this crate, so importing it
   back is a cycle cargo forbids. This is rule 2 of the extraction contract.
3. How to build and test it: `cargo build`, `cargo test`.
4. **Known limitation:** it executes arbitrary user-submitted programs with no
   step limit, memory cap or timeout. Carried from the spec so the next reader
   finds it rather than rediscovering it.

- [ ] **Step 6a: Copy `rustfmt.toml` into the crate**

```bash
cp rustfmt.toml crack-bf/rustfmt.toml
```

🪤 **Without this the new repository's first CI run fails `cargo fmt --check`,
and nothing in cracktunes can catch it.** The root `rustfmt.toml` sets
`match_block_trailing_comma = true` — a *stable* option — and crack-bf's source
is formatted to it. Inside the workspace the crate inherits that file and fmt
passes; a standalone clone has no such file, so default rustfmt wants every
`},` at the end of a match arm to be `}` and reports a diff in six places in
`src/lib.rs`.

Verified empirically: a standalone copy fails `cargo fmt --all -- --check`
without this file and passes with it. The two nightly-only options in it
(`imports_layout`, `match_arm_blocks`) print a warning under the stable
toolchain and are ignored — exactly as they already are on cracktunes' own
stable CI leg — which is harmless.

Do **not** also copy `clippy.toml`. Its `disallowed-methods` entries name
songbird and rusty_ytdl paths that crack-bf does not depend on, and an
unresolvable path there risks a lint warning that `-D warnings` would turn into
a CI failure. Standalone clippy was verified clean without it.

- [ ] **Step 6b: Add a `.gitignore`**

Create `crack-bf/.gitignore`:

```gitignore
/target/
```

🪤 The root `.gitignore`'s rules are `/target/` and `/*/target/` — both anchored
to the repository root with a leading slash, so **neither survives
`git subtree split -P crack-bf`**. Without this the new repository arrives with
no ignore rules at all and a contributor can commit `target/`. No crate in this
workspace has its own `.gitignore` today, so this applies to every extraction.

Do not add a `Cargo.lock` rule: leaving the lock untracked is the current state
and changing it is not this task's call.

- [ ] **Step 6c: Add the LICENSE**

The repository was created from `cycle-five/template`, whose only contributions
are a `LICENSE` and a stub `README.md`. Task 3 force-pushes over that commit, so
the licence has to survive as a file here or it is lost. Copy it verbatim:

```bash
cp .superpowers/sdd/2026-09-18-optional-module-extraction/template-LICENSE.txt crack-bf/LICENSE
```

Expected: 21 lines, MIT, `Copyright (c) 2026 Cycle Five`. The crate manifest
already declares `license = "MIT"`, so this makes the tree agree with it.

- [ ] **Step 7: Prove it still builds inside the workspace**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all pass, including crack-bf's 6 tests in `crack-bf/src/lib.rs`.

- [ ] **Step 8: Prove the version detached**

```bash
cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="crack-bf") | .version'
```
Expected: `0.1.0` — not the workspace's `0.13.0`.

- [ ] **Step 9: Commit**

```bash
git add crack-bf/Cargo.toml crack-bf/rust-toolchain.toml crack-bf/README.md \
  crack-bf/LICENSE crack-bf/rustfmt.toml crack-bf/.gitignore \
  crack-bf/.github/workflows/ci.yml \
  crack-bf/.github/dependabot.yml Cargo.lock
git commit
```

Message body: crack-bf now carries its own version, toolchain pin, CI and
dependabot config, so that `git subtree split` produces a repository that builds
and gates itself from its first commit.

---

### Task 3: Publish crack-bf to its own repository

**OWNER GATE.** This task creates a GitHub repository and pushes to it. Both are
outward actions. Present the plan for this task to the owner and **wait for an
explicit go-ahead** before running any step. If the owner would rather create the
repository themselves, do steps 1–2, hand them the branch name, and resume at
step 5.

**Files:** none in cracktunes — this task only produces a new remote repository.

**Interfaces:**
- Consumes: Task 2's prepared `crack-bf/` directory.
- Produces: `https://github.com/cycle-five/crack-bf` at tag `v0.1.0`. Task 4
  pins that exact tag.

- [ ] **Step 1: Split the history**

```bash
git subtree split -P crack-bf -b crack-bf-export
```
Expected: prints a commit sha. This creates a local branch whose history contains
only commits touching `crack-bf/`, with the directory promoted to the root.

- [ ] **Step 2: Verify the split before pushing anything**

```bash
git log --oneline crack-bf-export | wc -l
git ls-tree --name-only crack-bf-export
```
Expected: a non-zero commit count, and a file list showing `Cargo.toml`, `src`,
`README.md`, `rust-toolchain.toml` and `.github` **at the root** — not nested
under `crack-bf/`. If the tree still shows a `crack-bf/` directory, the split
went wrong; stop and diagnose rather than pushing.

- [ ] **Step 3: DONE ALREADY — the owner created the repository**

`https://github.com/cycle-five/crack-bf` exists, is **public**, and was created
from `cycle-five/template`. Confirm before pushing:

```bash
gh api repos/cycle-five/crack-bf --jq '.visibility'
```
Expected: `PUBLIC`. If it is private, stop — a private git dependency fails
authentication for everyone but the owner and silently breaks the
host-it-yourself story.

- [ ] **Step 4: OWNER GATE — force-push the history**

🪤 The template left one commit on `master` (`01eebcc8`, adding `LICENSE` and a
stub `README.md`), and the split branch is rooted years earlier in unrelated
history. A plain push is rejected; a merge would root the repository at a
template stub. The owner chose a force-push (2026-09-18), which is why Task 2
Step 6c copied that `LICENSE` into the crate — it survives as a real file, and
Task 2's README replaces the stub.

Confirm the owner's go-ahead is still current, then:

```bash
git push --force https://github.com/cycle-five/crack-bf crack-bf-export:master
```

Then verify nothing of value was lost:

```bash
gh api repos/cycle-five/crack-bf/contents/LICENSE --jq '.content' | base64 -d | head -3
```
Expected: `MIT License` / blank / `Copyright (c) 2026 Cycle Five` — the licence
is present, now carried by our own history rather than the template's.

- [ ] **Step 5: Tag v0.1.0 in the new repository**

```bash
WORK="$(mktemp -d)" && cd "$WORK"
git clone https://github.com/cycle-five/crack-bf
cd crack-bf
git tag -s v0.1.0 -m "crack-bf 0.1.0: first release as a standalone crate"
git push origin v0.1.0
echo "clone is at $WORK/crack-bf"
```

The tag is an ordinary git tag. Unlike cracktunes, no workflow watches it, so
this creates no GitHub release and nothing needs to be raced.

- [ ] **Step 6: Verify the published crate builds on its own**

```bash
cd "$WORK/crack-bf"
cargo build
cargo test
```
Expected: both succeed, out of tree, with no cracktunes checkout in sight. This
is the real proof the crate is standalone. If it fails, the manifest still
depends on something the workspace was providing — fix it in cracktunes, and
repeat from step 1.

🪤 **The clone must live outside the cracktunes checkout**, which is why this
uses `mktemp -d` rather than a directory under the repo. A nested checkout picks
up the parent's `clippy.toml` and `rustfmt.toml`, so it would be gating against
cracktunes' configuration while appearing to prove independence — a false pass
this repository has produced before.

- [ ] **Step 7: Clean up the local export branch**

```bash
cd /home/lothrop/projects/cracktunes
git branch -D crack-bf-export
```

No commit — this task changes nothing in the cracktunes repository.

---

### Task 4: Repoint cracktunes at crack-bf, and document the two-repo workflow

Swap the path dependency for the git dependency, remove the directory from the
workspace, and add the local-development scaffolding that this and every later
extraction reuses.

**Files:**
- Delete: `crack-bf/` (directory — its history now lives in the new repository)
- Modify: `Cargo.toml` (member list), `crack-core/Cargo.toml` (the dependency)
- (No `.gitignore` change — see Step 3; `.cargo/config.toml` is tracked)
- Create: `docs/module-development.md`

**Interfaces:**
- Consumes: `cycle-five/crack-bf` at tag `v0.1.0` from Task 3.
- Produces: the `--config` patch override pattern and
  `docs/module-development.md`, both reused verbatim by Task 8.

- [ ] **Step 1: Repoint the dependency**

In `crack-core/Cargo.toml`, replace:

```toml
crack-bf = { path = "../crack-bf", optional = true }
```

with:

```toml
crack-bf = { git = "https://github.com/cycle-five/crack-bf", tag = "v0.1.0", optional = true }
```

Leave the `crack-bf = ["dep:crack-bf"]` feature line exactly as it is. The
feature name is the host-it-yourself interface and must not change.

- [ ] **Step 2: Remove the directory and the workspace member**

```bash
git rm -r crack-bf
```

Then delete the `"crack-bf",` line from `[workspace] members` in the root
`Cargo.toml`, and the commented `# crack-bf = { path = "../crack-bf", ... }` line
from `[workspace.dependencies]`.

- [ ] **Step 3: Do NOT add a `.gitignore` entry — the earlier draft was wrong**

🪤 An earlier version of this plan said to gitignore `.cargo/config.toml`. **That
does not work: `.cargo/config.toml` is already a tracked file** (`git ls-files
.cargo/config.toml` confirms it), holding the workspace's `rustflags` and wasm
target settings. A `.gitignore` rule has no effect on a tracked file, so the
entry would do nothing while reading as though it protected something — and
writing the override into that file would edit tracked config that could then be
committed and break every other clone.

Make no `.gitignore` change in this task. The override moves to a command-line
flag instead, which writes no file at all and therefore cannot be committed by
accident.

- [ ] **Step 4: Document the workflow**

Create `docs/module-development.md` covering: which modules are extracted and
where they live; that a change spanning crack-core and a module is two commits, a
tag and a pin bump; and the override that makes local work feel like one repo.

The override is a `--config` flag, not a file:

````markdown
```bash
cargo check -p crack-core --features crack-bf \
  --config 'patch."https://github.com/cycle-five/crack-bf".crack-bf.path="../crack-bf"'
```
````

Explain why it is a flag and not a file: `[patch]` in `Cargo.toml` is tracked and
would break every other clone, and `.cargo/config.toml` is *also* tracked here —
so both file-based routes put a local-only override into version control. The
flag leaves nothing behind.

Record the release sequence: commit and push in the module repo, tag it, then
bump the `tag = ` value in `crack-core/Cargo.toml` and commit that.

- [ ] **Step 5: Confirm the override mechanism actually works**

Clone the module beside the cracktunes checkout — `crack-bf/` no longer exists
inside the repo, and Task 3's `mktemp -d` clone is gone:

```bash
git clone https://github.com/cycle-five/crack-bf /home/lothrop/projects/crack-bf
```

Then, from the cracktunes checkout:

```bash
cargo tree -p crack-core --features crack-bf -e normal \
  --config 'patch."https://github.com/cycle-five/crack-bf".crack-bf.path="/home/lothrop/projects/crack-bf"' \
  | grep crack-bf
```
Expected: crack-bf resolved from the local path, not the git URL.

The `--config` syntax itself is already confirmed accepted by the installed
cargo. What this step proves is that the patch actually redirects the
dependency. Record the exact working command in `docs/module-development.md`.

No cleanup needed afterwards: the flag changes nothing on disk, so the remaining
steps automatically verify against the real git dependency.

- [ ] **Step 6: Prove the feature still builds from git**

```bash
cargo check -p crack-core --features crack-bf
```
Expected: succeeds, and cargo logs `Updating git repository
https://github.com/cycle-five/crack-bf` on the first run.

```bash
cargo tree -p crack-core --features crack-bf -e normal | grep crack-bf
```
Expected: shows the git source, not a path.

- [ ] **Step 7: Prove the default build is untouched**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all pass. `Cargo.lock` now carries crack-bf with a `source = "git+…"`
line; stage it.

- [ ] **Step 7b: Sweep the references the move strands**

🔑 **Feature-name references stay correct and must NOT be touched.** The
`crack-bf` cargo feature still exists and still works — that is the entire point
of the design. So leave `#[cfg(feature = "crack-bf")]` in
`crack-core/src/commands/mod.rs`, `--features crack-bf` in
`scripts/run_one_test.sh`, and the feature lists in `.vscode/tasks.json` exactly
as they are.

**Path-based references break**, because the directory is gone. Fix these:

- `.vscode/settings.json` — remove `"./crack-bf/Cargo.toml"` from
  `rust-analyzer.linkedProjects` (~line 22) and `"crack-bf"` from the features
  list only if it names a *path*; if it is a feature name, leave it. Leave every
  `crack-osint` entry alone — Task 8 removes those.
- `README.md:192` — this line reads `--features crack-osint,crack-bf,crack-fpt`.
  `crack-fpt` is a typo for the now-deleted `crack-gpt`, so the documented
  command fails on an unknown feature. Task 1's sweep missed it because it
  searched for `crack-gpt`, not the misspelling. Remove `,crack-fpt`.

Then confirm the JSON still parses, since that is the likeliest way this breaks:

```bash
python3 .superpowers/sdd/2026-09-18-optional-module-extraction/check-jsonc.py .vscode/settings.json
```
Expected: `parses OK`, exit 0.

🪤 Use that script, not a regex that strips everything after `//`. These files are
JSONC — comments and trailing commas are legal, so plain `json.loads` rejects a
valid file — and `.vscode/settings.json` contains a `postgres://` URL that a
naive comment-stripper corrupts. The script tracks string state so a `//` inside
a quoted value survives. It has been sabotage-checked: it reports a PARSE ERROR
and exits 1 on structurally broken input.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crack-core/Cargo.toml \
  docs/module-development.md .vscode/settings.json README.md
git commit
```

Message body: crack-bf now lives at `cycle-five/crack-bf` and is consumed as an
optional git dependency pinned to `v0.1.0`; the `crack-bf` feature is unchanged,
so `cargo build --features crack-bf` still works for anyone cloning the repo.
Mention that `docs/module-development.md` records the path override for working
across both repositories.

---

### Task 5: Make crack-osint workspace-independent

The only code change in this arc. crack-osint's single tie to the workspace is
one import whose type is byte-identical to an alias the crate already declares.
`whois-rust` also goes: its only consumer, `whois.rs`, is commented out of
`lib.rs`, so it is an unused dependency.

**Files:**
- Modify: `crack-osint/src/lib.rs:31`, `crack-osint/src/checkpass.rs:1`,
  `crack-osint/Cargo.toml`

**Interfaces:**
- Consumes: nothing.
- Produces: `crack_osint::Error` becomes public (it was `pub(crate)`).
  `check_password_pwned` keeps the signature
  `pub async fn check_password_pwned(client: &reqwest::Client, password: &str) -> Result<bool, Error>`
  — the alias it names changes, the underlying
  `Box<dyn std::error::Error + Send + Sync>` does not, so crack-core's call site
  in `commands/osint.rs` needs no change.

- [ ] **Step 1: Confirm the two aliases are the same type**

```bash
grep -n "pub type Error" crack-types/src/lib.rs
grep -n "type Error" crack-osint/src/lib.rs
```
Expected: `crack-types/src/lib.rs:36: pub type Error = Box<dyn StdError + Send + Sync>;`
and `crack-osint/src/lib.rs:31: pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;`
— the same type, written two ways. `StdError` is `std::error::Error` imported
under an alias.

- [ ] **Step 2: Make crack-osint's own alias public**

`crack-osint/src/lib.rs:31`, change `pub(crate) type Error` to `pub type Error`.
It appears in `check_password_pwned`'s public signature, so it should be nameable
by callers.

- [ ] **Step 3: Drop the workspace import**

`crack-osint/src/checkpass.rs:1`, change `use crack_types::Error;` to
`use crate::Error;`.

- [ ] **Step 4: Remove both unused dependencies**

In `crack-osint/Cargo.toml` delete:

```toml
whois-rust = "3.1"
crack-types = { path = "../crack-types" }
```

- [ ] **Step 5: Prove the workspace tie is gone**

```bash
cargo tree -p crack-osint -e normal | grep -E "crack-types|whois" && echo "STILL COUPLED" || echo "clean"
```
Expected: prints `clean`. `-e normal` excludes dev- and build-dependencies, which
is what we want — this asks what the crate needs to *build*. If it prints
`STILL COUPLED`, read the tree output above it to see which dependency survived.

- [ ] **Step 6: Run the gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all pass, including crack-osint's compiled tests.

⚠️ `crack-osint/src/test/checkpass.rs` calls the **live** HIBP API at
`https://api.pwnedpasswords.com` and unwraps the result, so this leg can fail on
a network blip. That is a #523-class test the issue's census missed. **Do not fix
it here** — it is out of scope. If it fails, re-run; if it fails repeatedly, note
it and add it to #523 rather than chasing it.

- [ ] **Step 7: Commit**

```bash
git add crack-osint/Cargo.toml crack-osint/src/lib.rs crack-osint/src/checkpass.rs Cargo.lock
git commit
```

Message body: crack-osint's only workspace dependency was `crack_types::Error`,
which is the same `Box<dyn Error + Send + Sync>` as the alias the crate already
declared; `whois-rust` was unused because `whois.rs` is commented out of
`lib.rs`. The crate now satisfies rule 1 of the extraction contract.

---

### Task 6: Make crack-osint standalone, in place

Same shape as Task 2. crack-osint inherits its version and three dependency specs
from the workspace.

**Files:**
- Modify: `crack-osint/Cargo.toml`
- Create: `crack-osint/rust-toolchain.toml`, `crack-osint/README.md`,
  `crack-osint/.github/workflows/ci.yml`, `crack-osint/.github/dependabot.yml`

**Interfaces:**
- Consumes: Task 5's manifest, with `crack-types` and `whois-rust` already gone.
- Produces: `crack-osint` at version `0.1.0` with no `workspace = true` keys.

- [ ] **Step 1: Materialise the manifest**

In `crack-osint/Cargo.toml`: `version.workspace = true` becomes
`version = "0.1.0"`; delete `workspace = "../"`; and replace the three inherited
specs with the concrete ones copied from the root `[workspace.dependencies]`:

```toml
tokio = { version = "1.34.0", features = ["full"] }
tracing = "0.1.40"
reqwest = { version = "0.12.9", default-features = false, features = [
  "blocking",
  "json",
  "multipart",
  "rustls-tls",
  "cookies",
  "charset",
  "http2",
  "macos-system-configuration",
] }
```

Update `repository` to `https://github.com/cycle-five/crack-osint` and drop the
stale `v0.1.4.` from the `description` string.

- [ ] **Step 2: Pin the toolchain**

Create `crack-osint/rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 3: Give it CI**

Create `crack-osint/.github/workflows/ci.yml` with the same content as
`crack-bf/.github/workflows/ci.yml` from Task 2, Step 4 — identical file, so copy
it rather than retyping. The workflow is generic; nothing in it names the crate.

- [ ] **Step 4: Give it dependabot**

Create `crack-osint/.github/dependabot.yml` with the same content as
`crack-bf/.github/dependabot.yml` from Task 2, Step 5.

- [ ] **Step 5: Write the README**

Create `crack-osint/README.md`. It must say four things:

1. What ships: `check_password_pwned`, `VirusTotalClient`, `scan_url` and
   `get_scan_result`, behind the `checkpass`, `virustotal` and `scan` features.
2. **The poise wrappers live in cracktunes**, at
   `crack-core/src/commands/osint.rs`, and must never move here — a command needs
   `crack_core::Context`, and crack-core depends on this crate, so importing it
   back is a cycle cargo forbids. This is rule 2 of the extraction contract and
   it is what left `phcode.rs` and `phlookup.rs` uncompilable for years.
3. That roughly 474 lines across `ip.rs`, `ipv.rs`, `paywall.rs`, `phcode.rs`,
   `phlookup.rs`, `socialmedia.rs`, `wayback.rs` and `whois.rs` are commented out
   of `lib.rs` and have not been built in years, and that triaging them is the
   first job in this repository.
4. That `phlookup.rs` must not be revived as written — see cracktunes#549.
5. **That two of the three features gate nothing.** `default = ["checkpass",
   "virustotal", "scan"]`, but only `checkpass` appears in a `#[cfg(feature =
   ...)]`: `pub mod scan;` and `pub mod virustotal;` are unconditional in
   `lib.rs`, so turning either off changes nothing. Record it as a known defect
   for the new repository to fix — either gate the modules or drop the feature
   declarations. Do **not** fix it in this arc; changing feature gating is a
   behaviour change and out of scope. Note too that the CI workflow therefore
   only exercises one meaningful combination, and a `--no-default-features` leg
   would be worth adding once the gating is real.

- [ ] **Step 5a: Copy `rustfmt.toml` into the crate**

```bash
cp rustfmt.toml crack-osint/rustfmt.toml
```

Same trap as Task 2 Step 6a, and it applies here for the same reason: crack-osint's
source is formatted under the root `rustfmt.toml`'s
`match_block_trailing_comma = true`, which a standalone clone would not have, so
its first CI run would fail `cargo fmt --check` on a diff no cracktunes gate can
see. Do **not** copy `clippy.toml` — its `disallowed-methods` entries name
songbird and rusty_ytdl paths this crate does not depend on.

- [ ] **Step 5b: Add a `.gitignore`**

Create `crack-osint/.gitignore` with `/target/`, for the same reason as Task 2
Step 6b: the root rules are root-anchored and do not survive the split.

- [ ] **Step 5c: Add the LICENSE**

crack-osint has no `LICENSE` file though its manifest declares `license = "MIT"`.
Task 7 creates the repository **empty, with no template** (owner decision), so
it contributes no licence of its own — this file will be the only one it has:

```bash
cp .superpowers/sdd/2026-09-18-optional-module-extraction/template-LICENSE.txt crack-osint/LICENSE
```

Expected: 21 lines, MIT, `Copyright (c) 2026 Cycle Five`.

- [ ] **Step 6: Run the gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all pass. The HIBP caveat from Task 5, Step 6 applies.

- [ ] **Step 7: Prove the version detached**

```bash
cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="crack-osint") | .version'
```
Expected: `0.1.0`.

- [ ] **Step 8: Commit**

```bash
git add crack-osint/Cargo.toml crack-osint/rust-toolchain.toml crack-osint/README.md \
  crack-osint/LICENSE crack-osint/rustfmt.toml crack-osint/.gitignore \
  crack-osint/.github/workflows/ci.yml \
  crack-osint/.github/dependabot.yml Cargo.lock
git commit
```

---

### Task 7: Publish crack-osint to its own repository

**OWNER GATE.** Same outward actions as Task 3. Get the owner's explicit
go-ahead before creating the repository or pushing.

**Files:** none in cracktunes.

**Interfaces:**
- Consumes: Task 6's prepared `crack-osint/` directory.
- Produces: `https://github.com/cycle-five/crack-osint` at tag `v0.1.0`.

- [ ] **Step 1: Split the history**

```bash
git subtree split -P crack-osint -b crack-osint-export
```

crack-osint's oldest file dates to 2023-08-01; this is the history worth keeping.

- [ ] **Step 2: Verify the split before pushing anything**

```bash
git log --oneline crack-osint-export | wc -l
git ls-tree --name-only crack-osint-export
```
Expected: a non-zero commit count, and `Cargo.toml`, `src`, `README.md`,
`rust-toolchain.toml`, `.github` at the root — not nested under `crack-osint/`.

- [ ] **Step 3: Create the repository — EMPTY, no template**

Owner decision, 2026-09-18: unlike crack-bf, create this one with **no
template**, so `master` starts empty and the split pushes cleanly with no
force-push and nothing discarded.

```bash
gh repo create cycle-five/crack-osint --public \
  --description "OSINT lookups for cracktunes: pwned-password checks, VirusTotal scans"
```

`--public` is required, for the reason in Task 3: a private git dependency fails
authentication for everyone but the owner and silently breaks the
host-it-yourself story. Confirm it:

```bash
gh api repos/cycle-five/crack-osint --jq '.visibility'
```
Expected: `PUBLIC`.

Because there is no template, the repository contributes no `LICENSE` of its
own — which is why Step 5c copied one into the crate. That file is now the only
licence this repository will have.

- [ ] **Step 4: Push the history**

Confirm the repository really is empty before pushing, so a plain push is the
right call:

```bash
gh api repos/cycle-five/crack-osint/commits --jq 'length' 2>/dev/null || echo 0
```
Expected: `0`, or an error meaning the repository has no commits yet. **If it
returns 1 or more, stop** — something created content and this plan's assumption
no longer holds; ask before force-pushing over it.

```bash
git push https://github.com/cycle-five/crack-osint crack-osint-export:master
```

Then verify the licence arrived:

```bash
gh api repos/cycle-five/crack-osint/contents/LICENSE --jq '.content' | base64 -d | head -3
```
Expected: `MIT License` / blank / `Copyright (c) 2026 Cycle Five`.

- [ ] **Step 5: Tag v0.1.0**

```bash
WORK="$(mktemp -d)" && cd "$WORK"
git clone https://github.com/cycle-five/crack-osint
cd crack-osint
git tag -s v0.1.0 -m "crack-osint 0.1.0: first release as a standalone crate"
git push origin v0.1.0
echo "clone is at $WORK/crack-osint"
```

- [ ] **Step 6: Verify the published crate builds on its own**

```bash
cd "$WORK/crack-osint"
cargo build
cargo test
```
Expected: both succeed out of tree. The HIBP network caveat from Task 5, Step 6
applies to the test run.

🪤 As in Task 3: the clone must live outside the cracktunes checkout, or it
inherits the parent's `clippy.toml` and `rustfmt.toml` and proves nothing.

- [ ] **Step 7: Clean up**

```bash
cd /home/lothrop/projects/cracktunes
git branch -D crack-osint-export
```

---

### Task 8: Repoint cracktunes at crack-osint

The last task. Identical in shape to Task 4, minus the scaffolding, which Task 4
already created.

**Files:**
- Delete: `crack-osint/` (directory)
- Modify: `Cargo.toml`, `crack-core/Cargo.toml`, `docs/module-development.md`

**Interfaces:**
- Consumes: `cycle-five/crack-osint` at tag `v0.1.0` from Task 7, and the
  override pattern documented in Task 4.
- Produces: nothing later depends on.

- [ ] **Step 1: Repoint the dependency**

In `crack-core/Cargo.toml`, replace:

```toml
crack-osint = { path = "../crack-osint", optional = true }
```

with:

```toml
crack-osint = { git = "https://github.com/cycle-five/crack-osint", tag = "v0.1.0", optional = true }
```

Leave `crack-osint = ["dep:crack-osint"]` untouched.

- [ ] **Step 2: Remove the directory and the workspace member**

```bash
git rm -r crack-osint
```

Then delete `"crack-osint",` from `[workspace] members` and the commented
`# crack-osint = { path = "./crack-osint", ... }` line from
`[workspace.dependencies]`.

- [ ] **Step 3: Add crack-osint to the module docs**

In `docs/module-development.md`, add crack-osint to the list of extracted modules
and add its `--config` invocation beside crack-bf's:

````markdown
```bash
cargo check -p crack-core --features crack-osint \
  --config 'patch."https://github.com/cycle-five/crack-osint".crack-osint.path="/home/lothrop/projects/crack-osint"'
```
````

- [ ] **Step 4: Prove the feature still builds from git**

```bash
cargo check -p crack-core --features crack-osint
```
Expected: succeeds, resolving crack-osint from GitHub.

```bash
cargo tree -p crack-core --features crack-osint -e normal | grep crack-osint
```
Expected: shows the git source, not a path.

- [ ] **Step 5: Prove the whole osint command tree still compiles**

```bash
cargo clippy -p crack-core --features crack-osint --all-targets --locked -- -D warnings
```
Expected: passes. This is the real integration check — `commands/osint.rs`
imports `check_password_pwned`, `VirusTotalClient`, `get_scan_result` and
`scan_url`, and `messaging/message.rs` imports `virustotal::VirusTotalApiResponse`.
All five must resolve across the new repository boundary.

- [ ] **Step 6: Run the gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
```
Expected: all pass, and faster than before — the workspace no longer compiles
crack-osint's 1015 lines or crack-bf's 299 on every run.

- [ ] **Step 7: Confirm the shipped binary is unchanged**

```bash
cargo tree -p cracktunes -e normal | grep -E "crack-osint|crack-bf|crack-gpt"
```
Expected: no output. None of the three was ever in the binary, and none is now —
which is why this arc takes no version bump.

- [ ] **Step 7b: Sweep the references the move strands**

Same rule as Task 4 Step 7b: **feature-name references stay, path-based ones go.**
`#[cfg(feature = "crack-osint")]` in `crack-core/src/commands/mod.rs` and
`crack-core/src/messaging/message.rs`, and `--features crack-osint` in
`scripts/run_one_test.sh`, are all still correct — the feature survives the move.
Leave them.

Fix the path-based ones:

- `.vscode/settings.json` — remove `"./crack-osint/Cargo.toml"` from
  `rust-analyzer.linkedProjects`.
- `.vscode/launch.json` — remove the `"Debug unit tests in library 'crack-osint'"`
  configuration block (it passes `--package=crack-osint`, which will no longer
  resolve in this workspace).
- `docs/testing.md:56` — reads "Some tests in `crack-testing` and `crack-osint`
  make live calls". crack-osint's tests leave with the crate, so drop it from
  that sentence and note that its HIBP test moved to `cycle-five/crack-osint`.

Confirm both JSON files still parse, using the same checker Task 4 used (see the
trap noted there — a naive `//` stripper corrupts the `postgres://` URL in
settings.json):

```bash
python3 .superpowers/sdd/2026-09-18-optional-module-extraction/check-jsonc.py \
  .vscode/settings.json .vscode/launch.json
```
Expected: `parses OK` for both, exit 0.

Leave `docs/callgraphs.md` alone: its `crack-bf` and `crack-osint` lines link to
SVGs hosted on cracktun.es, which no build depends on — the same call made for
crack-gpt's line in Task 1.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crack-core/Cargo.toml docs/module-development.md \
  .vscode/settings.json .vscode/launch.json docs/testing.md
git commit
```

---

## Finishing

- [ ] **Open the PR** against `master` from `refactor/optional-module-extraction`.
  The body explains the unfinished-migration root cause, the five-rule contract,
  what moved where, and — explicitly — that there is no version bump because the
  shipped binary is unchanged. Cite the spec, #549, #550, and note that #523 gains
  a sixth live-network test (`crack-osint/src/test/checkpass.rs`) which has moved
  out of this repository along with the crate.
- [ ] **Add a comment to #523** recording that the HIBP test left the repo with
  crack-osint, so its census should no longer count it here.
- [ ] **Wait for green CI**, triage any review findings, then merge.
- [ ] **No tag, no release.** This arc ships nothing.
