# Rust Topology Map Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A standalone Rust tool in `tools/topology/` that maps any Cargo workspace into deterministic facts (units, edges, per-group taint, cycles, wide structs, orphans, timings), diffs two maps, and renders a self-contained review page — first used to find crack-core's Discord seams.

**Architecture:** `rust-analyzer scip` resolves every identifier; `syn` gives structure (items, spans, signature vs body). Occurrences are placed into syn units by position, aggregated into a graph, and analysed (taint fixpoint, SCCs, field matrices). One typed model (`model.rs`) is written as `topology.json`; a line-free `digest.json` feeds the diff; the page is computed in Rust and only rendered by its JS.

**Tech Stack:** Rust stable, edition 2024. Crates: `scip` 0.10 (+ its `protobuf`), `syn` 2 (`full`, `visit`, `extra-traits`), `proc-macro2` 1 (`span-locations`), `cargo_metadata` 0.23, `clap` 4 (`derive`), `serde` + `serde_json`, `toml`, `sha2`, `anyhow`, `walkdir`. External binary: `rust-analyzer` from the rustup component.

**Spec:** `docs/superpowers/specs/2026-10-24-rust-topology-design.md` — read it first; it is the authority. This plan argues from it.

## Global Constraints

- Everything lives under `tools/topology/` (package `rust-topology`, binary `topology`, lib `rust_topology`), a standalone package with its own empty `[workspace]` table and its own committed `Cargo.lock`. **Never edit the root `Cargo.toml` or root `Cargo.lock`.**
- Edition 2024, stable toolchain. `tools/topology/clippy.toml` exists (shadows the root one, whose songbird rules do not apply here). `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass for the package.
- Typed serde for every JSON/TOML file: no `serde_json::Value`, no `json!` for data the tool owns. Maps are `BTreeMap`/`BTreeSet`; every `Vec` written to disk is sorted. Two runs on the same tree produce byte-identical `topology.json` and `digest.json`.
- `extract` never touches the network. rust-analyzer runs as `rust-analyzer scip <workspace> --config-path <out>/ra-config.json --output <out>/index.scip`, cwd = workspace root, with `ra-config.json` = `{"cargo": {"features": "<config.rust_analyzer.features>"}}` (default `"all"`) and the config's `rust_analyzer.env` added to its environment. Binary: `$TOPOLOGY_RUST_ANALYZER` if set, else `rust-analyzer` on `PATH`. Only a non-zero exit or a missing index is failure; its stderr goes to `<out>/rust-analyzer.log` and lines containing ` ERROR ` are counted.
- SCIP `position_encoding` 1 = UTF-8 byte offsets from line start, 0-based lines. syn/proc-macro2 columns are 0-based **chars**, lines 1-based. All internal positions are `Pos { line: 1-based, col: 0-based chars }`. Any other SCIP encoding → error naming it.
- All file paths in output are relative to the workspace root with `/` separators, whatever the cwd.
- Never `cargo clean` or delete `~/.cache/cargo-target`. Test scratch lives in `env!("CARGO_TARGET_TMPDIR")`. The timings target dir defaults to `$HOME/.cache/crack-topology/timings-target`. Mutation scratch: `TMPDIR=$HOME/.cache/cargo-mutants`; never `--in-place`.
- Commits: `git add <paths>` only (never `-A`), message ends with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line. Tooling only: no version bump anywhere.
- **Mutation proof** for each task's pure-logic files (named in the task): `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue -- env TMPDIR=/home/lothrop/.cache/cargo-mutants CARGO_TARGET_DIR=target cargo mutants --dir tools/topology --file <files> -j 4 -- --lib --test fixture` (the RELATIVE `CARGO_TARGET_DIR` gives each scratch copy its own target dir; without it the machine's global `~/.cache/cargo-target` is shared by every copy and by the real tree, and copies overwrite each other's test binaries — false MISSED and stale mutated binaries in the real tree, seen in Task 3). Every missed mutant gets a test or a one-line waiver with its reason in the task report.
- **Memory cap (R8).** Run every local `cargo test` and every mutation proof inside a memory-capped scope, so a runaway gets killed alone instead of the whole session: `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue -- <command>`. systemd-run expands `$VAR` in its arguments, so write paths out in full (`TMPDIR=/home/lothrop/.cache/cargo-mutants`), and put env vars after `-- env`. Why: a mutant that turns a loop counter's `+=` into `*=` keeps pushing to a Vec at gigabytes per second until cargo-mutants' 20 s timeout. With `-j 4`, that filled 31 GB of RAM and 31 GB of swap, and systemd-oomd killed the whole session twice (Task 6). Under the cap, the kernel kills only the mutated test binary, and cargo-mutants counts it as caught.

## Shared vocabulary (model.rs, Task 1)

Unit ids are strings: crate id + module path + name, joined with `::`.
- **Crate id** = target name with `-` → `_`; a bin whose package also has a lib of the same normalised name gets `[bin]` appended (`cracktunes[bin]`); test targets `<name>[test]`, examples `<name>[example]`, benches `<name>[bench]`; build scripts are not crates.
- Module: `appcore::rules`; crate root module = crate id (`appcore`). Item: `appcore::rules::greet`. Inherent method: `appcore::handlers::Bot::ping`. Trait-impl method: `appcore::handlers::<Handler as EventHandler>::on_event`. Trait item: `fakecord::EventHandler::on_event`. Impl block: `appcore::handlers::<impl EventHandler for Handler>` / `<impl Bot>` (self type and trait = last path segment, generics dropped; non-path self types use their tokens without spaces). Macro invocation item: `appcore::state::lazy_static!`. Any duplicate id in a crate (cfg alternatives, two `impl Bot` blocks) gets `#2`, `#3`… in source order.
- Items nested inside function bodies are part of that function, not units.

## Review Focus

1. **Non-ASCII text before an identifier on the same line** (cracktunes' messages are full of emoji): a definition must still map to its own unit, not a neighbour. → Task 4 `emoji_line_maps_to_after_emoji`.
2. **A package whose lib and bin share a name** (`cracktunes`): two crates, no id collision, and references from the bin resolve to the bin's own items first. → Task 1 `crate_ids_for_fixture`, Task 4 `bin_reference_prefers_own_crate`.
3. **Run from another directory**: output paths stay workspace-relative. → Task 7 `extract_from_other_cwd_has_relative_paths`.
4. **Tool or rust-analyzer version differs between two digests**: the diff says so before anything else. → Task 8 `generator_mismatch_is_noted_first`.
5. **The real workspace floods warnings** (macro-generated definitions from sqlx, poise, derives): warnings are counted by kind, unmapped references stay under 1% of workspace references, and the page groups them. → Task 11 real-run gate.

---

### Task 1: Package, config, model, workspace discovery, fixture

**Files:**
- Create: `tools/topology/{Cargo.toml,Cargo.lock,clippy.toml,build.rs,topology.toml,README.md}`, `tools/topology/src/{lib.rs,main.rs,config.rs,model.rs,workspace.rs}`, `tools/topology/tests/fixtures/ws/**` (below), `tools/topology/tests/fixture/main.rs`, `scripts/topology.sh`
- Modify: `.gitignore` (add `tools/topology/out/` and `tools/topology/tests/fixtures/*/target/`), `scripts/README.md` (one line for `topology.sh`)

**Interfaces — Produces:**
- `config::Config` (serde from TOML, every field defaulted): `name: String` (default: workspace dir name), `groups: BTreeMap<String, Vec<String>>` (package names; a trailing `*` is a prefix glob), `headline_group: String` (`"discord"`), `test_crates: Vec<String>` (package names), `wide_struct_min_fields: usize` (10), `near_clean_max_cuts: u32` (2), `rust_analyzer: RaConfig { features: String /*"all"*/, env: BTreeMap<String,String> }`, `timings: TimingsConfig { rebuild_package: Option<String>, through_package: Option<String>, repeats: u32 /*3*/, profile: String /*"dev"*/, target_dir: Option<PathBuf> }`.
- `Config::load(path: Option<&Path>) -> anyhow::Result<Config>`; `Config::group_of(&self, package: &str) -> Option<&str>` (error at load if a package matches two groups); `Config::hash(&self) -> String` (sha256 hex of the file bytes, `"default"` when no file).
- `workspace::Workspace::load(root: &Path, test_crates: &[String]) -> anyhow::Result<Workspace>` via `cargo metadata --no-deps`: `Workspace { root: PathBuf, name: String, packages: Vec<Package> }`, `Package { name: String, version: String, dir: String, test_only: bool /* name in test_crates */, targets: Vec<Target> }`, `Target { crate_id: String, name: String, kind: model::TargetKind, root_file: String }`; `Workspace::is_member(&self, package: &str) -> bool`.
- `model.rs`, all `Serialize + Deserialize + Clone + Debug + PartialEq`, ids `Ord`:
  - `UnitId(String)` (`#[serde(transparent)]`), `TargetKind { Lib, Bin, ProcMacro, Test, Example, Bench }`, `UnitKind { Module, Function, Method, Struct, Enum, Union, Trait, TypeAlias, Const, Static, MacroRules, MacroCall, Impl }`, `Position { Signature, Body }`, `State { Clean, Signature, Body, Inherited }`, `Loc { file: String, line: u32 }`.
  - `Generator { tool_hash: String, rust_analyzer: String, config_hash: String, ra_error_lines: u32 }`.
  - `CrateInfo { id: String, package: String, kind: TargetKind, root_file: String, test_only: bool, lines: u32 }`.
  - `Unit { id: UnitId, kind: UnitKind, krate: String, module: Option<UnitId>, parent: Option<UnitId>, file: String, start_line: u32, end_line: u32, lines: u32, own_lines: u32, test: bool, features: BTreeSet<String>, hash: String }`.
  - `Edge { from: UnitId, to: UnitId, position: Position, count: u32, first: Loc }`, `ExternalRef { from: UnitId, package: String, symbol: String, position: Position, count: u32, first: Loc }`, `Reexport { module: UnitId, target: String, first: Loc }`.
  - `Evidence { package: String, symbol: String, position: Position, count: u32, first: Loc }`, `UnitTaint { state: State, cuts: u32, direct: Vec<Evidence>, via: Vec<UnitId> }`, `StateLines { clean: u32, signature: u32, body: u32, inherited: u32 }`.
  - `Cycle { krate: String, modules: Vec<UnitId>, edges: Vec<ModuleEdge> }`, `ModuleEdge { from: UnitId, to: UnitId, count: u32, first: Loc }`, `Cycles { by_module: Vec<Cycle>, by_top_module: Vec<Cycle> }`.
  - `WideStruct { id: UnitId, fields: Vec<String>, uses: BTreeMap<String, BTreeMap<UnitId, BTreeSet<UnitId>>> }` (field → module → units).
  - `Orphan { package: String, file: String, lines: u32 }`, `WarningKind { UnindexedFile, UnparsedFile, OccurrenceOutsideTree, UnmappedDefinition, UnmappedReference }`, `Warning { kind: WarningKind, message: String, loc: Option<Loc> }`.
  - `Topology { generator: Generator, name: String, groups: Vec<String>, crates: Vec<CrateInfo>, units: Vec<Unit>, edges: Vec<Edge>, external: Vec<ExternalRef>, reexports: Vec<Reexport>, imports: BTreeMap<UnitId, BTreeSet<String>>, taint: BTreeMap<String, BTreeMap<UnitId, UnitTaint>>, module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>>, cycles: Cycles, wide_structs: Vec<WideStruct>, module_groups: BTreeMap<UnitId, BTreeMap<String, u32>>, orphans: Vec<Orphan>, warnings: Vec<Warning> }`.
- `build.rs` sets `TOPOLOGY_TOOL_HASH` = sha256 over the sorted contents of `src/**` and `src/page/assets/**`, with `rerun-if-changed=src`.
- CLI (`main.rs`, clap): global `--workspace <dir>` (default `.`), `--config <file>` (default `<workspace>/topology.toml` if present), `--out <dir>` (default `<workspace>/topology-out`); subcommands `extract [--scip <file>] [--summary]`, `diff <a> <b> [--json]`, `page [--in <file>] [--timings <file>]`, `timings [--repeats <n>] [--target-dir <dir>]`. In this task each subcommand except parsing returns `anyhow::bail!("not implemented yet")`.
- `scripts/topology.sh`: `exec cargo run --release --quiet --manifest-path "$root/tools/topology/Cargo.toml" -- --workspace "$root" --config "$root/tools/topology/topology.toml" --out "$root/tools/topology/out" "$@"` with `root` = the repo root from the script's own location.
- `tools/topology/topology.toml` (cracktunes): `name = "cracktunes"`; groups `discord = ["serenity", "serenity-voice-model", "poise", "poise_macros", "songbird", "discortp", "davey"]`, `database = ["sqlx", "sqlx-*"]`, `http = ["reqwest"]`, `coin = ["crack-coin", "solana-*", "spl-*"]`, `media = ["symphonia", "symphonia-*", "audiopus", "audiopus_sys", "rusty_ytdl"]`; `test_crates = ["crack-testing", "crack-test-macros"]`; `rust_analyzer.env = { SQLX_OFFLINE = "true" }`; `timings.rebuild_package = "crack-core"`, `timings.through_package = "cracktunes"`.

**The fixture workspace** `tools/topology/tests/fixtures/ws/` (exact names; later tasks assert on them). Root `Cargo.toml`: `[workspace] members = ["fakechars", "fakecord-macros", "fakecord", "apptypes", "appcore", "apptesting", "appbin"]`, `resolver = "2"`. Commit its `Cargo.lock`. Every crate edition 2021, path deps only. Plus `ws/topology.toml`: `name = "fixture"`, `groups.discord = ["fakecord", "fakecord-macros"]`, `test_crates = ["apptesting"]`, defaults otherwise.
- `fakechars/src/lib.rs`: `pub struct FixedStr(pub String);`
- `fakecord-macros` (proc-macro, no deps): `#[proc_macro_attribute] pub fn command(_a: TokenStream, item: TokenStream) -> TokenStream { item }`
- `fakecord/src/lib.rs`: `pub use fakecord_macros::command; pub use fakechars;` `pub struct GuildId(pub u64);` `pub struct Context;` `impl Context { pub fn say(&self, _s: &str) {} }` `pub trait EventHandler { fn on_event(&self, ctx: &Context); }`
- `apptypes/src/lib.rs` (dep fakecord): `pub use fakecord::GuildId;` `pub type SharedCtx = std::sync::Arc<fakecord::Context>;` `pub struct Plain { pub n: u32 }`
- `apptesting/src/lib.rs`: `pub fn helper() -> u32 { 1 }`
- `appbin` (lib + bin both named `appbin`): `src/lib.rs` `pub fn run() -> u32 { 0 }`; `src/main.rs` `fn run() -> u32 { 1 }` and `fn main() { let _ = run() + appbin::run(); }`; `tests/it.rs` `#[test] fn it_runs() { assert_eq!(appbin::run(), 0); }`
- `appcore` (deps apptypes, fakecord, fakechars; `[features] extra = []`), `src/lib.rs`: `pub mod rules; pub mod handlers; pub mod state; pub mod ring_a; pub mod ring_b; pub mod text;` `#[cfg(feature = "extra")] pub mod gated;` `#[cfg(test)] mod tests;` `// pub mod orphan;`
  - `rules.rs`: `pub struct Track { pub id: u32 }` · `pub fn pick_next(queue: &[Track]) -> Option<&Track> { queue.first() }` · `pub fn score(t: &Track) -> u32 { t.id * 2 }` · `pub fn in_guild(g: &apptypes::GuildId) -> bool { g.0 > 0 }` · `pub fn uses_alias(c: apptypes::SharedCtx) -> usize { std::sync::Arc::strong_count(&c) }` · `greet` with body on two lines: `let c = fakecord::Context;` / `c.say("hi");` then `String::new()` · `pub fn wrapper() -> String { greet() }` · `pub fn plain(p: apptypes::Plain) -> u32 { p.n }`
  - `handlers.rs`: `pub struct Handler;` · `impl fakecord::EventHandler for Handler { fn on_event(&self, ctx: &fakecord::Context) { let _ = ctx; } }` · `pub struct Bot;` · `impl Bot { pub fn ping(&self) -> u32 { 1 } }` · `#[fakecord::command] pub fn cmd() -> u32 { crate::rules::score(&crate::rules::Track { id: 1 }) }` (attribute on its own line) · `pub fn third(w: &crate::state::Wide) -> u32 { w.f3 }`
  - `state.rs`: `pub struct Wide { pub f1: u32, … pub f10: u32 }` (10 named fields) · `pub fn read_a(w: &Wide) -> u32 { w.f1 + w.f2 }`
  - `ring_a.rs`: `pub fn a() -> u32 { crate::ring_b::b_leaf() }` · `pub fn a_leaf() -> u32 { 1 }`; `ring_b.rs`: `pub fn b() -> u32 { crate::ring_a::a_leaf() }` · `pub fn b_leaf() -> u32 { 2 }`
  - `text.rs`: `use fakecord::fakechars::FixedStr;` · `pub fn label() -> FixedStr { FixedStr("x".into()) }` · `pub fn label_inline() -> String { fakecord::fakechars::FixedStr("y".into()).0 }` · ONE line: `pub const BANNER: &str = "🎵🎵"; pub fn after_emoji() -> u32 { 7 }` · `pub fn emoji_user() -> u32 { after_emoji() }`
  - `gated.rs`: `pub fn gated_fn() -> u32 { 3 }`
  - `tests.rs`: `#[test] fn t() { let _ = fakecord::Context; }`
  - `orphan.rs` (declared nowhere): `pub fn never_compiled() {}`

- [ ] **Step 1: Write the failing tests** in `src/config.rs` (unit) and `tests/fixture/main.rs` (`mod workspace;` → `tests/fixture/workspace.rs`):

```rust
#[test] fn defaults_without_file() { let c = Config::load(None).unwrap(); assert_eq!(c.headline_group, "discord"); assert_eq!(c.wide_struct_min_fields, 10); assert_eq!(c.near_clean_max_cuts, 2); assert_eq!(c.rust_analyzer.features, "all"); assert_eq!(c.hash(), "default"); }
#[test] fn glob_and_exact_group_match() { /* groups database=["sqlx","sqlx-*"] */ assert_eq!(c.group_of("sqlx-core"), Some("database")); assert_eq!(c.group_of("sqlx"), Some("database")); assert_eq!(c.group_of("tokio"), None); }
#[test] fn package_in_two_groups_is_an_error() { /* a=["x"], b=["x*"] */ assert!(Config::load(Some(&p)).unwrap_err().to_string().contains("x")); }
#[test] fn crate_ids_for_fixture() { let ws = Workspace::load(&fixture_ws(), &["apptesting".into()]).unwrap(); let ids: BTreeSet<_> = /* all crate_ids */; for id in ["appbin", "appbin[bin]", "it[test]", "appcore", "apptypes", "fakecord", "fakecord_macros", "fakechars", "apptesting"] { assert!(ids.contains(id), "{id}"); } assert!(ws.packages.iter().find(|p| p.name == "apptesting").unwrap().test_only); }
#[test] fn cracktunes_config_parses() { let c = Config::load(Some(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/topology.toml")))).unwrap(); assert_eq!(c.group_of("poise_macros"), Some("discord")); assert_eq!(c.group_of("sqlx-postgres"), Some("database")); }
```

- [ ] **Step 2: Run** `cargo test --manifest-path tools/topology/Cargo.toml` — expect compile failure / FAIL.
- [ ] **Step 3: Implement** config, model, workspace, CLI skeleton, build.rs, wrapper, fixture files. `cargo check --manifest-path tools/topology/tests/fixtures/ws/Cargo.toml --all-features --all-targets` must pass (fixture is real code).
- [ ] **Step 4: Run** tests, clippy, fmt — PASS.
- [ ] **Step 5: Verify** the root workspace is untouched: `git diff --stat origin/master -- Cargo.toml Cargo.lock` prints nothing; `cargo metadata --format-version 1 --no-deps | grep -c rust-topology` prints 0.
- [ ] **Step 6: Commit** `feat(topology): package skeleton, config, model, fixture workspace`.

---

### Task 2: rust-analyzer runner and SCIP reader

**Files:** Create `src/rust_analyzer.rs`, `src/symbol.rs`, `src/scip.rs`, `tests/common/mod.rs` (shared helpers, included by both test binaries), `tests/fixture/index.rs`, `tests/e2e.rs`. Mutation proof: `src/symbol.rs src/scip.rs`.

**Interfaces — Consumes:** `Config`, `Workspace` (Task 1). **Produces:**
- `rust_analyzer::run(ws: &Workspace, cfg: &Config, out: &Path) -> anyhow::Result<RaRun>`; `RaRun { index: PathBuf, version: String, error_lines: u32 }` (`version` = trimmed `rust-analyzer --version`). Missing binary → error containing `rustup component add rust-analyzer`.
- `symbol::Symbol::parse(s: &str) -> Option<Symbol>`; `Symbol { package: String, version: String, descriptors: String }`; `None` for `local …`. `Symbol::module_path(&self) -> Option<Vec<String>>` — `Some` only when `descriptors` is a pure namespace chain ending in `/` (`crate/` → `[]`, `music/queue/` → `["music","queue"]`).
- `scip::read(path: &Path) -> anyhow::Result<ScipIndex>`; `ScipIndex { documents: Vec<ScipDocument> }` sorted by path; `ScipDocument { path: String, occurrences: Vec<Occurrence> }` (encoding checked here: anything but UTF-8 → error); `Occurrence { start_line: u32, start_col: u32, end_line: u32, end_col: u32, symbol: String, definition: bool }` (0-based, UTF-8 bytes; 3-element ranges have `end_line = start_line`; `definition` = `symbol_roles & 1`).
- `tests/common/mod.rs`: `fixture_ws() -> PathBuf`; `fixture_config() -> Config`; `fixture_index() -> &'static ScipIndex` — runs rust-analyzer once per test binary, caching the `.scip` in `CARGO_TARGET_TMPDIR/fixture-<hash of every file under tests/fixtures/ws except target/>.scip`.

- [ ] **Step 1: Failing tests.** Unit (symbol.rs):

```rust
#[test] fn parses_external_method() { let s = Symbol::parse("rust-analyzer cargo serenity 0.12.5 gateway/client/impl#[Client]data().").unwrap(); assert_eq!((s.package.as_str(), s.version.as_str(), s.descriptors.as_str()), ("serenity", "0.12.5", "gateway/client/impl#[Client]data().")); assert_eq!(s.module_path(), None); }
#[test] fn local_is_none() { assert!(Symbol::parse("local 12").is_none()); }
#[test] fn module_paths() { assert_eq!(Symbol::parse("rust-analyzer cargo crack-core 0.36.2 crate/").unwrap().module_path(), Some(vec![])); assert_eq!(Symbol::parse("rust-analyzer cargo crack-core 0.36.2 music/queue/").unwrap().module_path(), Some(vec!["music".into(), "queue".into()])); }
```
Fixture (`tests/fixture/index.rs`): `index_covers_compiled_files` — documents include `appcore/src/gated.rs`, `appcore/src/tests.rs`, `appbin/src/main.rs`, `appbin/tests/it.rs` and NOT `appcore/src/orphan.rs`; `definitions_and_references_exist` — some occurrence in `appcore/src/rules.rs` has `definition` and a symbol ending `greet().`, and some non-definition occurrence has package `fakecord`. E2E (`tests/e2e.rs`): `missing_binary_is_explained` — `TOPOLOGY_RUST_ANALYZER=/nonexistent` → error text contains `rustup component add rust-analyzer`; `run_records_version_and_log` — fresh run into a tmp out dir: `version` starts with `rust-analyzer`, `<out>/rust-analyzer.log` exists, `<out>/ra-config.json` deserialises to features `"all"`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** Decode with the `scip` crate's `protobuf` types (`scip::types::Index`), never by hand.
- [ ] **Step 4: Run** `cargo test --manifest-path tools/topology/Cargo.toml` — PASS. Report the fixture index wall time.
- [ ] **Step 5: Mutation proof** (Global Constraints).
- [ ] **Step 6: Commit** `feat(topology): run rust-analyzer and read its SCIP index`.

---

### Task 3: Structure from syn — module trees, units, regions, hashes, orphans

**Files:** Create `src/structure/{mod.rs,tree.rs,items.rs,hash.rs}`, `tests/fixture/structure.rs`. Mutation proof: `src/structure/items.rs src/structure/hash.rs src/structure/tree.rs`.

**Interfaces — Consumes:** `Workspace`, `Config`, model types. **Produces:**
- `structure::read(ws: &Workspace, cfg: &Config) -> anyhow::Result<Structure>`.
- `Pos { line: u32, col: u32 }` (1-based line, 0-based char col, `Ord`), `Span { start: Pos, end: Pos }` with `contains(&self, p: Pos) -> bool` (end exclusive).
- `Structure { crates: Vec<CrateInfo>, units: Vec<UnitDraft>, fields: Vec<FieldDef>, uses: Vec<UseSpan>, files: BTreeMap<String, FileInfo>, orphans: Vec<Orphan>, warnings: Vec<Warning> }`.
- `UnitDraft { unit: Unit, ident: Option<Pos>, span: Span, body: Option<Span> }` — `body` = a fn's block, a const/static's expression, a macro call's tokens; `None` for types/traits/impls/modules (everything is signature). `unit.parent` = the impl or trait unit of a method; `unit.module` = enclosing module.
- `FieldDef { owner: UnitId, name: String, file: String, ident: Pos }` — named fields only.
- `UseSpan { module: UnitId, file: String, span: Span, public: bool }` — one per `use` item.
- `FileInfo { krate: String, module: UnitId, text: String }`.

Rules (the tests below pin them):
- Walk each target's module tree from its `root_file`, following **every** `mod x;` regardless of `cfg`, honouring `#[path = "…"]`, `x.rs` / `x/mod.rs`. A file reached by two targets (lib and bin both `mod` it) belongs to the first in target order lib, bin, others.
- Units: modules (file and inline), and items at module, impl and trait level (see the vocabulary for ids). Module units span their whole file (file modules) or their braces (inline).
- `test` = under `#[cfg(test)]` (on the item or any ancestor), or carries an attribute whose last path segment is `test`, or in a test/example/bench target, or in a `test_only` package.
- `features` = every `feature = "…"` named in `cfg`/`cfg_attr` attributes on the item and its ancestors.
- `hash` = sha256 hex of the item's token stream with `#[doc]` attributes removed (whitespace and comments therefore ignored).
- `own_lines` = `lines` minus the lines covered by child units.
- Orphans: `.rs` files under each package dir (skipping any `target/` dir and the package's `build.rs`) reached by no target tree; `CrateInfo.lines` = lines of the files the crate's tree reaches.
- A file that fails to parse → `Warning { kind: UnparsedFile }`, skipped.

- [ ] **Step 1: Failing tests.** Unit tests in `items.rs` parse source strings with a stub crate `c`:

```rust
#[test] fn ids_for_impls_and_methods() { let s = parse("struct Bot; trait T { fn f(&self); fn d(&self) {} } impl Bot { fn ping(&self) {} } impl T for Bot { fn f(&self) {} } impl Bot { fn pong(&self) {} }"); for id in ["c::Bot", "c::T", "c::T::f", "c::T::d", "c::<impl Bot>", "c::Bot::ping", "c::<impl T for Bot>", "c::<Bot as T>::f", "c::<impl Bot>#2", "c::Bot::pong"] { assert!(s.has(id), "{id}"); } assert_eq!(s.get("c::Bot::ping").unit.parent.as_ref().unwrap().0, "c::<impl Bot>"); }
#[test] fn cfg_alternatives_get_suffixes() { let s = parse("#[cfg(feature = \"a\")] fn f() {} #[cfg(not(feature = \"a\"))] fn f() {}"); assert!(s.has("c::f") && s.has("c::f#2")); assert!(s.get("c::f").unit.features.contains("a")); }
#[test] fn fn_regions_split_signature_and_body() { /* "fn f(x: u32) -> u32 {\n    x\n}" */ let d = s.get("c::f"); assert_eq!(d.body.unwrap().start.line, 1); assert!(d.span.start <= d.body.unwrap().start); }
#[test] fn hash_ignores_docs_and_whitespace() { assert_eq!(hash_of("/// a\nfn f() { 1 }"), hash_of("fn f()   {\n 1 }")); assert_ne!(hash_of("fn f() { 1 }"), hash_of("fn f() { 2 }")); }
#[test] fn nested_fn_is_part_of_body() { let s = parse("fn outer() { fn inner() {} }"); assert!(!s.has("c::inner") && !s.has("c::outer::inner")); }
```
Fixture tests (`tests/fixture/structure.rs`): `fixture_units_present` (ids `appcore::rules::greet`, `appcore::handlers::<impl EventHandler for Handler>`, `appcore::handlers::<Handler as EventHandler>::on_event`, `appcore::handlers::Bot::ping`, `appcore::text::after_emoji`, `appcore::text::BANNER`, `appbin[bin]::run`, `appbin::run`, `it[test]::it_runs`), `gated_unit_has_feature` (`appcore::gated::gated_fn` features == `{"extra"}`), `test_units_marked` (`appcore::tests::t`, `it[test]::it_runs`, `apptesting::helper` all `test`; `appcore::rules::greet` not), `orphan_found` (exactly one orphan, `appcore/src/orphan.rs`, lines 1), `wide_fields_recorded` (10 `FieldDef`s owned by `appcore::state::Wide`), `use_span_recorded` (a non-public `UseSpan` in `appcore/src/text.rs`; a public one in `apptypes/src/lib.rs`).
- [ ] **Step 2: Run** — FAIL. **Step 3: Implement.** **Step 4: Run** — PASS.
- [ ] **Step 5: Mutation proof.** **Step 6: Commit** `feat(topology): read module trees and units with syn`.

---

### Task 4: Placement — map definitions, place occurrences, aggregate the graph

**Files:** Create `src/placement.rs`, `src/graph.rs`, `tests/fixture/placement.rs`. Mutation proof: `src/placement.rs src/graph.rs`.

**Interfaces — Consumes:** `Structure`, `ScipIndex`, `Symbol`, `Workspace`. **Produces:**
- `placement::place(structure: &Structure, index: &ScipIndex, ws: &Workspace) -> Placed`.
- `Placed { refs: Vec<RawRef>, reexports: Vec<Reexport>, imports: BTreeMap<UnitId, BTreeSet<String>>, warnings: Vec<Warning>, stats: PlacementStats }`; `RawRef { from: UnitId, target: RefTarget, position: Position, loc: Loc }`; `RefTarget { Unit(UnitId), Module(UnitId), Field { owner: UnitId, field: String }, External { package: String, symbol: String } }`; `PlacementStats { definitions_exact: u32, definitions_contained: u32, definitions_unmapped: u32, references: u32, references_unmapped: u32 }`.
- `graph::aggregate(placed: &Placed) -> Graph`; `Graph { edges: Vec<Edge>, external: Vec<ExternalRef>, field_uses: Vec<(UnitId, String, UnitId)> /* owner, field, from */ }`. A `Field` ref also yields an edge `from → owner`; `Module` refs yield edges too (cycles use them, taint ignores them). Self-edges are dropped. Counts aggregate per (from, to, position); `first` = smallest `Loc`.

Rules:
- Convert each occurrence to `Pos` with the file's text (UTF-8 byte col → char col; 0-based line → 1-based).
- **Definitions** (workspace packages only): exact match of the occurrence start against a unit `ident` or a `FieldDef.ident` → that unit/field; else the innermost unit whose span contains it (`definitions_contained`); else `UnmappedDefinition` warning. Module symbols map by `Symbol::module_path` to the module id in the defining file's crate. One symbol may map to several units (lib and bin of one package share symbols): keep all candidates.
- **References:** `from` = innermost unit containing the occurrence; `position` = `Body` if inside that unit's `body`, else `Signature`. Inside a `UseSpan`: public → `Reexport { module, target: symbol descriptors or target unit id }`, private → `imports[module]` gets the symbol's package (external packages only); neither becomes a `RawRef`. Workspace symbol → when the source text of the path segment just before it (same line, separated by `::`) is the package's lib crate name, the lib's candidate; else the candidate in the referencing unit's crate; else the lib target's; else `UnmappedReference`. (A bin and a lib of one package share SCIP symbols, so only the text tells `appbin::run()` from `run()` inside the bin.) An impl's header refs are its "implements" edges: Signature edges (or External refs) from the impl unit to its trait and self type. External package → `RefTarget::External`. Occurrences in files outside the structure → `OccurrenceOutsideTree`; structure files with no SCIP document → `UnindexedFile`.

- [ ] **Step 1: Failing fixture tests** (`tests/fixture/placement.rs`, helper `fixture_placed() -> &'static (Structure, Placed, Graph)` added to `tests/common`):

```rust
#[test] fn signature_edge_to_workspace_type() { assert!(has_edge("appcore::rules::pick_next", "appcore::rules::Track", Position::Signature)); }
#[test] fn body_edge_to_function() { assert!(has_edge("appcore::rules::wrapper", "appcore::rules::greet", Position::Body)); }
#[test] fn attribute_is_signature_evidence() { assert!(has_external("appcore::handlers::cmd", "fakecord", Position::Signature)); }
#[test] fn reexport_resolves_to_defining_crate() { assert!(has_external("appcore::rules::in_guild", "fakecord", Position::Signature)); }
#[test] fn plain_use_is_import_not_ref() { assert!(!has_external("appcore::text::label", "fakecord", Position::Signature) && !has_external("appcore::text::label", "fakecord", Position::Body)); assert!(placed.imports[&id("appcore::text")].contains("fakecord")); }
#[test] fn path_segment_counts_in_body() { assert!(has_external("appcore::text::label_inline", "fakecord", Position::Body)); }
#[test] fn emoji_line_maps_to_after_emoji() { assert!(has_edge("appcore::text::emoji_user", "appcore::text::after_emoji", Position::Body)); assert!(!has_edge("appcore::text::emoji_user", "appcore::text::BANNER", Position::Body)); }
#[test] fn bin_reference_prefers_own_crate() { assert!(has_edge("appbin[bin]::main", "appbin[bin]::run", Position::Body)); assert!(has_edge("appbin[bin]::main", "appbin::run", Position::Body)); }
#[test] fn field_use_recorded() { assert!(graph.field_uses.contains(&(id("appcore::state::Wide"), "f3".into(), id("appcore::handlers::third")))); }
#[test] fn pub_use_is_reexport() { assert!(placed.reexports.iter().any(|r| r.module == id("apptypes") && r.target.contains("GuildId"))); }
#[test] fn fixture_has_no_unmapped_references() { assert_eq!(placed.stats.references_unmapped, 0, "{:?}", placed.warnings); }
```
- [ ] **Step 2–4:** run (FAIL), implement, run (PASS). **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): place SCIP occurrences into units and build the graph`.

---

### Task 5: Taint per group, cuts, module lines, seams

**Files:** Create `src/taint.rs`, `tests/fixture/taint.rs`. Mutation proof: `src/taint.rs`.

**Interfaces — Consumes:** `Structure`, `Placed`, `Graph`, `Config`. **Produces:**
- `taint::compute(structure: &Structure, placed: &Placed, graph: &Graph, cfg: &Config) -> Taint`; `Taint { by_group: BTreeMap<String, BTreeMap<UnitId, UnitTaint>>, module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>> }` — every non-module unit appears in every group.
- `taint::seams(group_taint: &BTreeMap<UnitId, UnitTaint>, units: &[Unit], max_cuts: u32) -> Vec<Seam>` (works on `Taint.by_group[g]` and on `Topology.taint[g]` alike); `Seam { unit: UnitId, state: State, cuts: u32, lines: u32 }` — non-test, non-module, state ≠ Clean, `cuts ≤ max_cuts`, sorted by cuts asc, lines desc, id asc.

Algorithm, per group (ignore edges to `Module` targets throughout):
1. `direct_sig(u)` / `direct_body(u)`: `u` has an `External` ref in that position to a package in the group.
2. Tainted types = fixpoint over Struct/Enum/Union/TypeAlias/Trait units: tainted if `direct_sig` or a Signature edge to a tainted type.
3. `sig(u)` = `direct_sig(u)` or a Signature edge to a tainted type, or (for a method) its parent impl/trait is `sig`.
4. State: `sig` → Signature; else `direct_body` → Body; else reaches (any edge, transitively) a unit that is `sig` or `direct_body` → Inherited; else Clean.
5. `direct` = the group's External refs aggregated per (package, symbol, position) with count and first `Loc`; `via` = directly-used workspace units whose state ≠ Clean (plus the parent impl when inherited from it), sorted.
6. `cuts` = distinct `(file, line)` of the unit's direct group refs + `via.len()`; 0 when Clean.
7. `module_lines[group][module]` = sum of non-test units' `own_lines` by state.

- [ ] **Step 1: Failing tests.** Fixture (`tests/fixture/taint.rs`), group `discord`:

```rust
#[test] fn fixture_states() { for (u, s) in [("appcore::rules::pick_next", Clean), ("appcore::rules::score", Clean), ("appcore::rules::plain", Clean), ("appcore::rules::Track", Clean), ("appcore::rules::in_guild", Signature), ("appcore::rules::uses_alias", Signature), ("appcore::rules::greet", Body), ("appcore::rules::wrapper", Inherited), ("appcore::handlers::<impl EventHandler for Handler>", Signature), ("appcore::handlers::<Handler as EventHandler>::on_event", Signature), ("appcore::handlers::Bot::ping", Clean), ("appcore::handlers::cmd", Signature), ("appcore::text::label", Clean), ("appcore::text::label_inline", Body), ("apptypes::SharedCtx", Signature), ("apptypes::Plain", Clean)] { assert_eq!(state(u), s, "{u}"); } }
#[test] fn alias_taint_is_via() { let t = taint("appcore::rules::uses_alias"); assert!(t.direct.is_empty()); assert_eq!(t.via, vec![id("apptypes::SharedCtx")]); assert_eq!(t.cuts, 1); }
#[test] fn cuts_count_lines() { assert_eq!(taint("appcore::rules::greet").cuts, 2); assert_eq!(taint("appcore::handlers::cmd").cuts, 1); assert_eq!(taint("appcore::rules::wrapper").cuts, 1); }
#[test] fn seams_exclude_tests_and_sort_by_cuts() { let s = seams("discord", 2); assert!(s.iter().all(|x| x.unit != id("appcore::tests::t"))); assert!(s.windows(2).all(|w| w[0].cuts <= w[1].cuts)); assert!(s.iter().any(|x| x.unit == id("appcore::rules::wrapper"))); }
#[test] fn test_unit_still_has_a_state() { assert_eq!(state("appcore::tests::t"), Body); }
```
Unit tests on hand-built `Structure`/`Placed` values: `fixpoint_through_two_types` (A's field is B, B's field is external-group → A tainted), `inherited_is_transitive` (u→v→w, w body-tainted → u Inherited, `via == [v]`), `module_edges_do_not_taint`, `module_lines_sum_own_lines`.
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): per-group taint with evidence, cuts and seams`.

---

### Task 6: Analyses — cycles, wide structs, groups per module

**Files:** Create `src/analysis/{mod.rs,cycles.rs,wide.rs,groups.rs}`, `tests/fixture/analysis.rs`. Mutation proof: the three analysis files.

**Interfaces — Produces:**
- `analysis::cycles::compute(structure: &Structure, graph: &Graph) -> Cycles` — module graph per crate from non-test units' edges (unit → its `module`; target unit → its module, or the module itself for `Module` targets; same-crate, different modules). `by_module`: SCCs larger than one module at full depth; `by_top_module`: the same after collapsing each module to its first path segment below the crate root (items directly in the root stay in the root). Each `Cycle` lists its modules sorted and its internal `ModuleEdge`s; cycles sorted by (krate, first module).
- `analysis::wide::compute(structure: &Structure, graph: &Graph, min_fields: usize) -> Vec<WideStruct>` — structs with ≥ `min_fields` named fields; `uses[field][module of from][from]`.
- `analysis::groups::module_groups(structure: &Structure, placed: &Placed, cfg: &Config) -> BTreeMap<UnitId, BTreeMap<String, u32>>` — per module, the count of its non-test units' External refs per group, plus each group named in `placed.imports[module]` counted once.

- [ ] **Step 1: Failing tests:** `ring_cycle_found` (`by_module` contains a cycle with exactly `appcore::ring_a` and `appcore::ring_b`; `by_top_module` too), `no_cycle_between_rules_and_handlers`, `wide_struct_matrix` (one `WideStruct` `appcore::state::Wide` with 10 fields; `uses["f3"]` has module `appcore::handlers` → `{appcore::handlers::third}`; `uses["f1"]` has `appcore::state`), `handlers_reach_discord` (`module_groups[appcore::handlers]["discord"] > 0`; `appcore::state` has no `discord` key). Unit tests: `scc_of_three_modules`, `top_module_collapse_merges_children` (`a::x → b`, `b → a::y` ⇒ top-level cycle {a, b} while full depth has none if `a::x ≠ a::y`).
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): module cycles, wide-struct field matrices, groups per module`.

---

### Task 7: The `extract` command, `topology.json`, `digest.json`, summary

**Files:** Create `src/extract.rs`, `src/digest.rs`, `src/summary.rs`; modify `src/main.rs`; tests `tests/fixture/extract.rs`, more in `tests/e2e.rs`. Mutation proof: `src/digest.rs src/summary.rs`.

**Interfaces — Produces:**
- `extract::build(ws: &Workspace, cfg: &Config, index: &ScipIndex, generator: Generator) -> anyhow::Result<Topology>` (pure apart from reading source files; sorts everything; merges warnings from structure and placement).
- `extract::run(workspace: &Path, config: Option<&Path>, out: &Path, scip: Option<&Path>) -> anyhow::Result<Topology>` — runs rust-analyzer unless `scip` is given (then `rust_analyzer = "external index"`, `ra_error_lines = 0`), writes `<out>/topology.json` and `<out>/digest.json` (pretty JSON + trailing newline).
- `digest::Digest { generator: Generator, units: BTreeMap<UnitId, DigestUnit>, edges: BTreeMap<String, u32>, module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>>, cycles_by_module: Vec<Vec<UnitId>>, cycles_by_top_module: Vec<Vec<UnitId>>, wide_structs: BTreeMap<UnitId, BTreeMap<String, BTreeSet<UnitId>>>, orphans: BTreeMap<String, u32>, warnings: BTreeMap<String, u32> }`; `DigestUnit { kind: UnitKind, hash: String, lines: u32, test: bool, states: BTreeMap<String, State> }`; `Digest::from_topology(t: &Topology) -> Digest`. Edge keys: `"<from> -> <to> [signature|body]"` for unit edges, `"<from> -> ext:<package> [..]"` summed for external refs. No line numbers anywhere.
- `summary::render(t: &Topology, cfg: &Config) -> String` — crates with lines; for each group (headline first) non-test lines and units per state for the crate with the most lines; the top 15 seams of the headline group (unit, state, cuts, lines); cycle counts at both depths; a line `orphans: <files> files, <lines> lines`; warnings per kind; rust-analyzer error-line count.

- [ ] **Step 1: Failing tests:** `topology_roundtrips` (serialise → deserialise equal), `digest_ignores_line_shifts` (unit: shift every unit's `start_line`/`end_line` and every `Loc.line` by 5 → identical `Digest`), `digest_has_no_loc` (serialised digest text contains no `"line"` key). E2E: `extract_twice_is_byte_identical` (two `extract::run` into two out dirs, same `--scip` index → identical `topology.json` and `digest.json` bytes), `extract_from_other_cwd_has_relative_paths` (spawn the built binary with cwd = a tmp dir and `--workspace <fixture>`; every `Unit.file` starts with a package dir like `appcore/`, none is absolute), `summary_mentions_headline` (summary contains `discord`, `appcore::rules::wrapper`, `orphans: 1 files, 1 lines`).
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): extract command writes topology.json, digest.json and a summary`.

---

### Task 8: The `diff` command

**Files:** Create `src/diff.rs`; modify `src/main.rs`; tests in `src/diff.rs` and `tests/e2e.rs`. Mutation proof: `src/diff.rs`.

**Interfaces — Produces:**
- `diff::load(path: &Path) -> anyhow::Result<Digest>` — a `topology.json` (converted with `Digest::from_topology`) or a `digest.json`, tried as those two concrete types in that order.
- `diff::compare(a: &Digest, b: &Digest) -> DiffReport`; `DiffReport { notes: Vec<String>, added: Vec<UnitId>, removed: Vec<UnitId>, changed: Vec<UnitId>, moved: Vec<Move>, transitions: BTreeMap<String, Vec<Transition>>, module_lines: BTreeMap<String, Vec<ModuleLinesChange>>, cycles_added: Vec<Vec<UnitId>>, cycles_removed: Vec<Vec<UnitId>>, wide_changes: Vec<String>, orphans_added: Vec<String>, orphans_removed: Vec<String>, warnings: (BTreeMap<String,u32>, BTreeMap<String,u32>) }`; `Move { from: UnitId, to: UnitId, changed: bool }`; `Transition { unit: UnitId, before: Option<State>, after: Option<State> }` (moved units compare old id to new id); `ModuleLinesChange { module: UnitId, before: StateLines, after: StateLines }`.
- `diff::render(r: &DiffReport) -> String` (notes first); `--json` prints `serde_json::to_string_pretty(&report)`. Always exit 0.

Move matching: name = last segment of the id (for methods, the last two). Among removed × added with equal kind: equal name and hash → `moved`, `changed: false`; then equal name, different hash, exactly one candidate on each side → `changed: true`; anything else stays added/removed.

- [ ] **Step 1: Failing tests** (unit, on hand-built digests): `identical_digests_empty_report`, `added_removed_changed`, `move_same_hash`, `move_changed_unique`, `ambiguous_names_not_matched` (two removed `x::new`, one added `y::new` → no move), `transition_to_clean_reported` (unit Signature → Clean), `generator_mismatch_is_noted_first` (different `rust_analyzer` → `notes[0]` mentions rust-analyzer and render starts with it). E2E `moved_function_reads_as_move`: copy the fixture to `CARGO_TARGET_TMPDIR/moved-ws` (skip `target/`), move `pub fn score(t: &Track) -> u32 { t.id * 2 }` from `rules.rs` into `text.rs` (add `use crate::rules::Track;` there; change `cmd`'s call to `crate::text::score`), extract both, compare → `moved` contains `appcore::rules::score → appcore::text::score` with `changed: false`, and `changed` contains `appcore::handlers::cmd`.
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): diff two maps, recognising moves`.

---

### Task 9: The `timings` command

**Files:** Create `src/timings.rs`, `tests/fixtures/cargo-timing-sample.html` (captured from a real `cargo build --timings` of the fixture, trimmed to the `UNIT_DATA` line plus a few lines around it); modify `src/main.rs`; tests in `src/timings.rs` and `tests/e2e.rs`. Mutation proof: `src/timings.rs` (parsing and statistics only).

**Interfaces — Produces:**
- `timings::parse_unit_data(html: &str) -> anyhow::Result<Vec<UnitTime>>` — the JSON array after `const UNIT_DATA = ` up to its terminating `];`; `UnitTime { name: String, version: String, mode: String, target: String, start: f64, duration: f64 }` (unknown fields ignored).
- `timings::median(xs: &[f64]) -> f64`.
- `timings::run(ws: &Workspace, cfg: &Config, out: &Path, repeats: u32, target_dir: Option<&Path>) -> anyhow::Result<Timings>` — writes `<out>/timings.json`.
- `Timings { rustc: String, profile: String, jobs: u32, host: String, repeats: u32, crates: Vec<CrateTiming>, rebuild: Option<RebuildTiming> }`, `CrateTiming { package: String, target: String, median_secs: f64, samples: Vec<f64> }`, `RebuildTiming { package: String, through: String, median_secs: f64, samples: Vec<f64> }`.

Method: target dir = flag, else config, else `$HOME/.cache/crack-topology/timings-target`; env `CARGO_TARGET_DIR` = it, `CARGO_INCREMENTAL=0`. One warm build `cargo build --workspace --all-features --profile <profile>`. Each repeat: touch every workspace target's `root_file`, `cargo build --workspace --all-features --profile <p> --timings`, read `<target>/cargo-timings/cargo-timing.html`, record each workspace target's summed `duration` (build-script runs excluded); then, when `rebuild_package` and `through_package` are set, touch only the rebuild package's lib root and time `cargo build -p <through> --all-features --profile <p>` wall-clock. Never cleans anything.

- [ ] **Step 1: Failing tests:** `parses_sample` (sample yields ≥ 1 `UnitTime` with name `appcore`, `duration > 0`), `median_odd_even` (`[3,1,2]` → 2; `[1,2,3,4]` → 2.5), `missing_unit_data_is_an_error`. E2E `timings_on_fixture` (repeats 1, target dir in `CARGO_TARGET_TMPDIR`, fixture config with `rebuild_package = "appcore"`, `through_package = "appbin"`): every workspace lib target has a `CrateTiming`; `rebuild` is `Some`; `timings.json` deserialises.
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): timings per crate and for a core rebuild`.

---

### Task 10: The `page` command

**Files:** Create `src/page/{mod.rs,model.rs}`, `src/page/assets/{page.html,page.css,page.js}`; modify `src/main.rs`; tests `tests/fixture/page.rs`. Mutation proof: `src/page/model.rs`.

**Interfaces — Produces:**
- `page::model::build(t: &Topology, timings: Option<&Timings>, cfg: &Config) -> PageModel` — every number the page shows, computed here: overview (crates, lines, headline-group split per crate, timings if any), module rows per group (lines per state, units per state, groups reached, imports), unit details (state per group, evidence, uses, used-by — ids and `Loc`s only, no source text), seams per group (`taint::seams` with `cfg.near_clean_max_cuts`), test units per group whose state is not Clean (the tests a split must move or rewrite), cycles, wide-struct matrices, orphans, warnings grouped by kind with counts and the first 50 of each. Typed structs, `Serialize`.
- `page::render(m: &PageModel) -> String` — one HTML file: `page.html` with `page.css` and `page.js` inlined and the model embedded as `<script type="application/json" id="data">` with every `</` written `<\/`. The JS only renders (tabs, sortable/filterable tables, unit detail panel, links between them); it computes nothing.
- `page` command writes `<out>/page/topology.html` from `--in` (default `<out>/topology.json`) and `--timings` (default `<out>/timings.json` when present).

Page contract (it is published as a claude.ai Artifact): `<title>` = `<config.name> topology` (e.g. "cracktunes topology"); colours as CSS custom properties on `:root`, redefined under `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { … } }` and again under `:root[data-theme="dark"]`; explicit `body` background; usable at 400 px wide with a 16 px side gutter and no horizontal page scroll (wide tables scroll inside their own container); no external URLs at all (no fonts, scripts or images fetched).

- [ ] **Step 1: Failing tests:** `model_seams_sorted` (fixture model's discord seams sorted by cuts and include `appcore::rules::wrapper`), `model_has_no_source_text` (serialised model does not contain `t.id * 2`), `html_embeds_model` (extract the `#data` script text, replace `<\/` with `</`, deserialise into `PageModel`, equal to the input), `html_contract` (contains `<title>fixture topology</title>`, `prefers-color-scheme: dark`, `:root[data-theme="dark"]`; contains neither `http://` nor `https://`; contains no literal `</script` inside the data block).
- [ ] **Step 2–4.** **Step 5:** mutation proof. **Step 6: Commit** `feat(topology): self-contained review page`.

---

### Task 11: CI, docs, and the real run on cracktunes

**Files:** Create `.github/workflows/topology.yml`; finish `tools/topology/README.md`; no other repo files.

- [ ] **Step 1: CI workflow** `topology.yml`: triggers `pull_request` and `push` to `master`, both with `paths: ["tools/topology/**", ".github/workflows/topology.yml"]`; `permissions: contents: read`; one job on `ubuntu-latest`: checkout, `rustup toolchain install stable --profile minimal --component rust-analyzer,rust-src,clippy,rustfmt`, `Swatinem/rust-cache@v2` with `workspaces: tools/topology` and `shared-key: topology`, then `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, all with `--manifest-path tools/topology/Cargo.toml`. Pin actions the way the repo's other workflows do.
- [ ] **Step 2: README** (`tools/topology/README.md`): what the tool answers (the spec's questions), the commands with the wrapper, the pipeline (`extract --summary` → `page` → `timings` optional), comparing before/after a split with `diff`, the meaning of each taint state and of `cuts`, the two path rules, the warnings kinds, and the traps (default features hide gated code; ~3,000 harmless rust-analyzer ERROR lines; a tool or rust-analyzer change makes digests incomparable).
- [ ] **Step 3: Mutants check stays green.** From the repo root: `git diff origin/master -- '*.rs' > $HOME/.cache/cargo-mutants/topology.diff` then `scripts/mutants.sh --list --in-diff $HOME/.cache/cargo-mutants/topology.diff | wc -l` → `0`, exit 0. If not 0, stop and report (the required check would run shards).
- [ ] **Step 4: Real run.** `scripts/topology.sh extract --summary` on this branch (its crates are master's). Gates, each reported with numbers:
  - exit 0; every crack-core file a `mod` reaches is a unit's file; crack-core's orphans = 52 files / 3,601 lines (the spike's count; other packages' orphans listed);
  - `references_unmapped` < 1% of workspace references; warnings listed per kind; anything above 1% is a bug to fix in this task;
  - hand-check 10 units against the source — 5 the map calls Clean in crack-core, 5 it calls Signature/Body — and record each verdict in the report; a wrong one is a bug to fix in this task;
  - `extract` twice → identical `digest.json`.
- [ ] **Step 5:** `scripts/topology.sh page`, open-free check: the HTML is under 16 MB. `scripts/topology.sh timings --repeats 1` and report its numbers and wall time.
- [ ] **Step 6: Commit** `ci(topology): build and test the tool; README` (workflow + README + any fixes).
