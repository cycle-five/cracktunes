# Economy log, hidden buyers, /economy reveal and room mute — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the economy its own public log that never names a hidden buyer, make the mod log record the shop's member changes literally (no buyer), add a free moderator `/economy reveal`, stop unsigned cards falling back to a labelled follow-up, and make the room Mute button server-mute members while they are in the room.

**Architecture:** `crack-economy` gains `ledger` (the economy log: event type, text render, poster) and a logging wrapper around `EconomyDiscord` whose sink `crack-rooms` implements by posting to the mod log; `crack-core` wires both. `crack-rooms` gains `rooms::mute_hold` (a hold table, one pure decision function and one serialized `settle`) driven from room actions, voice events and reconcile.

**Tech Stack:** Rust, serenity (`next`), poise, sqlx 0.9 (Postgres, offline `.sqlx`), tokio, the dashboard in `crack-web` (askama-free HTML strings + `assets/settings.js`).

**Spec:** `docs/superpowers/specs/2026-10-28-economy-log-and-room-mute-design.md` (owner-approved 2026-10-28). Read it before your task; it is the authority.

## Global Constraints

- Release **v0.46.0**. Work on branch `feat/economy-log-room-mute` in `/home/lothrop/projects/cracktunes-split`.
- Every log line is written as if the whole server reads it: a hidden (unsigned, unrevealed) buyer, starter or backer appears in no channel post, log line or card.
- Mentions never ping: economy log lines and the reveal line are `Rendered::text` (its `mentions` is `Mentions::None`); mod log embeds stay as they are.
- User-facing strings are `pub const`s in `crack-common/src/messages.rs`; logs (`tracing`) stay inline.
- Typed serde only: no `serde_json::json!` / `Value` for data we own.
- Explicit imports (clippy gates globs in non-test code). Enforce rules with types, not source scans.
- Run every cargo command under the memory cap, written out each time (never in a shell variable):
  `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue -- cargo …`
- Builds use `SQLX_OFFLINE=true`. DB tests need the local postgres (`docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine` if `docker --context default ps` doesn't list `pg`) and the crate's `db-tests` feature.
- No live-network tests. Never `cargo clean`. Never build Docker locally. Never `git add -A`: add the files you changed by name.
- Commits end with exactly one trailer line: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.
- Clippy must pass: `cargo clippy --all --all-targets -- -D warnings` and `cargo clippy -p crack-core --features crack-coin --all-targets --locked -- -D warnings`.
- Glue that needs a live poise `Context` keeps the file's existing `#[cfg_attr(test, mutants::skip)]` + comment convention; put logic in pure functions that are tested.

## Review Focus

1. **A room-muted member must never stay server-muted after leaving** — through a disconnect, a kick, the room being deleted, a bot restart, or an Unmute press whose unmute fails. Pinned by Task 7's `settle` exit tests and Task 8's reconcile-order test.
2. **A moderator's unmute in the room sticks** until the member rejoins the room, even if the member then toggles self-mute or their camera (an in-place voice update). Pinned by Task 7's `in_place_update_never_takes_a_hold` test.
3. **A crowdfund backer who pledged twice, once signed and once not, is named once**, and the hidden count counts people, not pledges. Pinned by Task 3's `landed_bylines_follow_the_signers` test.
4. **An economy log channel the bot can't post in** fails quietly (a `tracing::warn!`) and changes nothing else about the purchase or the moderator action. Pinned by Task 1's `a_failed_post_is_only_logged` test.
5. **A nickname with markdown in it** (`**x**`, `` `x` ``, `@everyone`) renders escaped in the mod log's literal line. Pinned by Task 2's `shop_nick_line_escapes_the_name` test.

---

### Task 1: The economy log (crack-economy `ledger`)

**Files:**
- Modify: `crack-economy/src/settings.rs` (`EconomyConfig` + `Default` + serde test)
- Create: `crack-economy/src/ledger.rs`; Modify: `crack-economy/src/lib.rs` (`pub mod ledger;`)
- Modify: `crack-economy/src/preflight.rs` (log channel findings)
- Modify: `crack-economy/Cargo.toml` (dev-dep `crack-common` with `test-support`)
- Modify: `crack-common/src/messages.rs` (`ECONLOG_*` constants)

**Interfaces:**
- Produces:
  - `EconomyConfig.log_channel_id: Option<ChannelId>` (JSON key `log_channel_id`, string id like `announce_channel_id`; absent → `None`).
  - `ledger::Byline` — `pub struct Byline(Option<UserId>)` with a **private** field; `pub fn new(user: UserId, shown: bool) -> Self`; `pub fn shown(&self) -> Option<UserId>`. The only constructor; a hidden byline stores no id.
  - `ledger::Ender { Target, Moderator }`, `ledger::Unmasker { Target, Moderator }`.
  - `ledger::EconomyLogEvent` (derive `Debug, Clone, PartialEq, Eq`):
    - `Purchase { action: ActionKind, buyer: Byline, target: UserId, note: Option<String>, until: Option<DateTime<Utc>> }`
    - `Bounced { action: ActionKind, buyer: Byline, owner: UserId }` (owner = whose mirror)
    - `CrowdfundStarted { action: ActionKind, starter: Byline, target: UserId, note: Option<String> }`
    - `CrowdfundLanded { action: ActionKind, target: UserId, backers: Vec<Byline>, until: Option<DateTime<Utc>> }`
    - `PranksEnded { target: UserId, by: Ender }`
    - `ImmunityGranted { target: UserId, until: Option<DateTime<Utc>>, reason: Option<String> }`
    - `ImmunityLifted { target: UserId }`
    - `CreditsAdjusted { target: UserId, amount: i64, reason: String }`
    - `Unmasked { action: ActionKind, target: UserId, buyer: UserId, by: Unmasker }`
  - `ledger::line(e: &EconomyLogEvent) -> String` (pure, exact copy below) and `ledger::render(e: &EconomyLogEvent) -> Rendered` (= `Rendered::text(line(e))`).
  - `ledger::post(data: &EconomyData, transport: &dyn Transport, g: GuildId, e: EconomyLogEvent)` (async): posts to `log_channel_id` via `crack_common::courier::post_to_channel`; with no channel, `tracing::debug!` and return; a failed post → `tracing::warn!` only.
  - `preflight::NEED_LOG: Permissions = VIEW_CHANNEL | SEND_MESSAGES` (text lines need no Embed Links).
  - `preflight::log_findings(c: &EconomyConfig, perms: Option<&BotPerms>, name_of: impl Fn(ChannelId) -> String) -> Option<Vec<String>>` — same shape and wording as the announce findings; `None` with no channel or no perms. `report_for`/`bot_perms` callers that want it pass `log_channel_id` in the channel list.

**Exact copy** (`{t}`, `{b}`, `{o}` are `<@id>`; `{who}` is `<@id>` or `ECONOMY_CARD_SOMEONE` ("Someone"); `{Label}` is `ActionKind::label()`, `{label}` its lowercase; `{until}` is ` until <t:N:f>` or empty):
- Purchase: `{who} used {Label} on {t}{until}`
- Bounced: `{who}'s {label} bounced off {o}'s mirror`
- CrowdfundStarted: `{who} started a crowdfund: {Label} on {t}`
- CrowdfundLanded: `Crowdfund landed: {Label} on {t}{until}, backed by {backers}` — `{backers}`: the shown backers' mentions joined `", "`, then ` +1 other` / ` +N others` for the hidden ones; with none shown, `1 anonymous backer` / `N anonymous backers`.
- PranksEnded: Target → `{t} ended the pranks on them`; Moderator → `A moderator ended the pranks on {t}`
- ImmunityGranted: `A moderator made {t} immune until <t:N:f>` / `A moderator made {t} immune until lifted`
- ImmunityLifted: `A moderator lifted {t}'s immunity`
- CreditsAdjusted: `A moderator adjusted {t}'s balance by +5` / `by −5` (minus is `MODLOG_MINUS`)
- Unmasked: Target → `{t} unmasked the {label}: {b}`; Moderator → `A moderator unmasked the {label} on {t}: {b}`
- A non-blank note or reason (Purchase, CrowdfundStarted, ImmunityGranted, CreditsAdjusted) is appended as `\n> {escaped}` using `crack_common::format::escape`.

- [ ] **Step 1: Write the failing tests** in `ledger.rs` `#[cfg(test)]`:
  - `every_line_reads_exactly` — one case per variant and branch above (both Ender/Unmasker arms, until set/unset, note set/blank, 0/1/2 hidden backers with and without shown ones), asserting the full string, e.g. `assert_eq!(line(&Purchase{action: Rename, buyer: Byline::new(B, false), target: T, note: None, until: None}), "Someone used Rename on <@11>")`.
  - `no_render_of_a_hidden_buyer_contains_their_id` — for a sentinel `B = UserId::new(987654321)`, build every variant that carries a `Byline` with `Byline::new(B, false)` (every backer hidden for CrowdfundLanded) and assert `!line(&e).contains("987654321")`; then the same events with `shown = true` DO contain it (proves the test can fail).
  - `renders_ping_nobody` — `render(&e).mentions == Mentions::None` for every variant.
  - `posts_to_the_log_channel_only_when_set` — with `FakeTransport`, no channel → `texts()` empty; channel 77 set → exactly one send to 77 with `line(&e)`.
  - `a_failed_post_is_only_logged` — `FakeTransport` with one queued send failure: `post` returns normally, nothing else sent.
  - In `settings.rs`: `log_channel_round_trips_as_a_string` (`"log_channel_id":"5"` serialized; old JSON without the key reads `None`).
  - In `preflight.rs`: `log_findings_name_what_the_log_channel_lacks` (no channel → `None`; channel with `NEED_LOG` → `Some(vec![])`; missing Send → one finding naming the channel).
- [ ] **Step 2: Run them and see them fail** — `systemd-run … -- cargo nextest run -p crack-economy ledger settings preflight` → compile errors.
- [ ] **Step 3: Implement** the interfaces above. Add `crack-common = { path = "../crack-common", features = ["test-support"] }` to crack-economy's `[dev-dependencies]`. Constants go in `messages.rs` as `ECONLOG_*` word pieces, matching how `MODLOG_*` pieces are used.
- [ ] **Step 4: Run them and see them pass**, then sabotage one (render a hidden byline's id) and see `no_render_of_a_hidden_buyer_contains_their_id` fail; revert.
- [ ] **Step 5: Commit** — `git add crack-economy/src/ledger.rs crack-economy/src/lib.rs crack-economy/src/settings.rs crack-economy/src/preflight.rs crack-economy/Cargo.toml crack-common/src/messages.rs Cargo.lock` then `git commit` ("feat(economy): an economy log that never names a hidden buyer").

### Task 2: The mod log's literal shop lines (the hook)

**Files:**
- Modify: `crack-economy/src/discord.rs` (`ShopChange`, `ShopChangeLog`, `Logged`, `LiveEconomyDiscord`)
- Modify: `crack-economy/src/effects.rs:691` (`start_sweeper`), `crack-economy/src/lock.rs:321` (`spawn_jail_recheck`)
- Modify: `crack-rooms/src/modlog/event.rs` (new variant), `crack-rooms/src/modlog/live.rs` (`ShopChangeSink`), `crack-common/src/messages.rs`
- Modify: `crack-core/src/economy/mod.rs` (`live_discord`), and every `LiveEconomyDiscord {…}` site: `crack-core/src/commands/economy/{shop.rs:373, crowdfund.rs:285, credits.rs:231}`, `crack-core/src/handlers/serenity.rs:71,102,118,157,389,663`

**Interfaces:**
- Produces (crack-economy `discord`):
  - `pub enum ShopChange { Nick(Option<String>), Mute(bool), Deaf(bool), Move(ChannelId) }` (`Debug, Clone, PartialEq, Eq`)
  - `pub trait ShopChangeLog: Send + Sync { fn changed(&self, g: GuildId, u: UserId, change: ShopChange, kind: ActionKind); }` — synchronous; an implementation that does I/O spawns it.
  - `pub struct Logged<D> { inner: D, log: Arc<dyn ShopChangeLog> }`, `Logged::new(inner: D, log: Arc<dyn ShopChangeLog>) -> Self`, `impl<D: EconomyDiscord> EconomyDiscord for Logged<D>`: each of `set_nick/set_mute/set_deaf/move_member` forwards and, **only on `Ok`**, calls `log.changed` once; `member_nick` forwards without logging.
  - `LiveEconomyDiscord` keeps `http` but makes it **private**; its only constructor is `LiveEconomyDiscord::logged(http: Arc<Http>, log: Arc<dyn ShopChangeLog>) -> LiveShopDiscord`, with `pub type LiveShopDiscord = Logged<LiveEconomyDiscord>;`. Nothing outside crack-economy can build an unlogged live door.
  - `start_sweeper(data: EconomyData, discord: LiveShopDiscord, cache: Arc<Cache>)`; `spawn_jail_recheck(data: EconomyData, discord: LiveShopDiscord, cache: Arc<Cache>, guild: GuildId, user: UserId, at: Instant)`.
- Produces (crack-rooms `modlog`):
  - `ModLogEvent::ShopChange { target: UserRef, change: ShopChange, action: ActionKind }` — no buyer field.
  - `pub fn shop_change_event(u: UserId, change: ShopChange, kind: ActionKind) -> ModLogEvent`.
  - `pub struct ShopChangeSink { pub data: RoomsData, pub transport: Arc<dyn Transport> }` implementing `ShopChangeLog`: returns at once when `!data.modlog.is_enabled(g)`; otherwise `tokio::spawn`s `ChannelSink { data, transport }.post(g, shop_change_event(..))`.
- Produces (crack-core): `pub fn live_discord(data: &Data, http: Arc<Http>, cache: Arc<Cache>) -> LiveShopDiscord` in `crack-core/src/economy/mod.rs`; `credits::live(ctx)` returns `LiveShopDiscord` via it. All eight sites call it (the sweeper and jail recheck get a built door).

**Exact copy** (description; `(shop: {Label})` uses `ActionKind::label()`; nick escaped with `crack_common::format::escape`):
- Mute(true) `CrackTunes server-muted {t} (shop: Mute)`; Mute(false) `CrackTunes unmuted {t} (shop: Mute)`
- Deaf(true) `CrackTunes server-deafened {t} (shop: Deafen)`; Deaf(false) `CrackTunes undeafened {t} (shop: Deafen)`
- Move(c) `CrackTunes moved {t} to <#c> (shop: Jail)`
- Nick(Some(x)) `CrackTunes changed {t}'s nickname to "x" (shop: Rename)`; Nick(None) `CrackTunes cleared {t}'s nickname (shop: Rename)`
- Titles: `🛒 Shop mute`, `🛒 Shop unmute`, `🛒 Shop deafen`, `🛒 Shop undeafen`, `🛒 Shop move`, `🛒 Shop nickname`. No reason field, no audit footer.

- [ ] **Step 1: Write the failing tests:**
  - `discord.rs`: `logged_reports_each_success_once_and_no_failure` — `Logged::new(FakeEconomyDiscord::default(), rec)` where `rec` records `(u, change, kind)`; call all four setters once OK, then `fail_next` each kind and call again; assert `rec` holds exactly the four successes in order. `member_nick_is_never_reported`.
  - `effects.rs` (db test, beside the existing sweep tests): `apply_and_undo_each_report_once` — a Mute prank applied through `buy`/apply with `Logged<Fake>` then swept after expiry: the recorder holds `[Mute(true), Mute(false)]` with `ActionKind::Mute`.
  - `event.rs`: `shop_change_lines_read_exactly` (every arm above, full description and title), `shop_change_names_no_buyer` (the variant has no buyer; assert the rendered JSON contains only the target's id and, for Move, the channel's), `shop_nick_line_escapes_the_name` (`**x**` → `\*\*x\*\*` as `escape` does). Add one `ShopChange` per distinct title to `all()` and the rest to `pin_cases()`, extending `EXPECTED`.
- [ ] **Step 2: Run and see them fail** (`cargo nextest run -p crack-economy discord effects --features db-tests`, `-p crack-rooms modlog::event`).
- [ ] **Step 3: Implement.** In `live.rs` `events_of`, keep the `SHOP_AUDIT_REASON_PREFIX` skip and change its comment: the shop's changes are now logged by `ShopChangeSink`, without the buyer. Update `LiveEconomyDiscord`'s doc comment.
- [ ] **Step 4: Run the crate tests, the workspace build and both clippy commands; see them pass.** Sabotage `Logged` to report on `Err` once and see the test fail.
- [ ] **Step 5: Commit** ("feat(modlog): the shop's member changes as literal lines, with no buyer").

### Task 3: Move the economy's story out of the mod log

**Files:**
- Modify: `crack-rooms/src/modlog/event.rs` (remove `ShopAction`, `CrowdfundStarted`, `CrowdfundExecuted`; keep `ShopCancelled` for moderators; add `PrankUnmasked`)
- Modify: `crack-common/src/messages.rs` (drop constants only those variants used)
- Modify: `crack-core/src/commands/economy/{admin.rs, shop.rs, bounty.rs, crowdfund.rs}`, `crack-core/src/economy/buttons.rs`

**Interfaces:**
- Consumes: Task 1's `ledger::{EconomyLogEvent, Byline, Ender, Unmasker, post}`.
- Produces:
  - `ModLogEvent::PrankUnmasked { by: UserRef, action: ActionKind, target: UserRef, buyer: UserRef }` — title `🔎 Prank unmasked`, description `{by} unmasked the {label} on {t}: {b}`. (Used by Task 5.)
  - In `admin.rs`: rename `log_to` → `mod_log_to(data, ctx, guild, e: ModLogEvent)`; add `econ_log_to(data: &Data, ctx: &serenity::all::Context, guild: GuildId, e: EconomyLogEvent)` (builds `DiscordTransport::of(ctx)`, calls `ledger::post`), and `both_logs(ctx: Context<'_>, guild, m: ModLogEvent, e: EconomyLogEvent)` which awaits the two posts independently.
  - Pure builders, one per moderator action, each returning `(ModLogEvent, EconomyLogEvent)`: `grant_events(by, target, until, reason)`, `lift_events(by, target)`, `cancel_events(by, target)`, `adjust_events(by, target, amount, reason)`.
  - `shop_event(...)` in `shop.rs` keeps its parameters and returns `Option<EconomyLogEvent>`: Landed → `Purchase { buyer: Byline::new(buyer, signed), target, .. }`, Bounced → `Bounced { buyer: Byline::new(buyer, signed), owner: target }`, Blocked → `None`.
  - `pub fn landed_backers(contributors: &[UserId], signers: &[UserId]) -> Vec<Byline>` in `crowdfund.rs`: one `Byline::new(c, signers.contains(&c))` per contributor, in contributor order.
- Producers switch (economy log only): `run_prank` (shop_event), theme (`Purchase`, Theme), dedicate (`Purchase`, Dedicate, shown), bounty (`Purchase`, Bounty, `until: Some(placed.until)`), cleanse (`PranksEnded { by: Ender::Target }`), crowdfund start (`CrowdfundStarted { starter: Byline::new(starter, signed) }`), crowdfund landed (`CrowdfundLanded { backers: landed_backers(&done.contributors, &done.signers) }`), the paid reveals in `reveal_command` and the Reveal button (`Unmasked { by: Unmasker::Target }`, posted after the private reply).
- Moderator actions (grant, lift, `/economy cancel`, adjust) post **both**: the mod log names the moderator (existing variants, `ShopCancelled { by: moderator }`); the economy log says "a moderator".

- [ ] **Step 1: Write the failing tests:**
  - `admin.rs`: `moderator_actions_name_the_moderator_only_in_the_mod_log` — for each builder: the `ModLogEvent` carries `by == MOD`; `ledger::line(&econ)` starts with `"A moderator "` and does not contain the moderator's id.
  - `shop.rs`: rewrite the `shop_event` tests (lines ~1555–1590): Landed unsigned → `Purchase` whose `buyer.shown() == None`; signed → `Some(buyer)`; Bounced → `Bounced { owner: target }`; Blocked → `None`.
  - `crowdfund.rs`: `landed_bylines_follow_the_signers` — contributors `[A, B, C]`, signers `[A, C]` → shown `[Some(A), None, Some(C)]`; contributors `[A, B]` with signers `[B]` (B pledged once signed, once not: `Executed` lists B once and signs them) → `[None, Some(B)]`, and `ledger::line` of the landed event reads `… backed by <@B> +1 other`.
  - `event.rs`: remove the three variants' tests (`shop_action_renders_buyer_even_unsigned`, `crowdfund_started_names_the_starter`, `crowdfund_executed_renders_every_contributor`) and their `all()`/`pin_cases()`/`EXPECTED` rows; change `ended_pranks_read_the_same_for_a_cleanse_and_a_cancel` to a moderator-only `cancel_names_the_moderator`; add `prank_unmasked_reads_exactly`; update `economy_events_log_their_kind`.
- [ ] **Step 2: Run and see them fail.**
- [ ] **Step 3: Implement.** Replace the doc comments that call "always names the buyer/starter" an owner ruling with the 2026-10-28 rule. Every economy log post comes after the private reply, as the mod log posts do now.
- [ ] **Step 4: Run `cargo nextest run -p crack-rooms -p crack-core -p crack-economy` and clippy; see them pass.** `grep -rn "ShopAction\|CrowdfundExecuted\|CrowdfundStarted {" crack-*/src` returns only `ledger` hits.
- [ ] **Step 5: Commit** ("feat(economy): economy events move to the economy log; moderators' actions go to both").

### Task 4: Unsigned cards never fall back to a labelled follow-up

**Files:**
- Modify: `crack-core/src/commands/economy/{shop.rs:~450, bounty.rs:~128, crowdfund.rs:351}`, `crack-common/src/messages.rs`

**Interfaces:**
- Produces: `pub enum CardFallback { InvokingChannel, TellPrivately }` and `pub fn card_fallback(invoker_named: bool) -> CardFallback` in `shop.rs`; `ECONOMY_CARD_NOT_POSTED = "CrackTunes couldn't post the card where it goes, and posting it here would show it was you, so it was skipped. Your purchase still went through."`
- Rules: `run_prank` (slash and the Rename-back modal, both via `Answer`): `invoker_named = signed`. Bounty: `invoker_named = signed`. Crowdfund `post(...)` gains `invoker_named: bool`: the starter's `signed` when opening; for the fill card, whether the presser whose pledge filled it is in `done.signers`; for the failed card, the invoker's own `signed`. Dedicate/jump cards always name the buyer: unchanged. `TellPrivately` sends `ECONOMY_CARD_NOT_POSTED` through the private channel the path already has (`answer.private`, or `say(ctx, …, true)`).

- [ ] **Step 1: Write the failing test** `an_unsigned_card_never_falls_back_publicly` — `card_fallback(false) == TellPrivately`, `card_fallback(true) == InvokingChannel`; and `not_posted_text_is_exact`.
- [ ] **Step 2: Run, see it fail. Step 3: Implement** at the three sites. **Step 4: Run crate tests + clippy; pass.**
- [ ] **Step 5: Commit** ("fix(economy): an unsigned card is skipped, not posted under the buyer's name").

### Task 5: `/economy reveal`

**Files:**
- Modify: `crack-economy/src/shop.rs` (`mod_reveal`), `crack-core/src/commands/economy/admin.rs` (subcommand + autocomplete), `crack-common/src/messages.rs`

**Interfaces:**
- Consumes: Task 3's `PrankUnmasked`, `both_logs`; Task 1's `Unmasked { by: Unmasker::Moderator }`.
- Produces:
  - `pub struct ModRevealed { pub buyer: UserId, pub kind: ActionKind }`
  - `pub async fn mod_reveal(data: &EconomyData, g: GuildId, target: UserId, action_id: Option<i64>, by: UserId) -> Result<ModRevealed, BuyError>` — `reveal_candidates(pool, g, target, action_id, action_id.is_none(), 1)`; none → `Refused(NothingToReveal)`; `revealed_at.is_some()` → `Refused(AlreadyRevealed)`; then `mark_revealed(&mut conn, prank.id, by)`; `false` → `Refused(AlreadyRevealed)`. No charge, no purchase row, economy on or off (needs only the database: `NoDatabase` otherwise).
  - `pub fn member_option(options: &[CommandDataOption]) -> Option<UserId>` in `admin.rs`: the `member` option's user id from the autocomplete interaction's options, looking inside the `reveal` subcommand's nested options; `None` if absent or not a user id.
  - `pub fn mod_unmasked_text(kind: ActionKind, target: UserId, buyer: UserId) -> String` = `ledger::line(&Unmasked { by: Moderator, .. })` (one copy for the public line and the log).
  - `#[poise::command(slash_command, guild_only, rename = "reveal")] pub async fn reveal_command(ctx, member: User, #[autocomplete = "mod_reveal_autocomplete"] action: Option<i64>)` added to `economy`'s `subcommands(...)`. Gate: `gate_for(ctx, GateNeed::DatabaseOnly)` + `mod_gate`; `defer_ephemeral`.
- Behaviour: private reply `ECONOMY_MOD_REVEAL_DONE` = `"Unmasked: the {label} on {t} was bought by {b}."`; public line `mod_unmasked_text(..)` posted with `courier::post_message` to `ctx.channel_id()` (**not** a follow-up); if that post fails, a second private message `ECONOMY_MOD_REVEAL_NOT_POSTED = "The reveal stands, but CrackTunes couldn't post it in this channel."`; then `both_logs(PrankUnmasked{by: moderator,..}, Unmasked{by: Moderator,..})`. Refusals: `ECONOMY_NOTHING_TO_REVEAL`; already revealed → new `ECONOMY_MOD_ALREADY_REVEALED = "That one is already unmasked."` (the shop's "You've already unmasked that one." is wrong for a moderator — my call). Autocomplete lists `reveal_candidates(pool, g, member, None, true, 10)` labelled with `reveal_label`; no member chosen → no choices.

- [ ] **Step 1: Write the failing tests:**
  - `crack-economy/src/shop.rs` db tests: `mod_reveal_unmasks_for_free` (balance unchanged, no new `economy_actions` row, `revealed_by == MOD`); `mod_reveal_refuses_nothing_and_already` (no candidates → NothingToReveal; second call → AlreadyRevealed); `a_paid_reveal_after_a_mod_reveal_is_refused_and_refunded` (the target's paid `reveal` then returns AlreadyRevealed and their balance is unchanged); `a_crowdfund_is_unmasked_naming_its_starter`.
  - `admin.rs`: `member_option_reads_the_nested_member` (built from serde-deserialized `CommandDataOption` JSON fixtures: present → `Some`, missing → `None`); `mod_unmasked_text_is_exact` (`"A moderator unmasked the rename on <@2>: <@3>"`).
- [ ] **Step 2–4:** fail, implement, pass (crate tests with `--features db-tests`, clippy).
- [ ] **Step 5: Commit** ("feat(economy): /economy reveal, a moderator's free, public unmasking").

### Task 6: The economy log's setting, on the dashboard and in `/economy status`

**Files:**
- Modify: `crack-web/src/settings.rs` (view `log_channel`, `log_findings`; form `log_channel`; `apply_form`), `crack-web/src/page.rs` (picker beside announcements + findings row), `crack-web/assets/settings.js` (`e-log` select, fill, post, findings row), `crack-web/src/lib.rs:~620-650` (pass `log_channel_id` into `bot_perms`; fill `log_findings` via `log_findings`), `crack-web/src/routes.rs` tests
- Modify: `crack-core/src/commands/economy/admin.rs` (`economy_status_text` line + findings in `status_report_text`)
- Modify: `crack-common/src/messages.rs`

**Interfaces:**
- Consumes: Task 1's `log_channel_id`, `NEED_LOG`, `log_findings`.
- Produces: `EconomySettingsView.log_channel: Option<String>`, `.log_findings: Option<Vec<String>>`; `EconomySettingsForm.log_channel: Option<String>` with `#[serde(default)]` (absent = no channel; the page always sends it); `apply_form` resolves it as `ChannelKind::Text` like `announce_channel`. Labels: `DASH_ECONOMY_LOG_CHANNEL = "Economy log"`, `DASH_ECONOMY_LOG_STATUS = "Economy log"`, hint `DASH_ECONOMY_LOG_HINT = "Public by design: it never names an anonymous buyer. Leave empty to keep no economy log."`. `/economy status` gains a line after Gambling: `Economy log: <#id>` or `Economy log: off` (`ECONOMY_STATUS_LOG = "Economy log"`), and its findings are listed like the announcement channel's.

- [ ] **Step 1: Write the failing tests:** extend `economy_status_text_exact` (both forms of the line); in `crack-web/src/settings.rs` `apply_form_sets_and_clears_the_log_channel` and `an_unknown_log_channel_is_refused`; in `routes.rs` a page test asserting `id="e-log"` follows `id="e-announce"` and the findings row renders with `data-kind="log"`; update `ECONOMY_FORM` fixtures only where the test is about the log channel.
- [ ] **Step 2–4:** fail, implement, pass (`cargo nextest run -p crack-web -p crack-core`, clippy).
- [ ] **Step 5: Commit** ("feat(dashboard): pick the economy log channel; /economy status shows it").

### Task 7: Room mute holds — storage, the decision, and `settle`

**Files:**
- Create: `migrations/20261028120000_room_mute_holds.sql` and an identical `crack-core/test_migrations/20261028120000_room_mute_holds.sql`
- Create: `crack-rooms/src/db/mute_holds.rs` (+ `pub mod mute_holds;` in `crack-rooms/src/db/mod.rs`)
- Create: `crack-rooms/src/rooms/mute_hold.rs` (+ `pub mod mute_hold;` in `rooms/mod.rs`)
- Modify: `crack-rooms/src/rooms/registry.rs` (`pub holds: HoldIndex` on `RoomRegistry`)
- Modify: `crack-rooms/src/rooms/discord.rs` + `test_support.rs` (two calls), `crack-rooms/src/modlog/live.rs` (`events_of` skip)
- Regenerate: `.sqlx/` via `scripts/sqlx_sync.py` (its header has the exact commands; never `cargo sqlx prepare`)

**Interfaces:**
- Migration:
  ```sql
  -- A server mute CrackTunes put on a member because a room muted them, kept
  -- until it is undone: it outlives the room's row, which goes when the room empties.
  CREATE TABLE IF NOT EXISTS room_mute_holds (
      guild_id   BIGINT NOT NULL,
      user_id    BIGINT NOT NULL,
      channel_id BIGINT NOT NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
      PRIMARY KEY (guild_id, user_id)
  );
  ```
- `db::mute_holds`: `held_for(pool, g, u) -> sqlx::Result<Option<ChannelId>>`; `hold(pool, g, u, ch) -> sqlx::Result<()>` (upsert, sets `channel_id`); `release(pool, g, u) -> sqlx::Result<bool>`; `holds_in(pool, g) -> sqlx::Result<Vec<(UserId, ChannelId)>>`.
- `RoomsDiscord` gains:
  - `async fn set_mute(&self, guild: GuildId, user: UserId, on: bool) -> Result<(), DiscordError>` — live: `EditMember::new().mute(on).audit_log_reason(ROOM_MUTE_AUDIT_REASON)`.
  - `async fn voice_now(&self, guild: GuildId, user: UserId) -> Result<VoiceNow, DiscordError>` — live: `http.get_user_voice_state(guild, user)` → `VoiceNow { channel: vs.channel_id, server_muted: vs.mute }`; `JsonErrorCode::UnknownVoiceState` (10065) → `Ok(VoiceNow::default())`.
  - Fake: `Call::SetMute(user, on)`, `Call::VoiceNow(user)`, `CallKind::{SetMute, VoiceNow}`, and `set_voice(u: UserId, v: VoiceNow)` scripting what `voice_now` answers (unset → `VoiceNow::default()`).
- `discord.rs`: `pub const ROOM_AUDIT_REASON_PREFIX: &str = "CrackTunes room";` `pub const ROOM_MUTE_AUDIT_REASON: &str = "CrackTunes room: mute";`. `events_of` also skips CrackTunes-authored entries whose reason starts with `ROOM_AUDIT_REASON_PREFIX` (the room's `RoomMute` line is the record).
- `mute_hold.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub struct VoiceNow { pub channel: Option<ChannelId>, pub server_muted: bool }`
  - `pub enum Cause { Arrived(ChannelId), MutedInRoom(ChannelId), Other }` — only the first two may take a new hold, and only in that channel.
  - `pub enum HoldStep { Nothing, Mute(ChannelId), Unmute, Retarget(ChannelId), Drop }`
  - `pub fn hold_step(held_for: Option<ChannelId>, now: VoiceNow, mutes: impl Fn(ChannelId) -> bool, may_take_in: Option<ChannelId>) -> HoldStep` — pure, the whole rule:
    ```
    held_for = Some(r):
      now.channel = None                      -> Unmute
      now.channel = Some(c), c != r:
        server_muted && mutes(c)              -> Retarget(c)
        !server_muted                         -> Drop
        otherwise                             -> Unmute
      now.channel = Some(r):
        !server_muted                         -> Drop        (someone else unmuted them: moderators win)
        !mutes(r)                             -> Unmute      (the room let them speak; an earlier unmute failed)
        otherwise                             -> Nothing
    held_for = None:
      may_take_in = Some(c), now.channel = Some(c), mutes(c), !server_muted -> Mute(c)
      otherwise                               -> Nothing     (someone else's mute stays theirs)
    ```
  - `pub enum Settled { Unchanged, Muted, MuteRefused, Unmuted, StillOwed, Retargeted, Dropped, Failed }`
  - `pub async fn settle(data: &RoomsData, discord: &dyn RoomsDiscord, g: GuildId, u: UserId, cause: Cause) -> Settled` — no pool → `Unchanged`. Under `data.rooms.holds.lock(g).await`: read `held_for`; `mutes(c)` = the live room `c` of guild `g` (lock it briefly) has `u` in `state.muted`. If there is no hold and the cause can't take one (or that room doesn't mute `u`) → `Unchanged` **with no Discord call**. Else `voice_now` (an error → `Failed`), `hold_step`, and act:
    - `Mute(c)`: `hold` the row first (fail → `Failed`, no Discord call), then `set_mute(true)`: Ok → `Muted`; `MissingPermissions` → `release`, `MuteRefused`; `NotConnected` → `release`, `Unchanged`; other → `release`, `Failed`.
    - `Unmute`: `set_mute(false)`: Ok → `release`, `Unmuted`; `NotConnected` → keep the row, `StillOwed`; other → keep, `Failed` (`tracing::warn!`; retried at the next event or reconcile, never looped).
    - `Retarget(c)`: `hold(.., c)` → `Retargeted`. `Drop`: `release` → `Dropped`.
    - Keep `holds` (the index) in step with every `hold`/`release`.
  - `#[derive(Debug, Default)] pub struct HoldIndex` — `held: DashMap<(GuildId, UserId), ChannelId>` (a filter; the table is the truth) and `locks: DashMap<GuildId, Arc<tokio::sync::Mutex<()>>>`; `may_hold(g, u) -> bool`, `set(g, u, ch)`, `clear(g, u)`, `async fn lock(g) -> tokio::sync::OwnedMutexGuard<()>`.
  - Lock order: the guild hold lock, then a room lock, never the reverse. `settle` must never be called while a room lock is held.

- [ ] **Step 1: Write the failing tests:**
  - `hold_step_covers_every_exit` — one row per line of the rule table, plus: `in_place_update_never_takes_a_hold` (`held_for=None`, in R, `mutes(R)`, not server-muted, `may_take_in=None` → `Nothing`); `someone_elses_mute_is_never_taken` (server-muted already → `Nothing` even with `may_take_in=Some(R)`).
  - db tests with `FakeRoomsDiscord` and a live room in the registry whose state mutes `T`: `muting_in_the_room_takes_a_hold_and_mutes` (calls `[VoiceNow(T), SetMute(T,true)]`, row = R); `moving_out_unmutes_and_releases`; `disconnecting_keeps_the_debt` (`SetMute(T,false)` fails `NotConnected` → row kept, `StillOwed`; then joining elsewhere → `Unmuted`, row gone); `a_moderator_unmute_drops_the_hold_without_a_call`; `no_mute_members_falls_back` (`MissingPermissions` → `MuteRefused`, no row); `no_hold_and_no_cause_makes_no_call` (fake records no calls); `a_failed_unmute_is_kept_for_retry`.
  - `discord.rs`: `discord_error_maps_…` unchanged; fake test covers the two new calls. `live.rs`: `events_of_skips_the_rooms_own_mutes` (bot actor, reason `CrackTunes room: mute` → no events; a moderator's mute still logged).
- [ ] **Step 2: Run and see them fail.**
- [ ] **Step 3: Implement**, then regenerate `.sqlx` with the `scripts/sqlx_sync.py` recipe against the local `pg` (migrated) and add only the new/changed `.sqlx/query-*.json` files.
- [ ] **Step 4: Run `cargo nextest run -p crack-rooms --features db-tests` and clippy; pass.** Sabotage `hold_step`'s `Drop` arm to `Unmute` and see a test fail.
- [ ] **Step 5: Commit** ("feat(rooms): mute holds — a room's server mute, recorded until it is undone").

### Task 8: Room mute wiring — the buttons, voice events, reconcile and preflight

**Files:**
- Modify: `crack-rooms/src/rooms/ops.rs` (`act`), `crack-rooms/src/rooms/card.rs` (`RoomDone::MutedOnRejoin`), `crack-rooms/src/rooms/lifecycle.rs` (`on_voice_state`, `reconcile_with`), `crack-rooms/src/rooms/preflight.rs` (`OPTIONAL`), `crack-common/src/messages.rs`
- Modify (if it lists optional findings by name): `crack-web` rooms preflight rendering + its test

**Interfaces:**
- Consumes: Task 7's `settle`, `Cause`, `Settled`, `HoldIndex`, `RoomsDiscord::{set_mute, voice_now}`.
- Produces:
  - `act()`: after `drop(live)` and before the mod log post, `RoomAction::Mute { target, .. }` → `settle(cx.data, cx.discord, guild, target, Cause::MutedInRoom(room))`; `RoomAction::Unmute { target }` → `settle(.., Cause::Other)`. `Settled::MuteRefused` turns the reply into `RoomDone::MutedOnRejoin(target.to_ref())`, text `🔇 Muted <@7>. CrackTunes needs Mute Members to silence them now, so it takes effect when they rejoin.` (`ROOM_DONE_MUTED_ON_REJOIN` holds the sentence after "Muted <@7>."). The mod log line is unchanged (`RoomMute`).
  - `pub fn on_voice_state(data: &RoomsData, ctx: &Context, old: Option<&VoiceState>, new: &VoiceState)` in `mute_hold.rs` (synchronous; spawns): skip bots (`lifecycle::is_bot`, made `pub(crate)`) and events without a guild; `cause` = `Arrived(c)` when the channel changed to `Some(c)`, else `Other`; spawn `settle` with `Sweep::live(ctx).discord` only if `data.rooms.holds.may_hold(g, u)` or (`Arrived(c)` and `data.rooms.get(c)` is some). Called first in `lifecycle::on_voice_state`, **before** its `classify` early return (a mute in place must reach it).
  - `pub async fn reconcile(data: &RoomsData, discord: &dyn RoomsDiscord, g: GuildId)` in `mute_hold.rs`: `holds_in` → `holds.set` each → `settle(.., Cause::Other)` each. Called in `reconcile_with` **after** `reconcile(...)` has reloaded the rooms (a hold settled before its room is back would be unmuted wrongly), spawned so it holds up nothing.
  - `preflight::OPTIONAL = PIN_MESSAGES | MUTE_MEMBERS`.

- [ ] **Step 1: Write the failing tests:**
  - `ops.rs` (db tests, the existing world): `mute_in_the_room_server_mutes_them` (calls include `SetMute(TARGET, true)` after the Speak-deny `Put`; reply `Muted`); `mute_without_mute_members_says_rejoin` (fake fails `SetMute` with `MissingPermissions` → `RoomDone::MutedOnRejoin`, Speak deny still written); `unmute_lifts_the_hold` (row gone, `SetMute(TARGET,false)`); `mute_of_someone_outside_only_denies_speak` (no `SetMute`).
  - `card.rs`: add `done_muted_on_rejoin` to the pinned cases with the exact text.
  - `mute_hold.rs`: `reconcile_settles_after_rooms_are_back` — a held member found in their still-muting room by reconcile is left muted (no `SetMute`); one found outside is unmuted.
  - `preflight.rs`: `mute_members_is_optional` (missing it yields `Finding::Optional` with `MUTE_MEMBERS`, never `Missing`).
- [ ] **Step 2–4:** fail, implement, pass (`cargo nextest run -p crack-rooms -p crack-core -p crack-web --features …db-tests`, clippy). Sabotage the reconcile order (settle before rooms reload) and see `reconcile_settles_after_rooms_are_back` fail.
- [ ] **Step 5: Commit** ("feat(rooms): the Mute button server-mutes in the room and lets go when they leave").

### Task 9: Docs, notes on the old specs, and the v0.46.0 bump

**Files:**
- Modify: `docs/economy.md` (the economy log and its upgrade note; the anonymity rule; `/economy reveal`; unsigned cards), `docs/rooms-and-mod-log.md` (mute is a server mute while in the room — fix the promise near line 214; the mod log's literal shop lines; economy events moved out; Mute Members optional), `docs/web-dashboard.md` (the Economy log picker, if it lists economy settings)
- Modify: `docs/superpowers/specs/2026-10-20-economy-design.md` (dated note at the "Accountability" row and §5.4) and `docs/superpowers/specs/2026-10-16-voice-rooms-and-mod-log-design.md` (dated note at the mute row and §5's table): "Superseded 2026-10-28 by `2026-10-28-economy-log-and-room-mute-design.md`."
- Modify: `Cargo.toml` `version = "0.46.0"`, `Cargo.lock` (via `cargo update -w` under the memory cap), `CHANGELOG.md` (a v0.46.0 entry in the file's style)

- [ ] **Step 1:** Write the docs. Upgrade note in plain words: after upgrading, economy lines stop going to the mod log; pick a public economy log channel on the dashboard, and point the mod log at a moderators-only channel.
- [ ] **Step 2:** Bump the version; run the full workspace suite with the CI feature list from `docs/testing.md` and both clippy commands; all pass.
- [ ] **Step 3: Commit** ("v0.46.0: economy log, hidden buyers, /economy reveal, and a room mute that works").

## TuneTitan hand test (after merge, from the spec)

Mute mid-speech · move out · disconnect then rejoin elsewhere · rejoin the room · a moderator unmutes them in the room · the Unmute button · the room with Mute Members removed · an unsigned rename in a server whose economy log is set (check the economy log, the mod log and the card) · `/economy reveal`.
