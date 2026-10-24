# topology

Maps a Cargo workspace into facts (crates, modules, items, references) from
rust-analyzer's SCIP export plus `syn`, and says where the seams are. Standalone:
its own `[workspace]` and `Cargo.lock`, never part of the cracktunes root workspace.
Reusable on any Cargo workspace; everything specific to one (which crates count as
Discord, which structs are wide) lives in a config file.

## Questions it answers

Each answer carries file:line evidence.

1. Which units are free of a dependency group (Discord, by default) today, including
   everything they use? These can move as they are.
2. For each unit that touches a group: where does it come in (symbol and line), and
   is it in the signature or only the body?
3. Which units are one or two edges from clean? These are the cheap seams.
4. Which module cycles would stop a set of modules from being cut into a crate?
5. Which modules use which fields of the wide structs (`DataInner`)? The "god object" view.
6. Which modules pull in which groups (database, HTTP, coin, media, ...)?
7. How long does each crate take to build, and what does a rebuild of the big one cost?
8. Which `.rs` files are on disk but compiled by no build (orphans)?

## Commands

Run from the repo root through the wrapper, which builds in release mode and passes
`--workspace`, `--config tools/topology/topology.toml` and `--out tools/topology/out`:

```shell
scripts/topology.sh extract --summary   # rust-analyzer + syn -> topology.json, digest.json, one-screen summary
scripts/topology.sh extract --scip FILE # reuse an existing index instead of running rust-analyzer
scripts/topology.sh page                # out/page/topology.html, one self-contained file
scripts/topology.sh timings             # optional: per-crate and rebuild times -> out/timings.json
scripts/topology.sh page                # again, to put the timings on the page
scripts/topology.sh diff A B [--json]   # compare two digests.json (or topology.json files)
```

Pipeline: `extract --summary` -> `page` -> `timings` (optional; a full all-features build,
into `$HOME/.cache/crack-topology/timings-target`, never the shared target dir).
`--repeats N` sets how many timing repeats are taken (the median is reported).

Output goes to `tools/topology/out/` (gitignored). `topology.json` has every fact with
line numbers; `digest.json` has the facts without them, sorted, so moving a whole item
within a file leaves it byte-identical, and two runs on the same tree are byte-identical.
Container hashes (module, impl, trait) cover the whole item in source order, so
reordering items inside a container marks the container as changed.
Timings live in their own file and never enter the digest.

## Comparing before and after a split

Keep the digest from before: `cp tools/topology/out/digest.json /somewhere/before.json`.
After the split, run `extract` again and:

```shell
scripts/topology.sh diff /somewhere/before.json tools/topology/out/digest.json
```

It reports units added, removed, changed and moved (same kind, name and hash under a new
id; positional `#N` ids of one base, such as the 18 `<impl From for E>` blocks, are paired by hash first), taint transitions per group (what became clean), lines per state per module and
per crate lines per state (headline group first), modules that joined or left a cycle, and
wide-struct changes. Modules that exist on one side only are listed with a zero
on the other. It always exits 0. The first lines say if the tool, rust-analyzer or the
config differ between the runs; then a difference may come from the tooling, not the code.

## States and `cuts`

For each group, each non-test unit is in exactly one state:

- **clean**: neither it nor anything it uses, transitively through workspace units,
  touches the group.
- **signature**: its signature mentions a group symbol or a tainted workspace type
  (parameters, return type, generics, where-clauses; struct fields; enum variants; an
  alias target; trait supertraits and item signatures; an impl header; attributes, so
  `#[poise::command]` counts).
- **body**: only its body touches the group, directly.
- **inherited**: it touches the group only through workspace units it uses.

Workspace types are tainted by fixpoint: a struct, enum, alias or trait is tainted when
its definition mentions a group symbol or another tainted type. That is how a re-export
like `pub use serenity::all::UserId` carries Discord into everything that names it.

**`cuts`** is how far a unit is from clean: the number of distinct source lines that
reference the group directly plus the number of distinct tainted workspace units it
uses. A clean unit has 0. Seams are ranked by `cuts`, then by lines, so the cheapest and
biggest come first. Impl blocks are not seams: an `impl` header has one cut, its self
type, and its lines include all its methods. The self type and each method have their own
rows.

A trait is tainted by its items: when one item's signature mentions a group symbol or a
tainted type, the trait is `signature` and its `via` lists those items. Functions
generic over the trait, and impls of it, are then `signature` too.

## The two path rules

SCIP resolves a symbol to the crate that defines it. Two rules decide what counts:

- A path segment that names a group crate counts as a reference to that group, even when
  the symbol at the end is defined elsewhere. `serenity::small_fixed_array::FixedString`
  written inside a function is Discord evidence for that function.
- A symbol reached through a plain `use` resolves to its defining crate. A function that
  only names `FixedString` after a module-level `use serenity::small_fixed_array::FixedString`
  is not Discord on that account; the `use` belongs to the module. Add `small-fixed-array`
  and `nonmax` to the group in the config if you want them to count.

## Warnings

Nothing is guessed or silently dropped; what the tool cannot place is listed by kind:

- `unindexed_file`: a file in the module tree that rust-analyzer produced no document for.
- `unparsed_file`: `syn` could not parse a file, or a crate root or `mod x;` file is missing.
- `occurrence_outside_tree`: references in a file no `mod` reaches (macro output, generated files).
- `unmapped_definition`: a definition no unit's span contains.
- `unmapped_reference`: a reference to a workspace symbol no unit defines (macro-generated items such as `sqlx::query!` record structs). `Self` and derived members map to their self type, not here.
- `ambiguous_definition`: one symbol defined twice in a crate; the smallest unit id wins.
- `unparsed_symbol`: a SCIP symbol string the tool could not parse.

Unmapped references should stay under 1% of workspace references; the summary prints the counts.

## Config

`topology.toml`, every field optional: `name`, `[groups]` (group -> package names, a
trailing `*` is a prefix glob), `headline_group` (default `discord`), `test_crates`,
`wide_struct_min_fields` (10), `near_clean_max_cuts` (2), `[rust_analyzer]` (`features`,
default `"all"`; `env`), `[timings]` (`rebuild_package`, `through_package`, `repeats`,
`profile`, `target_dir`). Packages in a group are external: no unit edges point into them.

## Traps

- **Default features hide code.** The index is built with `features = "all"`; with default
  features the gated modules (`coin/`, ...) silently vanish. `SQLX_OFFLINE=true` is set in
  the config so `sqlx::query!` expands without a database.
- **About 3,000 `ERROR` lines in rust-analyzer's log are normal** ("enclosing definition
  with no name", duplicate crate-root symbols). They are kept in `out/rust-analyzer.log`
  and counted; only a non-zero exit or a missing index is a failure.
- **A tool or rust-analyzer change makes digests incomparable.** Both versions and the
  config hash are recorded in every output, and `diff` says when they differ.
- Orphans (files no `mod` reaches with all features on) compile in no build and are not
  units; they are listed with their line counts.
- `extract` takes a couple of minutes and a few GB; `timings` is a full cold build. Run
  both under a memory cap.
- Never `cargo clean` the shared target dir; timings use their own.
