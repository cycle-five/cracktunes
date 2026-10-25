# Neutral messages and output (PR 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `CrackedMessage`, its boxed payloads, `render` and `Rendered` hold no Discord type, and serenity's builders appear only in one Discord conversion module and the transports that send, with every message's Discord output byte-identical.

**Architecture:** New neutral output types (`Rendered`, `Card`, `Row`, `Control`, `Mentions`, `Mention`) go into `crack-model`. A conversion module, `crack-core/src/messaging/discord.rs`, turns them into serenity builders.
- Renderers and hand-built builder sites move to `Card`/`Row` one family at a time (Tasks 4–8). Each wraps its output back into today's Discord-typed `Rendered`, so every task ships with its own byte pins.
- Task 9 then swaps `Rendered` for the neutral one, moves the `to_*` methods into the conversion module, removes the wrappers, and locks the boundary with clippy's `disallowed-types`.

**Tech Stack:** Rust 2021, serenity (next branch) and poise, crack-model (serde and thiserror only), cargo-nextest, cargo-mutants.

**Spec:** `docs/superpowers/specs/2026-10-24-neutral-ids-and-messages-design.md`, §5 and its Testing and Measuring sections. PR 1 (ct#634, branch `refactor/neutral-ids`) is the base. This plan was written from an inventory of the code as PR 1 leaves it, as spec call 8 requires.

## Global Constraints

- **No user-visible change.** Every message's Discord payload stays byte-identical: content, embeds, components, allowed mentions, ephemeral flag, and edit/clear behaviour. `CrackedMessage`'s `Display` is unchanged byte for byte.
- **No storage, wire or dashboard change.** No migration, no SQL change.
- **`crack-model` stays platform-free.** Its normal dependencies remain `serde` (derive) and `thiserror`, and the lint.yml check stays green.
- **Refs at the seams only.** The poise command layer and the transports keep serenity ids. Payloads and messages hold refs, and convert once where they are built.
- **Change types in place; move only what must move.** A function moved between files counts as a NEW mutants miss: cover it with a test, or waive it only if it is live-Discord glue.
- **Tests are byte pins.** serenity's builders implement `Serialize`, so compare `serde_json::to_string(&old_builder)` with `serde_json::to_string(&converted)`.
  - Never use `serde_json::json!` or index a `serde_json::Value`.
  - Existing render and press-handler tests keep their assertions; only their construction changes. A changed assertion needs a stated reason in the report.
- **serenity facts:**
  - serenity serialises its id types as JSON strings.
  - In this serenity, `GuildId::new(0)` does not panic; `u64::MAX` does.
  - Every conversion from stored or external text is checked (PR 1's `crack_types::discord`).
- **Memory cap on every local cargo, nextest, mutants or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. systemd-run expands `$VARS`, so write paths and the cap in full, never through a shell variable.
- **Test database:** `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap):
  - suite: `cargo nextest run --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - doctests: `cargo test --doc --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - lint:
    - `cargo fmt --all -- --check`
    - `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`
    - `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`
    - `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`
    - `python3 scripts/check_waivers.py`
  - mutants: `python3 scripts/mutants_check.py --base <the task's base commit>`, run once, after the suite is green.
- **Waiting:** wait on long jobs in the foreground, with a blocking loop under the 10-minute Bash timeout. Never end the turn to wait for a notification.
- **Commits:**
  - never `git add -A`; add files by name;
  - run `cargo fmt` first;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.

## Calls this plan makes against the spec's sketch (from the inventory)

- **No `Control::Link`, `PickUsers` or `PickRoles`.**
  - The code builds no link button (`CreateButton::new_link` has 0 uses).
  - The only user and role pickers sit inside the voice-room modals (`rooms/interact.rs` `pick_user`/`pick_role`). Modals stay Discord (spec: input, not output).
  - Following the spec's own rule ("a superset of what cracktunes sends today, and nothing more"), they are left out.
- **`CrackedMessage::ErrorHttp` is deleted.** Nothing constructs it. Carrying its text would keep a dead variant.
- **Message-content clipping moves into the Discord conversion.**
  - `messaging::format::clip` and `CONTENT_MAX` stay where they are.
  - `discord.rs` clips `content` on every conversion. The neutral `Rendered::text` and `with_content` store what they are given.
  - One choke point at the Discord edge cannot be bypassed, and the limit is Discord's.
  - Embed description clipping stays in the renderers, unchanged.
- **Other neutral fields:**
  - a card's timestamp is Unix seconds (`Option<i64>`), matching its only user (`modlog/event.rs`);
  - emoji are unicode strings (all 3 emoji uses are unicode);
  - colour is `u32`.

## Review Focus

1. **A message that pinged nobody must still ping nobody.** `Mentions::None` converts to an empty `CreateAllowedMentions`, whatever mentions the content holds. Pinned in Task 2 and again in Task 9.
2. **Content over 2000 characters must still be clipped** on every send path (`to_reply`, `to_reply_edit`, `to_message`, `to_interaction_message`, `to_followup`, `to_edit`), now that clipping lives in the conversion. Pinned in Task 9.
3. **Editing a status that loses its buttons must clear them.** `to_edit` of a `Rendered` with no rows serialises `"components":[]` and empty content as `""`. Pinned in Task 9.
4. **A button's custom id is the contract with its press handler** and must reach Discord unchanged. The conversion copies `id` verbatim, and the existing press-handler tests keep passing. Pinned in Task 2.
5. **A ref-built mention must print exactly what serenity's `Mention` printed** (`<@123>`, `<#7>`, `<@&9>`), so every moderation and summon message reads the same. Pinned in Tasks 1 and 3.

---

### Task 1: Neutral output types in `crack-model`

**Files:**
- Create: `crack-model/src/output.rs`
- Modify: `crack-model/src/lib.rs` (`pub mod output; pub use output::*;`)

**Interfaces:**
- Consumes: `crack_model::{UserRef, RoomRef, RoleRef}` (PR 1).
- Produces:
  - **`Rendered`:** `#[derive(Debug, Clone, Default, PartialEq)] pub struct Rendered { pub content: Option<String>, pub cards: Vec<Card>, pub rows: Vec<Row>, pub mentions: Mentions }`, with:
    - `Rendered::text(impl Into<String>) -> Self` (sets content, no clipping);
    - `Rendered::card(Card) -> Self`;
    - `with_content(self, impl Into<String>) -> Self`;
    - `with_rows(self, Vec<Row>) -> Self`;
    - `with_cards(self, Vec<Card>) -> Self` (appends after the first card);
    - all `#[must_use]`.
  - **`Card`:** `#[derive(Debug, Clone, Default, PartialEq)] pub struct Card { pub title: Option<String>, pub url: Option<String>, pub description: Option<String>, pub author: Option<Author>, pub fields: Vec<Field>, pub thumbnail: Option<String>, pub footer: Option<Footer>, pub timestamp: Option<i64>, pub colour: Option<u32> }`.
  - **Card parts:**
    - `pub struct Author { pub name: String, pub url: Option<String>, pub icon_url: Option<String> }`;
    - `pub struct Field { pub name: String, pub value: String, pub inline: bool }`;
    - `pub struct Footer { pub text: String, pub icon_url: Option<String> }`.
  - **Controls:**
    - `pub struct Row(pub Vec<Control>)`;
    - `pub enum Control { Button { id: String, label: Option<String>, emoji: Option<String>, style: ButtonStyle, disabled: bool }, Select { id: String, placeholder: Option<String>, options: Vec<SelectOption>, min: Option<u8>, max: Option<u8> } }`;
    - `pub enum ButtonStyle { Primary, Secondary, Success, Danger }`;
    - `pub struct SelectOption { pub label: String, pub value: String, pub default: bool }`.
  - **Mentions:**
    - `#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)] pub enum Mentions { #[default] None, Users }`;
    - `pub enum Mention { User(UserRef), Room(RoomRef), Role(RoleRef) }`, with `Display` writing `<@{raw}>`, `<#{raw}>` and `<@&{raw}>` from the ref's raw id.
  - All structs derive `Debug, Clone, PartialEq` (plus `Eq` where every field allows it). No serde: nothing serialises them.

- [ ] **Step 1: Write the failing tests** in `output.rs`:
  - `a_mention_prints_discords_markdown`: `Mention::User(UserRef::new(Platform::Discord, "123")?)` displays `<@123>`, a `Room` with raw `"7"` displays `<#7>`, and a `Role` with raw `"9"` displays `<@&9>`.
  - `text_keeps_its_content_unclipped`: `Rendered::text("x".repeat(3000)).content.unwrap().len() == 3000`.
  - `a_card_then_extra_cards_keep_their_order`: `Rendered::card(a).with_cards(vec![b, c]).cards == vec![a, b, c]`.
  - `the_default_mentions_nobody`: `Rendered::default().mentions == Mentions::None`.
- [ ] **Step 2: Run them and see them fail** with `cargo nextest run -p crack-model output`: compile errors.
- [ ] **Step 3: Implement the types.**
- [ ] **Step 4: Run them and see them pass.** Then run the lint.yml crate check by hand (`cargo tree -p crack-model -e normal` names no platform crate).
- [ ] **Step 5: Run mutants and commit** as `feat(model): neutral output types (Rendered, Card, Row, Mention)`.

### Task 2: The Discord conversion module

**Files:**
- Create: `crack-core/src/messaging/discord.rs`
- Modify: `crack-core/src/messaging/mod.rs` (`pub mod discord;`); `crack-core/src/messaging/render.rs` (`Mentions` becomes `pub use crack_model::Mentions;`, the same two variants).

**Interfaces:**
- Consumes: Task 1's types.
- Produces, in `crate::messaging::discord`:
  - `pub fn embed(card: &Card) -> CreateEmbed<'static>`
  - `pub fn action_rows(rows: &[Row]) -> Vec<CreateComponent<'static>>`. Each `Row` becomes one `CreateComponent::ActionRow`: buttons become `CreateActionRow::Buttons`, a select becomes `CreateActionRow::SelectMenu`, and `Select` becomes `CreateSelectMenuKind::String`.
  - `pub fn allowed_mentions(m: Mentions) -> CreateAllowedMentions<'static>`. `None` gives `CreateAllowedMentions::new()`, and `Users` gives `.all_users(true)`, as `render.rs` does today.
  - **Field mapping:**
    - `timestamp`: `Timestamp::from_unix_timestamp(secs)`, skipped if `Err`;
    - `colour`: `Colour::new(u32)`;
    - `emoji`: `ReactionType::Unicode`.

- [ ] **Step 1: Write the failing byte-pin tests**, one per field: `serde_json::to_string(&embed(&card))` must equal `serde_json::to_string(&<the serenity builder chain the code writes today>)`.
  - `every_card_field_converts_like_the_builder`: title, url, description, author (name, url, icon_url), several fields (inline and not), thumbnail, footer (with and without an icon), timestamp, colour, and an all-`None` card.
  - `every_control_converts_like_the_builder`:
    - a button in each style, with and without label, emoji and `disabled`;
    - a select with placeholder, min, max, and an option with `default_selection(true)`;
    - two rows in order.
  - `a_custom_id_reaches_discord_verbatim`: an id with `:` and digits survives byte for byte.
  - `mentions_none_allows_nobody` and `mentions_users_allows_users`: the JSON matches the two `CreateAllowedMentions` values `render.rs` builds today.
- [ ] **Step 2: Run them and see them fail** (compile errors).
- [ ] **Step 3: Implement the three functions.**
- [ ] **Step 4: Run the suite and see it pass.**
- [ ] **Step 5: Run mutants and commit** as `feat(messaging): convert neutral cards and rows to serenity builders`.

### Task 3: `CrackedMessage` holds refs and text

**Files:**
- Modify: `crack-core/src/messaging/message.rs`; `crack-core/src/messaging/render.rs` (the generic arm's colour); and every construction site of the changed variants:
  - `commands/music/summon.rs`, `commands/music/voteskip.rs`, `commands/music_utils.rs`;
  - `messaging/interface.rs`;
  - the moderation and admin commands that build `User*`, `Role*`, `AutoRole`, `TextChannelCreated` and `VoiceChannelCreated`;
  - the seek path (`SeekFail`);
  - the 11 `CrackedMessage::CrackedError` sites;
  - the 5 `CrackedMessage::CreateEmbed` sites.

**Interfaces:**
- Consumes: Task 1's `Mention` and `Card`; Task 2's `discord::embed`; PR 1's `ToRef`.
- Produces:
  - **Ids become refs:** every `UserId`, `RoleId`, `GenericChannelId` and `GuildId` field becomes `UserRef`, `RoleRef`, `RoomRef` or `CommunityRef`.
  - **Mentions:** every `mention: Mention` field is removed. `Display` mentions from the ref with `crack_model::Mention`.
  - **Names:** `FixedString` fields (`role_name`, `channel_name`, `timeout_until`) become `String`.
  - **Text in place of error types:**
    - `SeekFail { error: String }`;
    - `CrackedError(String)`, with `impl From<CrackedError> for CrackedMessage` building it from `error.to_string()`;
    - `ErrorHttp` is deleted.
  - **Card:** `CreateEmbed(Box<CreateEmbed>)` becomes `Card(Box<Card>)`. `render` turns it into `Rendered::embed(discord::embed(&card))` until Task 9.
  - **Colour:** `pub fn colour(&self) -> u32` replaces `impl From<&CrackedMessage> for Colour`, with the same RGB values. `render` uses `Colour::new(msg.colour())`.
  - After this task, `message.rs` imports nothing from serenity, poise, songbird or small_fixed_array outside `#[cfg(test)]`.

- [ ] **Step 1: Pin today's output before changing anything.** Write `every_changed_variant_displays_as_before`.
  - Build each changed variant with today's types and fixed ids (user 123, role 9, channel 7, guild 5).
  - Capture its `to_string()` from the base commit, and assert those exact strings.
  - Also write `colours_are_unchanged`: one variant per colour arm, asserting the `u32` that `Colour::from` gives today. Capture the values at the base.
  - These pass at the base. They are the pins that the change must keep.
- [ ] **Step 2: Change the types,** and watch the tests stop compiling (RED).
- [ ] **Step 3: Rewrite the tests' construction to refs** with the same expected strings, and make them pass. Then update every construction site: `user.id.to_ref()` and the like, with no `.mention()`.
- [ ] **Step 4: Run the full suite.** Hand-sabotage: change `Mention::Role`'s spelling and watch a pin fail; restore it.
- [ ] **Step 5: Run mutants and commit** as `refactor(messaging): CrackedMessage holds refs and text, not Discord types`.

### Task 4: Music cards and controls render neutral

**Files:**
- Modify:
  - `crack-core/src/messaging/cards.rs` (`NowPlayingCard`, `QueuedCard`, `Via`, `EchoLine`, `Echo`);
  - `crack-core/src/messaging/buttons.rs` (`Controls`, `NowPlayingButton`'s row building);
  - `crack-core/src/messaging/track_failed.rs`;
  - `crack-core/src/messaging/render.rs` (the generic `Style::Embed` arm and `TrackFailed`);
  - the construction sites of these payloads.

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces:
  - The payloads hold refs in place of serenity ids, and `String` in place of `FixedString`.
  - Each renderer builds a `Card` and a `Vec<Row>` in place of serenity builders: `cards::now_playing`, `queued`, `echo` and `finished`; the `buttons.rs` row builders (e.g. `Controls::rows(&self) -> Vec<Row>`); `render`'s generic arm.
  - Each renderer then returns today's `Rendered` through `discord::embed`/`discord::action_rows`. The neutral flip is Task 9.
  - Custom ids are unchanged. `buttons.rs`'s parse functions are untouched.

- [ ] **Step 1: Pin today's output.** Write `music_cards_render_as_before`, covering a now-playing card with controls, a queued card, an echo line, finished, a track-failed list, and one generic embed message.
  - For each, assert `serde_json::to_string` of the `Rendered`'s embed(s) and components, captured at the base commit.
  - These pass at the base.
- [ ] **Step 2: Convert the payload types and renderers.** The pins must still pass unchanged, and every existing render and press-handler test must too.
- [ ] **Step 3: Run the full suite.**
- [ ] **Step 4: Run mutants and commit** as `refactor(messaging): music cards and controls build neutral cards and rows`.

### Task 5: `/gp` renders neutral

**Files:**
- Modify:
  - `crack-core/src/commands/music/gp/ui.rs` (`GpCard`, `GpComponent`, `render_card`, `gp_rendered`, the select and button builders);
  - `crack-core/src/commands/music/gp_prompts.rs` (`GpCategory`);
  - `crack-core/src/commands/music/gp/state.rs` (`GpReveal`);
  - `crack-core/src/commands/music/gp/commands.rs` (the command parameters).

**Interfaces:**
- Consumes: Tasks 1–4.
- Produces:
  - The `GpCard` family holds refs, and its renderers build `Card`/`Row`, wrapped as in Task 4.
  - `GpCategory` and `GpReveal` lose `poise::ChoiceParameter`.
  - Mirror enums `GpCategoryChoice` and `GpRevealChoice` in `gp/commands.rs` derive `ChoiceParameter`, with the same variant names and `#[name = …]` strings, and implement `From<…Choice>` into the domain enums.
  - The command functions take the mirror types and convert once.
  - Any domain code that used `ChoiceParameter::name()` gets an explicit `fn label(&self) -> &'static str` with the same strings.

- [ ] **Step 1: Pin today's output.**
  - `gp_cards_render_as_before`: the select menu (players), the category picker with a default selection, a disabled confirm button, and a reveal card. Assert the JSON captured at the base.
  - `the_gp_choices_keep_their_names`: each mirror variant's `ChoiceParameter` name equals today's.
- [ ] **Step 2: Convert,** keeping the pins and existing gp tests green.
- [ ] **Step 3: Run the full suite.**
- [ ] **Step 4: Run mutants and commit** as `refactor(gp): /gp cards render neutral; choice enums mirror the domain`.

### Task 6: Voice rooms and the mod log render neutral

**Files:**
- Modify:
  - `crack-core/src/rooms/card.rs` (the `RoomCard` family, `PanelView`, `RoomDone`, `RoomRefusal`, `render_card`);
  - the output builders in `crack-core/src/rooms/interact.rs`. The panel and its buttons move; the modals (`CreateModal`, `CreateInputText`, and the select menus inside modals) stay;
  - `crack-core/src/modlog/event.rs` (`Actor`, `Attribution`, `ModLogEvent`, `render`);
  - their construction sites.

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - These payloads hold refs, and their renderers build `Card`/`Row`, wrapped as in Task 4.
  - The mod log's card timestamp is `Some(cx.now_unix)`.

- [ ] **Step 1: Pin today's output:** `room_cards_render_as_before` and `modlog_events_render_as_before`. One case per `RoomCard` and `ModLogEvent` variant that builds an embed or components. Assert the JSON captured at the base.
- [ ] **Step 2: Convert,** keeping the pins and the existing rooms and modlog tests green.
- [ ] **Step 3: Run the full suite.**
- [ ] **Step 4: Run mutants and commit** as `refactor(rooms, modlog): cards render neutral`.

### Task 7: Music command sites build cards

**Files:**
- Modify: the hand-built builder sites in `crack-core/src/commands/music/{queue,doplay,dosearch,spotify,volume,remove,collector,manage_sources}.rs`, `crack-core/src/handlers/track_end.rs` (the queue page flip), `crack-core/src/music/ops/edit.rs`, `crack-core/src/utils.rs` and `crack-core/src/http_utils.rs`.

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces:
  - These files build `Card`/`Row` and wrap them into today's `Rendered` through `discord::embed`/`discord::action_rows`, as in Task 4. The neutral flip is Task 9. Apart from the wrapper calls, they name no `CreateEmbed`, `CreateEmbedAuthor`, `CreateEmbedFooter`, `CreateButton`, `CreateSelectMenu`, `CreateSelectMenuOption`, `CreateActionRow` or `CreateComponent`.
  - They may still name serenity ids and contexts: the command layer keeps them.

- [ ] **Step 1: Pin today's output.** For each file's builder function, pin its JSON at the base in a `*_renders_as_before` test, e.g. `queue_page_renders_as_before` and `song_select_menu_renders_as_before`.
  - Where a builder is inline in a command body, first extract it in place into a pure function in the same file, with the pin test.
- [ ] **Step 2: Convert,** keeping the pins green.
- [ ] **Step 3: Run the full suite,** then mutants, then commit as `refactor(commands): music commands build neutral cards`.

### Task 8: The remaining builder sites

**Files:**
- Modify:
  - `crack-core/src/commands/{status,register}.rs`, `commands/utility/ping.rs`, `commands/admin/debug.rs`;
  - `crack-core/src/poise_ext.rs`, `crack-core/src/handlers/serenity.rs`;
  - `crack-core/src/economy/buttons.rs` and the shop/credits card builders under `economy/` and `commands/economy/`;
  - the output builders in `crack-core/src/messaging/interface.rs` (embeds and buttons built for display).

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces: these files build `Card`/`Row` and wrap them into today's `Rendered`, as in Task 7.

- [ ] **Step 1: Pin today's output** for each converted builder, as in Task 7.
- [ ] **Step 2: Convert.**
- [ ] **Step 3: Run the full suite,** then mutants, then commit as `refactor(messaging): the remaining cards and buttons are neutral`.

### Task 9: `Rendered` becomes neutral, and clippy locks the boundary

**Files:**
- Modify:
  - `crack-core/src/messaging/render.rs`: `pub use crack_model::Rendered;` in place of the struct. The `to_*` methods and `allowed_mentions` move out.
  - `crack-core/src/messaging/discord.rs`: gains the send conversions.
  - Every file that uses `Rendered`'s fields or methods (35 files; `grep -rlw Rendered crack-core/src`).
  - The Task 4–8 wrappers come out.
  - `clippy.toml`.

**Interfaces:**
- Consumes: Tasks 1–8.
- Produces:
  - `pub trait ToDiscord` in `discord.rs`, implemented for `crack_model::Rendered`, with today's names and signatures: `to_reply(&self, ephemeral: bool)`, `to_reply_edit<'a>(&self)`, `to_message`, `to_interaction_message`, `to_followup(&self, ephemeral: bool)`, `to_edit`, `allowed_mentions`.
  - Callers `use crate::messaging::discord::ToDiscord;`.
  - Every conversion clips `content` with `clip(.., CONTENT_MAX)`.
  - `to_edit` keeps clearing: no rows means `components([])`, and no content means `""`.
  - `render.rs`'s test helper `description(r)` reads `r.cards.first()?.description`.
  - **`clippy.toml`:** `disallowed-types` lists `serenity::builder::CreateEmbed`, `CreateEmbedAuthor`, `CreateEmbedFooter`, `CreateButton`, `CreateSelectMenu`, `CreateSelectMenuOption`, `CreateActionRow` and `CreateComponent`, each with the reason "build crack_model::Card/Row; convert in messaging::discord". Use whatever paths serenity's re-exports resolve to, and check that clippy fires.
  - **Allowed builder files.** `#[allow(clippy::disallowed_types)]` is permitted only:
    - at the top of `messaging/discord.rs`;
    - on the transports (`messaging/courier.rs`, `messaging/transport.rs`, `messaging/status.rs`);
    - on the press-handling functions of `messaging/interface.rs`;
    - on the modal builders in `rooms/interact.rs`;
    - in test modules.

    Each allow carries a `//` reason.

- [ ] **Step 1: Write the failing tests** in `discord.rs`:
  - `every_send_path_clips_long_content`: a 3000-character `Rendered::text`, through `to_message`, `to_interaction_message`, `to_followup` and `to_edit`, serialises with content of exactly `CONTENT_MAX` characters. `to_reply`'s `CreateReply` isn't `Serialize`; check its content through its public field or accessor if one exists, otherwise name the limit in the waiver.
  - `an_edit_without_rows_clears_the_buttons`: `to_edit` of `Rendered::text("x")` serialises `"components":[]`.
  - `mentions_none_survives_every_send_path`: content `<@123>` with `Mentions::None` serialises `"allowed_mentions":{"parse":[]}`, or whatever today's `CreateAllowedMentions::new()` writes, on every serialisable path.
  - `the_send_paths_match_the_old_ones`: for one `Rendered` with content, two cards and a row, each serialisable path's JSON equals the base commit's, captured before the flip.
- [ ] **Step 2: Run them and see them fail.**
- [ ] **Step 3: Flip the type,** move the methods, and update every user. Existing tests change only their construction (`Rendered::embed(x)` becomes `Rendered::card(card)`).
- [ ] **Step 4: Run the full suite.** Hand-sabotage: drop the clip in `to_message`, and watch the clipping test fail; restore it.
- [ ] **Step 5: Add the clippy entries.** Prove they bite: temporarily add a `CreateEmbed::new()` in `commands/utility/ping.rs` and watch clippy fail; remove it.
- [ ] **Step 6: Run the done-when check.**
  - `grep -rlwE 'CreateEmbed|CreateButton|CreateSelectMenu|CreateActionRow|CreateComponent' crack-core/src` lists only `messaging/discord.rs`, the transports, `interface.rs`, `rooms/interact.rs` (modals) and test files.
  - Report anything else with its reason.
- [ ] **Step 7: Run mutants.** Moved methods must be caught by the tests above. `to_reply`/`to_reply_edit` may be waived as live-Discord glue if poise's builder can't be observed.
- [ ] **Step 8: Commit** as `refactor(messaging): Rendered is neutral; clippy keeps builders in messaging::discord`.

### Task 10: Version, changelog, final verification

**Files:** `Cargo.toml` (`[workspace.package] version = "0.38.0"`), `Cargo.lock`, `CHANGELOG.md`.

- [ ] **Step 1: Bump the version** and run `cargo update --workspace` under the cap. The lock changes only the workspace members' versions.
- [ ] **Step 2: Update the changelog.** Under `## Unreleased` → `### Internal` (the section PR 1 added), append:
  - **Messages are platform-neutral.** `CrackedMessage` and every card it carries hold refs and plain text, and rendering produces `crack_model::Card`/`Row` instead of serenity builders. `messaging::discord` is the one place that turns them into Discord messages, and clippy keeps it that way. What the bot sends is unchanged.
- [ ] **Step 3: Run every verification command.** All pass.
- [ ] **Step 4: Commit** with `git add Cargo.toml Cargo.lock CHANGELOG.md` and the message `chore: v0.38.0`.
