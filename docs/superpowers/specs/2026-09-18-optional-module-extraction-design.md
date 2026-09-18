# Optional module extraction — design

**Date:** 2026-09-18
**Target version:** none — no bump in this arc (the shipped binary is unchanged;
see Versioning)
**Status:** approved in conversation (sections 1–3 by the owner); ready for planning

## Problem

Four workspace crates — `crack-osint`, `crack-bf`, `crack-gpt`, `crack-voting` —
are optional cargo features that are off by default, so none of them is in the
shipped binary. Verified against the real dependency tree:

| ships | absent |
| --- | --- |
| crack-core, crack-types, crack-testing, crack-musicreco, crack-sleevenote | **crack-osint, crack-bf, crack-gpt, crack-voting** |

The compile-time exclusion was deliberate and is still wanted: cracktunes is
host-it-yourself, and the intent is that you clone the repo, turn on what you
want, and build your own bot. What went wrong is unrelated to that choice. A
crate-and-module reorganisation was started and never finished, and the four
crates were left on the far side of it. Because a feature that is off is never
compiled, never linted and never tested, the code rotted where nobody could see
it:

- `crack-osint/src/phcode.rs` and `phlookup.rs` **cannot compile at all**. They
  are written as poise commands inside crack-osint, which imports
  `crack_core::{Context, Error}` — but crack-core depends on crack-osint, so
  cargo forbids the cycle. Roughly 474 of crack-osint's 1015 lines are commented
  out of its `lib.rs` and have not been built in years.
- `/scan`, `/checkpass` and `/virustotal_result` are written, compile cleanly and
  are registered as subcommands of `osint()` — and are absent from production.
  So is `/chat`.
- `crack-voting` is a standalone warp server with its own `src/main.rs`, and
  **crack-core never references it**: zero matches for `crack_voting` in
  `crack-core/src`, despite crack-core declaring it as an optional dependency.

A second complaint is versioning. Since #538 every member carries
`version.workspace = true`, so all ten bump together. That fixed the ten
hand-edited copies problem and created this one: crack-osint is at 0.13.0
because the *bot* released thirteen times, and its own description string still
reads "v0.1.4" from when it versioned independently.

## What this is not

Recorded so it is not re-litigated.

**Runtime-loadable dynamic libraries — rejected.** Rust has no stable ABI, so a
`dlopen`'d plugin is sound only when both sides are built by the identical rustc
with identical dependency versions. The Docker builder tracks `stable`, and a
1.98.0 → 1.98.1 drift already cost a full rebuild during the cargo-chef work
(#542); across a dylib boundary that same silent drift is memory corruption
rather than a slow build. `abi_stable` makes it sound but requires every type
crossing the boundary to be `#[repr(C)]`/`StableAbi` — poise's `Context`,
`CrackedMessage` and `Recommendation` cannot be, so it would mean a parallel FFI
type layer: strictly more work than a wire protocol for strictly less benefit.
The only win over a process boundary is latency, which is irrelevant to a bot
whose commands already take seconds.

**A REST or gRPC seam for these four — rejected.** The bar is the one
sleevenote clears: it is a service because it drives a headless browser and is
written in TypeScript, so it *could not* be a Rust crate. None of the four
clears it.

- `crack-osint` and `crack-gpt` are already thin clients — of VirusTotal, HIBP,
  ipinfo and OpenAI. Fronting them with a service of our own means bot → our
  service → their service: two hops to reach the same API, plus a deployment to
  monitor and a new failure mode, buying independent deploy cadence and nothing
  else.
- `crack-bf` is 299 lines with `tokio` as its only dependency and no network at
  all. It does have a real isolation problem — it runs arbitrary user-submitted
  programs — but a network seam does not solve it: a program that loops forever
  hangs the service instead of the bot, and now there is a hung service *and* a
  stuck request. The answer is a step limit, a memory cap and a timeout,
  wherever it runs.
- `crack-voting` is the one that comes close, and it already has its answer: it
  is a separate process that reaches the bot through the shared database, which
  is the right seam for a webhook receiver.

A network seam also works directly against the host-it-yourself design, by
converting a build flag into a deployment topology. Someone who wants `/scan`
would go from `--features crack-osint` to "also run a second container".

**The versioning complaint does not need a network seam.** Repo separation fixes
it; the transport is orthogonal. A crate's public Rust API is governed by semver
exactly as strictly as a wire protocol is.

## 1. The extraction contract

A crate may live outside the workspace only if it satisfies all four rules.

1. **No workspace dependencies.** Not crack-core, not crack-types. Its own error
   type via `thiserror`, as `crack-sleevenote` and `crack-musicreco` already do.
2. **No Discord types in its public API.** No poise, no serenity. Pure data in,
   typed data out. **The poise command wrapper always lives in crack-core.**
   This rule is absolute (owner decision, 2026-09-18) and applies to every
   module, including any command surface `crack-voting` may grow later.
3. **Its own version line**, not `version.workspace = true`.
4. **Its own tests**, needing nothing from the bot to run.

Rule 2 is the load-bearing one. Its violation is what produced `phcode.rs` and
`phlookup.rs` — two files that cannot compile and that nobody noticed for years,
because the cycle that forbids them is an accident of the dependency graph that
each person has to rediscover. Stating it makes the mistake structurally
impossible rather than merely discouraged.

### Readiness against the contract

| crate | workspace deps | Discord types | poise wrapper | ready? |
| --- | --- | --- | --- | --- |
| crack-voting | none | none | n/a — own `main.rs` | **today** |
| crack-bf | none | none | `commands/bf.rs` | **today** |
| crack-osint | `crack_types::Error` ×1 | `phcode.rs`, `phlookup.rs` | `commands/osint.rs` | one line + two rotted files |

Verified by grep: `crack-bf/src` and `crack-voting/src` import no `crack_core`,
no `crack_types`, no `poise` and no `serenity`. crack-osint's single workspace
import is `crack_types::Error` at `crack-osint/src/checkpass.rs:1`, and
`crack_types::Error` is `Box<dyn StdError + Send + Sync>` — byte-identical to the
`Error` alias crack-osint already declares in its own `lib.rs:70`. The migration
the owner started is therefore already finished for two of the three, and one
line short for the third.

## 2. Scope

### Dies: crack-gpt

Deleted outright (owner decision): a very thin wrapper that is not wired in and
requires the operator to spend their own OpenAI tokens. The surface is ~20 sites,
every one behind `#[cfg(feature = "crack-gpt")]`, so nothing that ships changes:

- `crack-gpt/` (268 lines) and `crack-core/src/commands/chatgpt.rs`
- `crack-core/Cargo.toml:30` (feature) and `:79` (optional dep)
- `crack-core/src/commands/mod.rs:4-5, 20-21, 60` (module, re-export, `chat()` in
  `all_commands()`)
- `crack-core/src/lib.rs:26-27` (import), `:410-411` (`gpt_ctx` field), `:440-441`
  (Display), `:474-477` (`with_gpt_ctx`), `:607-608` (construction)
- `crack-core/src/errors.rs:56, 169` (error variant and its Display arm)
- `Cargo.toml:6` (workspace member) and `:123` (commented line)
- `scripts/run_one_test.sh:4, 22`

Two incidental gains: it removes one of the four `ctx.defer()` call sites that
#535 is trying to reason about, and it drops `async-openai` from `Cargo.lock`.

### Moves now: crack-voting, crack-bf, crack-osint

All three are dormant — nothing that ships depends on them — so extraction
carries no risk of disrupting live work.

### Deferred, with triggers

Both satisfy the contract already. They are deferred because **the cost of
extraction scales with change rate, not with size or contract-compliance**: a
dormant crate costs nothing to move, while a crate under active development pays
a push-and-bump cycle on every change.

- **crack-musicreco** (4529 lines). The autoplay hot path, recently built, still
  carrying deferred follow-ups, and with exactly one consumer. It has no external
  contract to track, so the versioning argument barely applies.
  **Trigger:** extract when it goes a release cycle without changes, or when a
  second consumer appears.
- **crack-sleevenote** (1580 lines). The strongest versioning case of any of
  them — its own docs say it "models the v0.1.0 wire contract and nothing else",
  so its correct version is a function of *sleevenote's wire contract*, not of
  the bot. Today it bumps with the bot and never with the service, which is
  exactly backwards. Deferred anyway, at the owner's direction, until the dormant
  extractions have proven the pipeline.
  **Trigger:** revisit after crack-osint lands. An open sub-decision at that
  point: its own repo, or inside `cycle-five/sleevenote` beside the service whose
  contract it mirrors (which would grow a Rust job in that repo's CI).

### Stays

`crack-core`, `crack-types`, `crack-cli`, plus the two deferred crates.

## 3. Mechanics

### Dependency form: git, pinned by tag

```toml
crack-osint = { git = "https://github.com/cycle-five/crack-osint", tag = "v0.1.0", optional = true }
```

`Cargo.lock` pins the exact commit. The feature declaration is unchanged —
`crack-osint = ["dep:crack-osint"]` — so the host-it-yourself story is untouched:
`cargo build --features crack-osint` still works and cargo fetches the
dependency. No submodules, and nothing extra for anyone cloning the repo.

**The door this closes:** crates.io refuses a crate with git dependencies, so
`cargo publish` of cracktunes is unavailable until the extracted crates are
themselves published. That costs nothing today — nothing in the workspace has
ever been published, and distribution is cargo-dist binaries plus Docker images.
Switching a git dep to a version dep later is a one-line manifest change per
module, so this is reversible.

### Local development: a gitignored path override

The two-repo dance is the real cost of extraction, and this is the mitigation.
Check the repos out side by side and add a **gitignored** `.cargo/config.toml`
with a path override pointing at the local checkout. Development then proceeds
exactly as it does today — one `cargo build`, edits visible immediately, no
pushing. When finished: commit both, tag the module, bump the tag in cracktunes.

Because the override is gitignored it never leaves the machine, so CI and every
other clone always build the pinned version.

The mechanism is cargo's `[patch]` table keyed by the git URL, which is the
documented way to redirect a git dependency:

```toml
# .cargo/config.toml — gitignored, never committed
[patch."https://github.com/cycle-five/crack-osint"]
crack-osint = { path = "../crack-osint" }
```

`[patch]` in `Cargo.toml` would be committed and would break every other clone,
so it must live in the config file. The first extraction confirms that cargo
honours `[patch]` from `.cargo/config.toml` on the installed toolchain and
documents the result in `docs/`; the legacy `paths` key is the fallback if it
does not.

### History comes along

`git subtree split -P crack-osint -b export` produces a branch containing only
that directory's commits; push it as the new repository's initial history.
crack-osint's oldest file dates to 2023-08-01 and that history is worth keeping.

### Versioning

Each extracted crate gets its own version line starting at **0.1.0**. None was
ever published, so inheriting 0.13.0 would claim thirteen minor releases of a
crate nobody has installed.

The cracktunes workspace keeps `version.workspace = true` for the members that
remain — #538's one-line release bump is unaffected, and the tag-versus-version
guard in `docker.yml` continues to work unchanged.

**No version bump on any PR in this arc.** The cadence's rule is to skip the
bump for work that does not ship an artifact, and none of these changes reaches
the binary — every affected crate was already absent from it. Four PRs bumping in
sequence would spend 0.14.0 through 0.17.0 on a release series in which the
shipped bot is byte-for-byte equivalent. The arc is instead recorded in the notes
of whatever version next ships for a real reason.

This is the one place the spec departs from the usual loop, so it is worth the
owner's explicit agreement: the alternative is a single minor bump on the last PR
of the arc, marking the removal of crack-gpt as a dormant capability.

### CI and Docker

No changes required. No submodules means no checkout edits across the workflows.
cargo-chef treats a git dependency like any other — it is pinned in `Cargo.lock`,
so it lands in the recipe and the cook layer normally, and invalidates that layer
only when the pin moves, which for dormant crates is rare.

The workspace also shrinks. `cargo test --workspace` currently compiles
crack-osint (1015 lines), crack-bf (299) and crack-voting (345) on every PR
despite nothing shipping them, plus the 268 that crack-gpt deletes.

## 4. Order and per-module detail

**1. crack-voting.** First, because crack-core never references it, making this
the purest test of the pipeline with zero integration risk. It is already a
standalone binary (`src/main.rs` → warp server → postgres via sqlx) that reaches
the bot through the shared database. Remove the workspace member entry, the
optional dep and the unused `crack-voting` feature from crack-core.

**2. crack-bf.** Tiny and contract-clean. `commands/bf.rs` stays in crack-core as
the poise wrapper. Note for the new repo, not a blocker here: it runs arbitrary
user programs and wants a step limit, a memory cap and a timeout.

**3. crack-osint.** Needs work before it can go:

- Replace `use crack_types::Error;` (`checkpass.rs:1`) with the crate's own
  `Error` alias, removing the last workspace dependency.
- `whois-rust` is an unused dependency — its only consumer, `whois.rs`, is
  commented out of `lib.rs`. Drop it or revive the module.
- The ~474 uncompiled lines (`ip.rs`, `ipv.rs`, `paywall.rs`, `phcode.rs`,
  `phlookup.rs`, `socialmedia.rs`, `wayback.rs`, `whois.rs` and their tests) move
  as-is with their history and are triaged in the new repo, where they are not
  cluttering the bot.
- `phcode.rs` and `phlookup.rs` cannot compile there under rule 2. Their logic
  becomes pure functions taking plain data, with any command wrapper living in
  crack-core — or they are deleted.
- `phlookup.rs` additionally must not be revived as written: it builds
  `http://apilayer.net/api/validate?access_key={key}&...`, putting the Numverify
  credential in a query string over plain HTTP. If revived it needs HTTPS and the
  key out of the URL. **This gets its own issue** rather than riding along here.

## 5. Out of scope

- **`/phcode` revival.** A separate, already-designed piece of work; see Related
  issues. It is unaffected by this spec either way, because under rule 2 its
  poise wrapper belongs in crack-core regardless of where `crack-osint` lives.
- **Turning the osint feature on.** Whether `/scan`, `/checkpass` and
  `/virustotal_result` should ship is a product question, untouched here.
- **crack-testing's name.** It ships, and it should: despite the name it holds
  the bot's track resolution, not tests. Renaming it is #550 and is deliberately
  kept out of this arc, which is already touching several manifests.
- **crack-bf resource limits.** Recorded above as a note for the new repo.

## 6. Risks

- **The two-repo dance.** Mitigated by the path override, but it is real: a
  change spanning crack-core and an extracted module is two commits, a tag and a
  pin bump. This is the main cost being accepted in exchange for independent
  versioning.
- **Git availability at build time.** Cargo fetches git dependencies from GitHub
  during the build, including inside the Docker builder. A GitHub outage breaks
  builds that a vendored crates.io registry would survive. Low impact given
  current practice; the crates.io path remains open.
- **The path override leaking into a commit.** If `.cargo/config.toml` is ever
  committed, CI silently builds a local path that does not exist there. It must
  be gitignored in the same commit that introduces it.
- **Dead code moving rather than dying.** Extracting crack-osint relocates ~474
  uncompiled lines instead of resolving them. That is deliberate — the new repo
  is the right place to triage them — but it is a deferral, not a fix, and should
  be tracked as an issue in the new repository.

## 7. Related issues

- **#472** — the `#[allow(dead_code)]` sweep. Three of its entries
  (`crack-osint/src/phlookup.rs:6`, and `crack-core/src/lib.rs:299` and `:301`)
  sit in this cluster. `phlookup.rs:6` guards a struct in a file that is not even
  compiled.
- **#523** — tests that reach the live network. Its worst offender,
  `test_phone_code_data`, exercises `PhoneCodeData::load()`, which no production
  code path calls: both construction sites use `PhoneCodeData::default()`.
- **#535** — deleting crack-gpt removes `commands/chatgpt.rs:21` from the four
  `ctx.defer()` sites this issue has to reason about.
- **#549** — phlookup's Numverify credential in a query string over plain HTTP.
  Latent, since the module does not compile; it blocks any revival.
- **#550** — `crack-testing` is misnamed. It is not a test crate: it holds
  `ResolvedTrack`, `CrackTrackClient`, `suggestion2` and `fetch_playlist`, 2014
  lines of track resolution that the bot uses at runtime and correctly ships.
  The name raised a false alarm during this very design, which is the argument
  for renaming it.
