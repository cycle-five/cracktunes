# Rust topology map: finding crack-core's seams

2026-10-24. Agreed with the owner in chat. The owner's go: "Looks good, go ahead and continue
autonomously until PR."

## Why

The owner wants to "find some seams that make sense in crack-core and separate them out into
different modules, keep core really core and cut down compile times." Asked what "really core"
means: "Separation of concerns, discord specific as segregated as can be, or at least all useful
business logic detangled from discord specific."

Three reasons, in order:

- **Separation of concerns.** The useful rules (queue decisions, the economy, permissions,
  playlists, …) should not need serenity, poise or songbird to compile or to test.
- **Compile times.** crack-core is 116,596 lines in 279 files, about 85% of the workspace's Rust
  (2026-10-24). Every edit to it rebuilds all of it and everything that depends on it.
- **Mutation testing.** "The time it was going to take to generate all the mutants at once was
  prohibitive." The full run was measured at about 35 hours (7,732 mutants), and the owner
  declined it (2026-10-22, `2026-10-22-test-tooling-design.md`). cargo-mutants builds and tests
  only the package a mutant lives in. A seam cut out of crack-core into its own crate, with its
  tests beside it, makes every mutant in it cheaper.

This is the same direction as the Matrix investigation (`docs/investigations/2026-10-22-matrix-music.md`):
rules take plain values in and give decisions out, Discord stays at the edges, and the
crack-types rethink splits platform-neutral types from Discord glue. Matrix itself stays out of
scope; this keeps that door open.

**This document covers step 0 only: the map.** The RuneCast frontend refactor started the same
way, with a deterministic topology tool (`scripts/topology/` in runecast-frontend, fe #230 and
#234) that was re-run and diffed after every refactor step. That tool reads TypeScript, so it
cannot read cracktunes. The owner: "Build the Rust version, we'll reuse it after each split."
The splits themselves get their own design after the owner has looked at the first map.

## Goals

1. **A repeatable extraction** of the workspace's Rust code into facts: units, edges between
   them, which ones touch Discord (and the other heavy dependency groups), module cycles, and who
   uses the fields of the wide structs. No network, no judgement calls.
2. **The tool answers these questions,** each with file:line evidence:
   1. Which crack-core units are Discord-free today, including everything they use? These can
      move as they are.
   2. For each unit that touches Discord: where exactly does Discord come in (the symbol and the
      line), and is it in the signature or only in the body?
   3. Which units are only one or two edges away from Discord-free? These are the cheap seams.
   4. Which module cycles would stop a set of modules from being cut out into a crate?
   5. Which modules use which fields of `DataInner` (29 public fields today) and the other wide
      structs? This is the "god object" view.
   6. Which modules pull in which heavy dependency groups (database, HTTP, coin, media, …)?
   7. How long does each workspace crate take to build, and what does a crack-core rebuild cost?
   8. Which source files are on disk but compiled by no build at all?
3. **A digest and a diff,** so each split can be checked: run the tool before and after, and the
   diff says what moved, what changed, and what became Discord-free. Moving code without changing
   it must read as a move, not as a removal plus an addition.
4. **A report page:** one self-contained HTML file to review the map on. After the first run it
   is published as a private Artifact for the owner.
5. **Reusable on any Cargo workspace.** cracktunes is the first user. Anything specific to it
   (which crates count as Discord, which structs are wide) lives in a config file, not in code.

## Non-goals

- **Splitting anything.** This PR builds and runs the map. Which seams to cut, and in what order,
  is decided with the owner after the first map, through a new brainstorm.
- **TypeSafe labels.** RuneCast's tool also asked TypeSafe which system each unit belongs to.
  Here the main question, "does this touch Discord?", is a fact the compiler can answer, so v1 has
  no labeller, no network and no API key. A labeller can be added later if the module paths turn
  out not to be enough. *(My call.)*
- **A CI gate or a lint.** The tool reports; it never fails a build.
- **Matrix, or any other platform work.**
- **Changing the bot.** Tooling only: no version bump, no tag, no release, no deploy.

## Anti-goals: it fails if…

- **A fact is guessed or silently dropped.** When the tool cannot resolve a reference, or cannot
  place a reference inside a unit, it says so in a warnings list with file:line. RuneCast's lesson
  (fe #234): a refactor that narrows a type made the extractor drop edges without a word, and the
  diff looked like progress.
- **It slows the bot's build.** The tool adds nothing to the workspace: no new member, no change
  to the root `Cargo.lock`, no time on ordinary PRs, no new mutants.
- **Two runs on the same tree differ.** Output is sorted and has no timestamps; timings live in a
  separate file that the digest ignores.
- **It breaks a standing rule.** Never `cargo clean` (or delete) the shared target dir
  `~/.cache/cargo-target`; scratch goes under `$HOME/.cache`, never `/tmp`; typed serde only.

## Constraints

- **Where:** `tools/topology/` in the cracktunes repo, a standalone Cargo package with its own
  `[workspace]` table and its own `Cargo.lock`, so the root workspace never sees it and its
  manifest stays untouched. Stable toolchain, edition 2024. *(My call: in-repo because cracktunes
  is the first user; workspace-agnostic so it can move to its own repo later.)*
- **Run as** `cargo run --release --manifest-path tools/topology/Cargo.toml -- <command>`, with a
  short wrapper script in `scripts/`.
- **Typed serde for every JSON file** (the owner's standing rule): no `serde_json::Value` or
  `json!` for data the tool owns.
- **Facts come from two sources:**
  - **rust-analyzer's SCIP export** (`rust-analyzer scip`, from the `rust-analyzer` rustup
    component) for resolution: what every identifier in the source refers to, across crates,
    through `use`, re-exports, type aliases, trait methods and macro call sites.
  - **`syn`** (with span locations) for structure: items, their spans, which part of an item is
    its signature and which is its body, struct fields, `cfg` attributes.

  Each SCIP occurrence is placed in the innermost `syn` unit whose span contains it, and tagged
  as signature or body by where it falls. *(My call: SCIP is a stable interchange format, where
  rust-analyzer's own library crates change their API weekly; syn alone cannot resolve a method
  call or a re-export.)* The rust-analyzer version is recorded in every output.
- **Index everything that builds:** all features of the workspace crates on (crack-coin, osint,
  db-tests, mutation-testing, …) through `rust-analyzer scip --config-path` with
  `{"cargo": {"features": "all"}}`, and `SQLX_OFFLINE=true` so `sqlx::query!` expands without a
  database. With default features the index silently leaves out `coin/` and the other gated
  modules; with all features it took 136 s and covered every crack-core file a `mod` reaches.
- **Measured in a spike on master `63a15373` (2026-10-24, rust-analyzer 1.99.0):**
  - `rust-analyzer scip` on the whole workspace took 130 s and wrote a 26 MB index: 270
    documents, 263,364 occurrences, 35,960 definitions.
  - Every definition carries its enclosing range.
  - External symbols name their crate and version, e.g.
    `rust-analyzer cargo serenity 0.12.5 gateway/client/impl#[Client]data().`, so the crate → group
    mapping is a string match on the third field.
  - References inside `#[poise::command]` bodies resolve: 3,496 Discord references across 75
    command files.
  - rust-analyzer logs about 3,000 `ERROR` lines on a healthy run ("enclosing definition with no
    name", "definition … should have been in an SCIP document but was not", duplicate symbols for
    crate roots). The tool keeps rust-analyzer's log, counts these, and treats only a non-zero exit
    or a missing index as failure.
- **Test code is recorded and marked,** never mixed into the ranking: `#[cfg(test)]` modules,
  `#[test]`/`#[tokio::test]` functions, `tests/` directories and dev-only crates
  (crack-testing, crack-test-macros). A split moves tests with their code, so the map shows which
  tests touch Discord, separately.
- **CI:** one path-filtered workflow builds and tests the tool when `tools/topology/**` changes,
  with the rust-analyzer component installed. The required `Mutants / Mutants verdict` check must
  still pass on this PR: its diff includes every `*.rs` file, and cargo-mutants only sees
  workspace packages, so a diff under `tools/` should plan zero mutants. Check that locally before
  pushing. The root workspace lists its members explicitly, so the other workflows (lint, clippy,
  coverage) never see `tools/`; dependabot is not pointed at the tool's lockfile in v1 *(my call:
  a local dev tool, not shipped)*.
- **Timings** are measured in a dedicated target dir under `$HOME/.cache`, never by cleaning the
  shared one.

## What the tool records (decided)

### Units

Every crate, module (file or inline), free function, method (with its `Self` type, and the trait
for a trait impl), struct, enum, union, trait, type alias, const, static and `macro_rules!`.

Each unit has:
- a stable id: the crate plus its module path and name, e.g. `crack_core::music::queue::shuffle`;
  methods include their impl's self type, and trait methods their trait;
- its kind, file, line span and line count;
- whether it is test code, and the `cfg(feature = …)` guards around it;
- a content hash over its tokens (whitespace and comments ignored), used to recognise a move.

### Edges

- **uses:** unit → unit, from SCIP references inside the unit, each tagged **signature** or
  **body**. Signature means a function's parameters, return type, generics and where-clauses;
  a struct's fields and an enum's variants; a type alias's target; a trait's supertraits and
  item signatures; an impl's header; and the item's attributes (so `#[poise::command]` counts as
  Discord in the signature).
- **external:** unit → (crate, symbol), aggregated with a count and the first file:line.
- **implements:** impl → trait and impl → self type.

### Dependency groups and taint

The config maps external crates to named groups. Discord is the headline group (serenity, poise,
songbird and their satellite crates); the others start as database (sqlx), HTTP (reqwest),
coin (crack-coin and its Solana stack) and media (symphonia, audio, yt-dlp wrappers). Every group
gets the same analysis; the page leads with Discord.

For each group, each non-test unit is in exactly one state:

- **clean:** neither the unit nor anything it uses, transitively through workspace units, touches
  the group;
- **signature:** the unit's signature mentions a group symbol or a tainted workspace type;
- **body:** only the unit's body touches the group, directly;
- **inherited:** the unit touches the group only through workspace units it uses.

Workspace types are tainted by fixpoint: a struct, enum, alias or trait is tainted when its
definition mentions a group symbol or another tainted type. This is how `crack_types`'s
re-exports (`pub use serenity::all::UserId`) and aliases (`SongbirdCall = Arc<Mutex<songbird::Call>>`)
carry Discord into everything that names them.

SCIP resolves a symbol to the crate that **defines** it, so two rules decide what counts *(my
calls)*:
- **A path segment that names a group crate counts as a reference to that group,** even when the
  symbol at the end is defined elsewhere. serenity re-exports `small_fixed_array` and `nonmax`, and
  `serenity::small_fixed_array::FixedString` written inside a function is Discord evidence for that
  function, because the code as written needs serenity.
- **A symbol reached through a plain `use` resolves to its defining crate.** A function that only
  names `FixedString` after a module-level `use serenity::small_fixed_array::FixedString` is not
  Discord on that account; the `use` belongs to the module unit. `small-fixed-array` and `nonmax`
  are not in the Discord group by default; the config can add them.

Occurrences that come from macro expansion and cannot be placed inside a unit go to the warnings
list, never silently to the enclosing module.

Every tainted unit carries its **evidence**: its direct group references (symbol, file:line) and
the tainted workspace units it uses. Units are ranked by how few edges stand between them and
clean, so the cheapest seams come first. Modules are summarised by lines in each state.

### Cycles

The module dependency graph inside each crate (aggregated from unit edges, test code excluded),
its strongly connected components larger than one module, and for each component the edges that
close it, with file:line.

### Orphan files

`.rs` files under a crate's source tree that no module declaration reaches with all features on.
They compile in no build, so they are not units; the page lists them with their line counts.
The spike found 52 in crack-core, 3,601 lines: `commands/mod.rs` has `//pub mod playlist;` and
`//pub mod settings;`, `commands/admin/mod.rs` comments out 17 of its submodules, and
`guild/cam_rules.rs` is declared nowhere. (Deleting them is a split-arc decision, not this PR's.)

### Wide structs

For every struct with at least a configured number of fields (default 10, which catches
`DataInner`), a field × module matrix of uses, and the functions behind each cell.

### Timings (a separate command)

For each workspace crate, the time to compile that crate alone with its dependencies already
built, and the wall time of a full crack-core rebuild through to the bot binary. Incremental
compilation off, so a run measures real work; a few repeats with the median reported; the
rustc version, profile, job count and machine recorded beside the numbers. Timings go to their
own file and never into the digest.

## Digest and diff

- **digest.json** holds the facts without line numbers, sorted, one entry per line: unit ids with
  kind and hash, edges counted per kind and endpoint, each unit's state per group, cycles, the
  wide-struct matrices, the warnings count. Moving code within a file leaves it byte-identical.
- **diff** compares two digests (or two full fact files) and reports units added, removed,
  changed and **moved** (same kind, name and hash under a new id; same kind and name with a new
  hash is reported as moved-and-changed), taint transitions per group (which units became clean),
  lines per state per module before and after, cycle changes and wide-struct changes. It always
  exits 0. `--json` prints the typed diff.
- Each output records the tool's own source hash, the rust-analyzer version and the config hash.
  The diff prints a note when they differ between the two runs, because then a difference may come
  from the tool rather than the code.

## Report page

One self-contained HTML file (data, CSS and script inline; nothing fetched), readable in light and
dark and at phone width. It shows structure, not source text. Views:

- **Overview:** crates with lines, the Discord split, timings when present.
- **Modules:** a sortable, filterable table with a bar of lines per state, per group.
- **Unit detail:** state, evidence with file:line, uses and used-by.
- **Seam candidates:** the near-clean units and modules, cheapest first.
- **Cycles.**
- **Wide structs:** the field × module matrices.
- **Dependency groups:** which modules reach which groups.
- **Warnings:** unresolved references and unplaced occurrences.

## Commands

- `extract`: runs rust-analyzer (or reads a given `.scip` file), writes `topology.json` and
  `digest.json`; `--summary` prints the headline numbers.
- `diff <a> <b>`.
- `page`: writes the report page.
- `timings`.

Output goes to `tools/topology/out/` (gitignored). The config is `tools/topology/topology.toml`,
committed.

## Testing

Small fixture workspaces under `tools/topology/tests/fixtures/`, each a real Cargo workspace with
a stand-in "discord" crate, cover the taint states, re-exports, type aliases, trait methods,
macro call sites, cfg-gated code, cycles, wide structs, test code, and a move between crates for
the diff. Tests run the real `rust-analyzer scip` on them. Unit tests cover the pure parts
(placement of occurrences into units, the fixpoint, the ranking, the digest, the diff).

## The first run, before the PR is opened

Run the tool on master, put the headline numbers in the PR body, and publish the page as a
private Artifact. Then stop: the owner reviews the map, and the split design starts from it.

## Left to the builder

The JSON schema's exact shape, flag names, the page layout and any charting, how tokens are
normalised for the hash, the exact ranking formula, the mechanics of placing occurrences, and how
timings are collected from cargo.
