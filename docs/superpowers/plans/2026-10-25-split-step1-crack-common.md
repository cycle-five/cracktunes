# Split step 1: crack-common Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `CrackedError`, the user-facing strings, `CrackedMessage` and its neutral rendering out of crack-core into a new crate, `crack-common`, with no change to what the bot sends.

**Architecture:** First cut every reference from those files up into crack-core, while everything still lives in crack-core (Tasks 1–2). Then `git mv` the files into `crack-common` and leave `pub use` shims, so every `crate::messaging::…` and `crate::errors::…` path in crack-core keeps working (Task 3).

**Tech Stack:** Rust 2021 Cargo workspace, serenity/poise/songbird, `crack-model`'s neutral `Rendered`/`Card`/`Row`, nextest.

**Spec:** `docs/superpowers/specs/2026-10-25-split-crack-core-design.md` (step 1, rules 1–9).

## Global Constraints

- **No user-visible change.**
  - Every message's Discord payload stays byte-identical: content, embeds, components and allowed mentions.
  - `CrackedMessage`'s `Display` and `CrackedError`'s `Display` are unchanged byte for byte.
  - Existing render and pin tests keep their assertions. Only their construction or location may change, and a changed assertion needs a stated reason in the report.
- **No storage, wire, migration or SQL change.**
- **crack-common never depends on poise or crack-core** (spec rules 1 and 9). It may depend on crack-model, crack-types, serenity, songbird and the other external crates the moved files already use (spec rule 6).
- **Dependency versions.** Copy each new dependency line exactly from `crack-core/Cargo.toml`, or use `{ workspace = true }` where crack-core does. `Cargo.lock` gains the `crack-common` package and no second version of any crate.
- **Paths keep working.**
  - crack-core re-exports each moved module at its old path.
  - Call sites elsewhere in crack-core, crack-web and crack-cli change only where this plan says so.
- **Typed serde only.** Never `serde_json::json!`, and never index a `serde_json::Value`.
- **No local mutants runs** (owner, 2026-10-25: mutation testing is a tool, not a gate).
- **Memory cap on every local cargo, nextest or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. Write paths in full, never through a shell variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap, from the worktree root `/home/lothrop/projects/cracktunes-split`):
  - suite: `cargo nextest run --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - doctests: `cargo test --doc --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - lint:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
- **Waiting:** wait on long jobs in the foreground, with a blocking loop under the 10-minute Bash timeout. Never end the turn to wait for a notification.
- **Commits:**
  - never `git add -A`; add files by name, and use `git mv` for moves;
  - run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.

## Calls this plan makes against the spec

- **The trait is `Payload`** (`messaging::render::Payload`). The three feature variants keep their names and positions, and change only their payload type to `Box<dyn Payload>`. Construction sites (`CrackedMessage::Gp(Box::new(card))` and the `From` impls) compile unchanged.
- **`/gp start`'s notice leaves `CrackedMessage`** as a new `GpCard::Started` variant. That removes `GpStarted` and its `GpClip`/`GpReveal` fields from the shared enum, as the spec says.
  - Removing a variant shifts the discriminants of those after it. `discriminant()` is used only by `PartialEq` and in-memory tests, and nothing stores it.
- **The now-playing card carries its buttons as neutral rows.** `controls: Option<Controls>` becomes `rows: Vec<crack_model::Row>`, built where the controls are set today. `Controls` and the custom-id encoding stay in crack-core's `buttons.rs`.
- **The pure helpers the neutral files borrow move down** and are re-exported where they were: `duration_to_string`, `build_footer_info`, `requesting_user_to_string` and `finished_card`.
- **`TrackLabel::from_ref(&TrackRef)` becomes `TrackRef::label(&self) -> TrackLabel`** in `music/audit.rs`, so `format.rs` stops naming a music type.
- **track_failed splits.** The text (`FailReason`, `Failure`, `entry`, `render_text`) goes to a new `messaging/failure.rs`, which moves to crack-common. Delivery (`FailureNotice`, `report`, `failures`, `notify`, the `Data` slot) stays in `track_failed.rs`, which re-exports the moved names.
- **Tests that convert to Discord JSON stay in crack-core,** with unchanged assertions. These are the render.rs and cards.rs pins that use `messaging::discord` or `interface::create_nav_btns`. Tests that need only `Rendered` or text move with their code.
- **crack-types' poise use goes behind a feature.**
  - crack-types depends on poise only for `reply_handle.rs` (`ReplyHandleTrait`, `ReplyHandleWrapper`, `MessageOrReplyHandle`, `Container`), and only crack-core uses those.
  - crack-common needs crack-types for `TrackResolveError`, `NotAYtdlUrl` and `discord::IdError`.
  - So poise becomes optional in crack-types, behind a `poise` feature that gates `reply_handle`. crack-core turns it on.
  - Without this, crack-common would pull in poise through crack-types, and the spec's "no poise" rule would hold only in name.
- **v0.39.0** is a minor bump, following the transpose arc. The controller does it inline after Task 3, as Ruling P14 did for v0.38.0.

## Review Focus

1. **`/gp start`'s notice:** its text, its blue colour and its embed must not change when it moves into `GpCard`. Pinned in Task 1, Step 1.
2. **The now-playing status message's buttons:**
   - Pause/Resume, Skip, Repeat and Shuffle must still appear on the status message, with the same custom ids and styles.
   - They must not appear on replies.
   - A guild whose ref is not a Discord snowflake still gets no buttons.
   
   Pinned in Task 2, Step 1.
3. **The track-failed notice:** one track, several tracks, "+N more", and a blank title (`Some("")`) all keep their exact text after the split. Pinned in Task 2, Step 1, by the existing track_failed tests moving with `render_text`.
4. **Builds with `crack-osint` on:** the osint arms of `CrackedMessage` still compile and render under `--features crack-osint`. Checked in Task 3, Step 5, by the seam check `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`.
5. **Error replies:** every `CrackedError`'s `Display`, its colour (red) and the `From` conversions stay the same. Covered by the existing errors.rs and message.rs tests, which move with the code unchanged (Task 3).

---

### Task 1: Feature payloads render themselves (`Payload`), and `/gp start` becomes a `GpCard`

**Files:**
- Modify: `crack-core/src/messaging/render.rs`: add `Payload`; dispatch the three feature arms through it.
- Modify: `crack-core/src/messaging/message.rs`:
  - three variant payload types;
  - remove `GpStarted` (variant, `Display` arm and its 4 tests at about lines 822–867);
  - its `crate::commands::music::gp::{GpClip, GpReveal}` uses go with it.
- Modify: `crack-core/src/commands/music/gp/ui.rs`:
  - `GpCard::Started`, plus its `Display` text and render;
  - `impl Payload for GpCard`;
  - the moved `GpStarted` tests.
- Modify: `crack-core/src/commands/music/gp/commands.rs:474`: send `GpCard::Started` (via `CrackedMessage::Gp`) instead of `CrackedMessage::GpStarted`.
- Modify: `crack-core/src/rooms/card.rs`: `impl Payload for RoomCard`.
- Modify: `crack-core/src/modlog/event.rs`: `impl Payload for ModLogEvent`.

**Interfaces:**
- Produces:
  - `pub trait Payload: std::fmt::Debug + std::fmt::Display + Send + Sync { fn render(&self, cx: &RenderCx) -> Rendered; }` in `crate::messaging::render`.
  - `CrackedMessage::Gp(Box<dyn Payload>)`, `CrackedMessage::Room(Box<dyn Payload>)` and `CrackedMessage::ModLog(Box<dyn Payload>)`, same positions and names.
  - `GpCard::Started { category: String, rounds: usize, timer_secs: u64, clip: Option<GpClip>, reveal: GpReveal, round_results: bool, cleared_queue: bool }`.
- Consumes: the existing `gp::render_card`, `rooms::card::render_card` and `modlog::event::render`. Each `Payload::render` calls its own.

- [ ] **Step 1: Pin `/gp start`'s output as it is today.** Add a test `gp_started_renders_as_before` to `messaging/message.rs`'s test module.
  - Use the first existing `GpStarted` test's field values.
  - Assert `render(&msg, &RenderCx::default())` equals a literal `Rendered::card(Card { description: Some(<the exact text>), colour: Some(colour::BLUE), ..Card::default() })`.
  - Take the text from that test's existing `to_string()` assertion.
  - Add a second case with `clip: Some(GpClip { start: Duration::from_secs(30), length: Duration::from_secs(20) })` and `reveal: GpReveal::Song`. Take its literal text from one run of the current code.
- [ ] **Step 2: Run it.** `cargo nextest run -p crack-core gp_started_renders_as_before` (under the cap). Expected: PASS on today's code. This is a characterization pin.
- [ ] **Step 3: Implement `Payload` and the three impls.** Change the three variants' payload type, and replace the three `render()` arms with one arm: `CrackedMessage::Gp(p) | CrackedMessage::Room(p) | CrackedMessage::ModLog(p) => p.render(cx)`. `CrackedMessage`'s `Display` arms for them stay `write!(f, "{p}")`.
- [ ] **Step 4: Move `GpStarted` into `GpCard::Started`.**
  - `GpCard`'s `Display` for `Started` writes exactly the text `CrackedMessage::GpStarted` wrote, as `Line` writes its text. `CrackedMessage::Gp(Started)`'s `to_string()` is therefore unchanged.
  - `render_card` renders `Started` as `Rendered::card(Card { description: Some(clip(&text, DESCRIPTION_MAX)), colour: Some(colour::BLUE), ..Card::default() })`.
  - Move the pin from Step 1 and the 4 `GpStarted` text tests into `gp/ui.rs`'s tests, building `CrackedMessage::Gp(Box::new(GpCard::Started { … }))`, with assertions unchanged.
  - Update the sender at `commands.rs:474`.
- [ ] **Step 5: Run the gp, rooms, modlog and messaging tests.** `cargo nextest run -p crack-core -E 'test(/gp|room|modlog|messaging|render/)'`. Expected: PASS, including the existing byte pins `gp_cards_render_as_before`, the rooms card pins and the modlog render tests.
- [ ] **Step 6: Run the suite and lint** (Global Constraints). Expected: green.
- [ ] **Step 7: Commit.** `refactor: feature payloads render through messaging::render::Payload; /gp start is a GpCard`

### Task 2: The neutral files stop naming crack-core

**Files:**
- Modify: `crack-core/src/messaging/cards.rs`:
  - `NowPlayingCard.controls` becomes `rows: Vec<crack_model::Row>`;
  - `now_playing` uses `rows`;
  - receives `requesting_user_to_string`, `build_footer_info` and `finished_card`.
- Modify: `crack-core/src/messaging/interface.rs` (about lines 144, 324, 342, 677 and 696): `requesting_user_to_string` moves out, re-exported; build the rows where `Controls` is set today.
- Modify: `crack-core/src/messaging/status.rs` (about lines 324 and 406): clear `rows` where `controls = None` is set today; `finished_card` moves out, re-exported.
- Modify: `crack-core/src/utils.rs` (about lines 701 and 778): `build_footer_info` and `duration_to_string` move out, re-exported.
- Modify: `crack-core/src/messaging/format.rs`: receives `duration_to_string`; drops `TrackLabel::from_ref` and the `music::audit::TrackRef` import.
- Modify: `crack-core/src/music/audit.rs`: `impl TrackRef { pub fn label(&self) -> TrackLabel }`; update every `TrackLabel::from_ref(x)` caller to `x.label()`.
- Create: `crack-core/src/messaging/failure.rs`: `FailReason` (with `of` and `text`), `Failure`, `entry`, `render_text`, and their tests.
- Modify: `crack-core/src/messaging/track_failed.rs`:
  - keeps `WINDOW`, `LISTED_MAX`, `FailureNotice`, `NoticeSlot`, `impl Data`, `report`, `failures`, `notify` and their tests;
  - adds `pub use super::failure::{FailReason, Failure, render_text};`.
- Modify: `crack-core/src/messaging/mod.rs`: `pub mod failure;`.
- Modify: `crack-core/src/messaging/message.rs`:
  - `TrackFailed` names `failure::Failure`;
  - `duration_to_string` comes from `format`;
  - `CrackedHowResult` stays `crate::CrackedHowResult`, which Task 3 provides from crack-common.

**Interfaces:**
- Produces:
  - `pub struct NowPlayingCard { pub label: TrackLabel, pub thumbnail: Option<String>, pub requester: Option<UserRef>, pub progress: Progress, pub rows: Vec<crack_model::Row> }`. The `rows` doc comment says only the status message has rows, never a reply or a DM.
  - `crate::messaging::format::duration_to_string(Duration) -> String`.
  - `crate::messaging::cards::{build_footer_info(&str) -> (String, String, String), requesting_user_to_string(&UserRef) -> String, finished_card() -> crack_model::Card}`.
  - `crate::music::audit::TrackRef::label(&self) -> TrackLabel`.
  - `crate::messaging::failure::{FailReason, Failure, render_text}`.
- Consumes: `crate::messaging::buttons::Controls::rows(&self) -> Vec<Row>`, unchanged.

- [ ] **Step 1: Pin the buttons and the failure text before changing them.**
  - In `cards.rs`'s tests, the existing tests that set `controls: Some(Controls { … })` (about lines 511 and 563) are the button pins.
  - Add `a_non_snowflake_guild_gets_no_buttons`:
    - use a `Controls` whose guild ref is not a Discord snowflake;
    - assert the rendered `rows` are empty;
    - assert the card is otherwise equal to the same card with no controls.
  - The `track_failed.rs` tests on `render_text` (one track, several, "+N more", blank title) are the failure-text pins.
- [ ] **Step 2: Run them.** `cargo nextest run -p crack-core -E 'test(/cards|track_failed|now_playing/)'`. Expected: PASS on today's code.
- [ ] **Step 3: Implement the moves listed under Files.**
  - In the button pins, only construction changes: `controls: Some(c)` becomes `rows: c.rows()`, and the new test's empty case becomes `rows: Vec::new()`.
  - `now_playing` returns `rendered` unchanged when `rows` is empty, and `rendered.with_rows(rows.clone())` otherwise.
  - The `render_text` tests move to `failure.rs` unchanged.
- [ ] **Step 4: Check the neutral files' imports.** Run `grep -nE 'crate::(commands|music|rooms|modlog|utils|guild|handlers|db|economy|coin|sources|http_utils|poise_ext|Data|DataInner|Context)\b|messaging::(buttons|interface|status|courier|transport|discord|throttle|track_failed|placeholder)' crack-core/src/errors.rs crack-core/src/messaging/{messages,message,render,format,cards,failure}.rs`. Expected: matches only inside `#[cfg(test)]` modules, and those are the Discord-JSON pins Task 3 leaves in crack-core. List any non-test match in the report and remove it.
- [ ] **Step 5: Run the suite and lint.** Expected: green, with the button and failure pins unchanged apart from construction.
- [ ] **Step 6: Commit.** `refactor: messaging's neutral files stop naming crack-core (card rows, helpers, TrackRef::label, failure text)`

### Task 3: Create crack-common and move the files

**Files:**
- Create: `crack-common/Cargo.toml`:
  - `name = "crack-common"`, `version.workspace = true`, with the other package fields modelled on `crack-model/Cargo.toml`;
  - features `crack-osint = ["dep:crack-osint"]` and `mutation-testing = []`;
  - dependencies copied per Global Constraints: crack-model, crack-types, serenity, songbird, sqlx, reqwest, url, anyhow, tokio, audiopus, rspotify, rusty_ytdl, and optional crack-osint, plus whatever else the moved files use.
- Create: `crack-common/src/lib.rs`:
  - `pub mod cards; pub mod errors; pub mod failure; pub mod format; pub mod message; pub mod messages; pub mod render;`
  - `pub type CrackedResult<T> = std::result::Result<T, errors::CrackedError>;`
  - `pub type CrackedHowResult<T> = anyhow::Result<T, errors::CrackedError>;`
- Move (`git mv`) into `crack-common/src/`:
  - `crack-core/src/errors.rs` → `errors.rs`;
  - `crack-core/src/messaging/{messages,message,render,format,cards,failure}.rs` → the same names.
  - Inside them, `crate::messaging::X` becomes `crate::X`.
  - `poise::serenity_prelude` imports become `serenity::…`.
  - `crate::CrackedHowResult` and `crate::CrackedResult` resolve to the aliases above.
- Create: `crack-core/src/messaging/pins.rs` (`#[cfg(test)]`). It receives the render.rs and cards.rs tests that use `messaging::discord`, `interface::create_nav_btns` or `buttons::Controls`, with assertions unchanged.
- Modify:
  - `crack-core/src/messaging/mod.rs`: replace the six `pub mod` lines with `pub use crack_common::{cards, failure, format, message, messages, render};` and add `#[cfg(test)] mod pins;`.
  - `crack-core/src/lib.rs`: `pub use crack_common::errors;`. The `CrackedResult` and `CrackedHowResult` aliases become `pub use crack_common::{CrackedHowResult, CrackedResult};`.
- Modify: `crack-core/Cargo.toml`: depend on `crack-common = { path = "../crack-common" }`; `crack-osint = ["dep:crack-osint", "crack-common/crack-osint"]`.
- Modify: root `Cargo.toml` (add the `crack-common` member) and `Cargo.lock`.
- Modify: `crack-types/Cargo.toml`: `poise = { workspace = true, optional = true }`, plus a feature `poise = ["dep:poise"]`.
- Modify: `crack-types/src/lib.rs`: `#[cfg(feature = "poise")]` on `pub mod reply_handle;` and on `pub use reply_handle::*;`.
- Modify: `crack-core/Cargo.toml`: the crack-types dependency adds `features = ["poise"]`.
- Modify: `.github/workflows/lint.yml`:
  - add a step "crack-common depends on no poise or crack-core", modelled on the crack-model step: `cargo tree -p crack-common -e normal --prefix none | grep -E '^(poise|crack-core) '` must find nothing;
  - change the seam comment's `messaging/message.rs` to `crack-common/src/message.rs`.
- Modify: any doc or comment naming `crack-core/src/messaging/messages.rs` or `crack-core/src/errors.rs` (`grep -rn` the repo, excluding `target/`), pointing it at the new path.

**Interfaces:**
- Consumes: Task 1's `Payload` and Task 2's neutral helpers, now at `crack_common::render::Payload` etc.
- Produces: the `crack_common` crate. Every old path (`crack_core::errors::CrackedError`, `crack_core::messaging::message::CrackedMessage`, `crack_core::messaging::messages::*`, `crack_core::CrackedResult`, …) still resolves through the shims.

- [ ] **Step 1: Create the crate and move the files.** Do it as above, as one change, using `git mv`.
- [ ] **Step 2: Build crack-common on its own.** `cargo check -p crack-common` and `cargo nextest run -p crack-common` (under the cap). Expected: it compiles, and its moved tests pass. `cargo tree -p crack-common -e normal --prefix none | grep -E '^(poise|crack-core) '` prints nothing. The check covers the whole tree, so crack-types' `poise` feature must be off on this path.
- [ ] **Step 3: Build the workspace.** `cargo check --workspace --all-targets`. Expected: compiles with no call-site changes outside what this task lists. If a call site in crack-core, crack-web or crack-cli needs changing, list it in the report.
- [ ] **Step 4: Check the lock file.** `git diff Cargo.lock`. Expected: only the new `crack-common` package entry, plus `crack-common` added to crack-core's dependency list. No crate gains a second version.
- [ ] **Step 5: Run the suite, doctests and lint** (Global Constraints), including the seam check with `crack-osint`. Expected: green. The pins in `messaging/pins.rs` pass with assertions unchanged.
- [ ] **Step 6: Commit.** `refactor: crack-common holds errors, strings and neutral messages (split step 1)`

### Task 4 (controller, inline): v0.39.0

- [ ] Bump the version to `0.39.0` in the root `Cargo.toml` (`[workspace.package] version`) and `Cargo.lock`. Add a `CHANGELOG.md` entry in the style of v0.38.0's, saying no user-visible change.
- [ ] Run `cargo check --workspace` and the lint commands. Commit as `chore: v0.39.0`.
- [ ] Measure for the PR body:
  - topology `extract` under the 24G cap, then `diff` against the post-v0.38.0 digest;
  - `scripts/topology.sh timings`;
  - `cargo mutants --list` counts for crack-core and crack-common.
