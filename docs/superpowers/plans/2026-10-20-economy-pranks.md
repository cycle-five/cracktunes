# Economy Pranks (PR 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the prank shop. Members buy actions against other members: rename, lock, clown, drag, mute,
deafen and jail. The targets defend with mirror, cleanse and immunity, and unmask with reveal. Every
prank posts a public card and a mod log entry naming the buyer, and undoes itself on time, across
restarts. Ships as **v0.33.0**, on top of v0.32.0 (#617).

**Architecture:** this extends PR 1's `crack-core/src/economy/`.
- **One Discord seam.** `trait EconomyDiscord`, with a recording fake, holds every member change.
- **Pure decisions.** Target checks, preflight, the lock's executor rule, undo plans and card text
  are pure functions over plain snapshot structs.
- **`economy_actions` (PR 1's table) is the source of truth.** A 15 s sweeper, a reconcile at
  `GuildCreate` and the voice/audit routes finish every effect.
- **The shop's audit reason is the marker.** Every member change the shop makes carries the audit
  reason prefix `CrackTunes shop`. The mod log's audit path skips CrackTunes-authored entries with
  that prefix, and the shop posts its own exact `ShopAction` naming the buyer.

**Tech Stack:** Rust 2021, serenity `next` (`EditMember::nickname/mute/deafen/voice_channel`,
`audit_log_reason`), poise (modals and components as rooms uses them), sqlx 0.9/Postgres, chrono,
axum (crack-web).

**Spec:** `docs/superpowers/specs/2026-10-20-economy-design.md`. This plan covers 4.1, 4.2, 4.3, 5
(cards, buttons and the shop's mod log events), 7 and 8, and the PR 2 parts of 1, 3, 6 and 13.
**Read the spec before any task**, especially section 3 (the purchase steps), 4.1's notes,
6.4 and 8.

## Global Constraints

- **Everything from PR 1's constraints still holds.** That covers off by default, the money rules,
  the repo rules, the local postgres, the gate and the shell notes. Copy at
  `.superpowers/sdd/2026-10-20-economy-pranks/constraints.md`.
- **New `EconomyConfig` fields** (serde default, so PR 1 rows load):
  - `rename_hours` 24 (1..=168);
  - `short_effect_minutes` 5 (1..=60);
  - `jail_channel_id: Option<ChannelId>` (none);
  - `announce_channel_id: Option<ChannelId>` (none).
- **New `ActionKind` variants and default prices** (spec 4): rename 120, lock 60, clown 30,
  drag 60, mute 90, deafen 90, jail 150, mirror 150, cleanse 80, reveal 100. All are on by default.
  Jail is sold only with a jail channel; lock only with View Audit Log (preflight).
- **Note and signing:** `note` is 1..=200 chars after trim, with no mentions (`<@`, `<#`, `<@&`,
  `@everyone`, `@here`) and no links (`http://`, `https://`). `signed` defaults to false. A
  nickname is 1..=32 chars after trim, under the same mention and link rule.
- **The audit reason on every shop member change is `CrackTunes shop: {kind}` exactly.** The mod
  log's audit path skips an entry when its executor is CrackTunes and its reason starts with
  `CrackTunes shop`.
- **Never undo a moderator.** For every timed effect whose author the bot can read (nick, mute,
  deaf), a change by anyone other than the target or CrackTunes ends the row with
  `end_reason = 'overridden'`, and the bot restores nothing for it.
- **Restore only what the bot changed.** The name to restore is captured when the first name prank
  starts on a target (`None` = no nickname). Mute and deafen undo sets the value back to false.
- **Undo never fails silently.**
  - A member not in voice (Discord 40032) leaves a mute or deafen row `ending`, finished on their
    next voice join.
  - Unknown Member (10007) ends the row with `left`.
  - Missing permissions are logged at warn and retried by the next sweep. After 1 hour it ends with
    `undo_failed`, logged at error.
- **PR 2 must-dos from PR 1's review:**
  - (a) Reconcile resolves `pending` rows. Immunity: refund. Others: landed → active, else failed
    and refunded.
  - (b) A supervisor logs at error when the earning loop or the sweeper ends or panics.
  - (c) An opt-out and a mod grant end running pranks on the member, undone, at the two
    `// PR 2:` markers.
  - (d) Remove the "pranks arrive in the next update" copy.

## Review Focus

1. **A bot restart while pranks run**, including one whose target is offline. Every row still ends
   on time, or at the target's next join. Pinned by Task 5's `reconcile_finishes_overdue_rows` and
   `mute_undo_for_an_offline_member_lands_on_their_next_join`.
2. **A moderator acts during a prank** (renames, mutes or unmutes the target). The mod's change
   stands, and the row ends `overridden`. Pinned by Task 6's `a_mods_rename_ends_the_lock_and_is_kept`
   and `a_mods_unmute_ends_the_mute_row`.
3. **A purchase that can't land** (an immune target, a target above the bot, the target not in
   voice for drag or mute, a missing permission, the same prank already running). Refused before
   any charge. Pinned by Task 3's `check_target_order` and Task 4's `refused_purchases_charge_nothing`.
4. **Drag into a channel the target's roles keep them out of.** Refused (never-widen). Pinned by
   Task 3's `drag_refuses_a_channel_the_target_cannot_join`.
5. **Mirror.** The attacker is charged, and the prank lands on them with their own parameters; if it
   can't land on them, it's blocked. Pinned by Task 4's `a_mirror_bounces_the_rename_onto_the_buyer`
   and `a_mirror_blocks_what_cannot_land_on_the_buyer_and_still_charges`.

---

### Task 1: Catalogue and settings for pranks

**Files:** `crack-core/src/economy/settings.rs`, `crack-core/src/economy/wallet.rs`
(`ActionPayload`), `crack-core/src/messaging/messages.rs`, and `crack-core/src/commands/economy/shop.rs`
(remove the "next update" copy, must-do d).

**Interfaces:**
- `ActionKind` gains `Rename, Lock, Clown, Drag, Mute, Deafen, Jail, Mirror, Cleanse, Reveal`,
  with `key()`s the snake_case names, labels in messages.rs, defaults from Global Constraints,
  and `ALL` in that order after `Immunity`. Add `fn is_prank(self) -> bool` (true for the seven
  4.1 kinds) and `fn is_timed(self) -> bool` (rename, lock, clown, mute, deafen, jail).
- `EconomyConfig` gains the four fields with defaults and bounds (`EconomySettingsError::BadRenameHours`
  and `BadShortEffect`).
- `ActionPayload` gains these variants (serde `tag = "kind"`):
  - `Rename { name: String, restore: Option<String> }`;
  - `Clown { applied: String, restore: Option<String> }`;
  - `Lock { locked: Option<String> }`;
  - `Drag { from: ChannelId, to: ChannelId }`;
  - `Mute`, `Deafen`;
  - `Jail { jail: ChannelId }`;
  - `Mirror`, `Cleanse`;
  - `Reveal { action_id: i64 }`.
- Pure: `fn validate_note(&str) -> Result<String, NoteError>` and
  `fn validate_nick(&str) -> Result<String, NickError>`, returning the trimmed string. Each enum
  has `TooLong`, `Empty`, `Mention` and `Link`.
- Pure: `fn clown_name(current: &str) -> String`, which appends " 🤡", trimming `current` so the
  result is at most 32 chars (char count, not bytes).

- [ ] **Step 1: Failing tests.**
  - `prank_defaults_match_the_spec`;
  - `new_fields_default_and_bound` (both edges of each new bound);
  - `a_pr1_config_loads_with_pr2_defaults`: the PR 1 JSON without the new fields loads with the
    defaults;
  - `payload_round_trips_every_variant`;
  - `validate_note_and_nick_edges`: 200 vs 201 chars, 32 vs 33, `<@1>`, `@everyone`, `https://x`,
    blank;
  - `clown_name_caps_at_32_chars`: a 32-char name gives 29 chars + " 🤡" (32 total); a short name
    just gets the suffix;
  - `menu_text` tests updated for the new items and with the "next update" copy gone.
- [ ] **Step 2: Run; fail.** **Step 3: Implement.** **Step 4: Gate.**
- [ ] **Step 5: Commit**: `feat(economy): the prank catalogue and settings`.

### Task 2: The Discord seam and the preflight

**Files:** create `crack-core/src/economy/discord.rs` and `crack-core/src/economy/preflight.rs`.

**Interfaces:**
- `#[async_trait] trait EconomyDiscord: Send + Sync` with:
  - `set_nick(&self, g: GuildId, u: UserId, nick: Option<&str>, kind: ActionKind) -> Result<(), EconDiscordError>`;
  - `set_mute(&self, g, u, on: bool, kind)`;
  - `set_deaf(&self, g, u, on: bool, kind)`;
  - `move_member(&self, g, u, to: ChannelId, kind)`.

  Every call sends the audit reason `format!("CrackTunes shop: {}", kind.key())`. The real impl is
  over `Arc<Http>`. `FakeEconomyDiscord` records every call as a typed enum, and can be told to fail
  a given call with a given error.
- `enum EconDiscordError { MissingPermissions, NotConnected, UnknownMember, Other(String) }`,
  mapped from serenity errors as `rooms::discord::DiscordError` maps them (codes 50013, 40032,
  10007).
- `pub const SHOP_AUDIT_REASON_PREFIX: &str = "CrackTunes shop";`
- Preflight:
  - `struct BotPerms { guild: Permissions, in_channel: HashMap<ChannelId, Permissions> }`;
  - `fn needs(kind: ActionKind) -> Permissions` (guild-level, from spec 7's table);
  - `fn missing(kind, perms: &BotPerms, channels: &[ChannelId]) -> Vec<(Option<ChannelId>, Permissions)>`.
    It checks the guild-level needs and, for each channel given, View Channel + Connect for
    drag and jail. Empty means OK;
  - `fn lock_needs_audit_log(perms: &BotPerms) -> bool`;
  - thin glue `fn bot_perms(cache, guild, channels: &[ChannelId]) -> Option<BotPerms>`.

- [ ] **Step 1: Failing tests.**
  - `every_call_sends_the_shop_audit_reason`: this needs a seam for the reason. Make
    `fn audit_reason(kind) -> String` pure, test it, and have the real impl use it;
  - `the_fake_records_calls_and_fails_on_demand`;
  - `error_mapping_matches_rooms_codes`;
  - `needs_per_kind` (each kind's exact permission bits);
  - `missing_reports_guild_and_channel_gaps`;
  - `lock_needs_view_audit_log`.
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(economy): the Discord seam and the preflight`.

### Task 3: Who can be pranked (pure)

**Files:** `crack-core/src/economy/shop.rs` (extend), and `crack-core/src/economy/target.rs` (new).

**Interfaces:**
- `struct TargetFacts` with:
  - `is_bot: bool`;
  - `is_self: bool`;
  - `is_owner: bool`;
  - `top_role_position: u16`;
  - `bot_top_role_position: u16`;
  - `in_voice: Option<ChannelId>`;
  - `opted_out: bool`;
  - `protection: Option<Protection>`;
  - `has_mirror: bool`;
  - `running_same_kind: Option<DateTime<Utc>>` (when it ends);
  - `can_join_destination: Option<bool>` (drag/jail destination; `None` = not applicable);
  - `current_nick: Option<String>` (what a name prank restores).
- `Refusal` gains:
  - `TargetIsBot`;
  - `TargetIsSelf`;
  - `OutOfReach`;
  - `TargetNotInVoice`;
  - `BuyerNotInVoice`;
  - `TargetOptedOut`;
  - `TargetImmune(Protection)`;
  - `AlreadyRunning { until: DateTime<Utc> }`;
  - `CannotJoinDestination`;
  - `NoJailChannel`;
  - `Unavailable { missing: Permissions }` (preflight).
- `fn check_target(kind: ActionKind, t: &TargetFacts, buyer_in_voice: Option<ChannelId>) -> Result<TargetOutcome, Refusal>`.
  `enum TargetOutcome { Lands, Mirrored }` (has_mirror → `Mirrored`). It checks in this order:
  1. bot;
  2. self (for the 4.1 kinds);
  3. out of reach (`is_owner`, or `top_role_position >= bot_top_role_position`, for nick, mute,
     deaf, move kinds);
  4. voice needs: drag needs both in voice; mute, deafen and jail need the target in voice;
  5. opted out;
  6. mirror, which returns `Mirrored` (checked before immunity, spec 4.2);
  7. immunity;
  8. running same kind (rename and clown share a slot);
  9. destination join (never-widen; drag only; jail exempt, spec ruling Q4).
- Never-widen glue: `fn can_join(cache, guild, member: UserId, channel: ChannelId) -> Option<bool>`.
  It builds a `rooms::guard::BaselineView` from the channel's overwrites and the guild roles, and
  calls `member_can_join`.
- Reachability glue: `fn top_position(cache, guild, member) -> u16` (the highest role position; 0
  for none).

- [ ] **Step 1: Failing tests.**
  - `check_target_order`: start from facts where everything is wrong, clear one at a time, and
    assert each refusal is the first that applies;
  - `out_of_reach_covers_owner_and_equal_or_higher_roles` (equal positions → out of reach);
  - `mirror_is_checked_before_immunity`;
  - `rename_and_clown_share_a_slot`;
  - `drag_refuses_a_channel_the_target_cannot_join`;
  - `jail_ignores_never_widen`;
  - `voice_requirements_per_kind`.
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(economy): who can be pranked`.

### Task 4: Buying a prank (charge → act → card → mod log)

**Files:** `crack-core/src/economy/shop.rs`, `crack-core/src/economy/cards.rs` (new),
`crack-core/src/modlog/event.rs`, `crack-core/src/modlog/live.rs` (the skip rule), and
`crack-core/src/db/economy.rs`.

**Interfaces:**
- `ModLogEvent` gains:
  - `ShopAction { action: ActionKind, buyer: UserId, target: UserId, note: Option<String>, signed: bool, until: Option<DateTime<Utc>>, mirrored_from: Option<UserId> }`;
  - `ShopCancelled { by: UserId, target: UserId }`.

  Both render with the buyer always named (owner ruling). `mirrored_from` reads "(bounced by
  @target's mirror)".
- `modlog::live::events_of` skips an entry whose actor is the bot and whose reason starts with
  `SHOP_AUDIT_REASON_PREFIX`. Keep the existing behaviour for every other bot-authored entry.
- DB:
  - `active_of_kind(&PgPool, GuildId, target: UserId, kinds: &[ActionKind]) -> sqlx::Result<Vec<ActionRow>>`;
  - `struct ActionRow { id, kind, buyer, target, payload: ActionPayload, state, expires_at, note, signed, revealed_at }`;
  - `activate_action(&PgPool, id, expires_at: Option<DateTime<Utc>>) -> sqlx::Result<bool>`, which
    moves `pending` → `active` (timed) or `done` (instant), guarded by `state = 'pending'`;
  - `consume_mirror(&PgPool, GuildId, owner: UserId) -> sqlx::Result<bool>`, which moves an active
    Mirror row to `ended`, `end_reason = 'fired'`, atomically.
- `struct BuyRequest { guild, buyer, kind, target: Option<UserId>, nick: Option<String>, note: Option<String>, signed: bool, now }`.
- `async fn buy(data: &Data, discord: &dyn EconomyDiscord, facts: TargetFacts, buyer_voice: Option<ChannelId>, req: BuyRequest) -> Result<Bought, BuyError>`. It runs:
  1. the PR 1 purchase check, then `check_target`;
  2. `charge_for_action` (payload per kind; a first name prank's `restore` is
     `facts.current_nick`, and a later one keeps the running prank's `restore`);
  3. on `Mirrored`: `consume_mirror`, then retarget the action onto the buyer with the same
     parameters. If it can't land on the buyer (protection, out of reach, or not in voice where
     needed), the row goes `done` with `end_reason = 'blocked'` and **no refund** (spec 4.2);
  4. act through the seam; on error, `refund_action` and `BuyError::Discord(EconDiscordError)`;
  5. `activate_action` with `expires_at` (rename, clown and lock: `rename_hours`; mute, deafen and
     jail: `short_effect_minutes`; a lock with a running name prank: that prank's end);
  6. return `Bought { action_id, balance, outcome: Landed | Bounced | Blocked, until }`.

  The caller posts the card and the mod log event.
- `cards.rs`, pure: `fn card_text(kind, target: UserId, nick: Option<&str>, until: Option<DateTime<Utc>>, note: Option<&str>, buyer: Option<UserId> /* None = "someone" */, outcome) -> String`
  from messages.rs constants. Plus
  `fn card_destination(kind, target_voice: Option<ChannelId>, invoked_in: ChannelId, announce: Option<ChannelId>) -> ChannelId`:
  - drag, jail, mute and deafen post in the target's voice channel chat;
  - everything else goes to `announce`, else `invoked_in`.

- [ ] **Step 1: Failing tests.**
  - db-tests (with `FakeEconomyDiscord`):
    - `a_rename_charges_sets_the_nick_and_activates`;
    - `refused_purchases_charge_nothing`: each `check_target` refusal leaves the balance and the
      ledger unchanged and writes no action row;
    - `a_discord_failure_refunds`;
    - `a_mirror_bounces_the_rename_onto_the_buyer`: the seam saw `set_nick(buyer, their chosen
      name)`, and the mirror row ended `fired`;
    - `a_mirror_blocks_what_cannot_land_on_the_buyer_and_still_charges`;
    - `a_lock_ends_with_the_running_rename`;
  - pure:
    - `card_text_signed_and_anonymous`;
    - `card_destination_per_kind`;
    - `shop_action_renders_buyer_even_unsigned`;
    - `events_of_skips_the_shops_own_mute`, next to the modlog tests, with a bot-authored entry
      with reason `CrackTunes shop: mute` → no event; the same entry with another reason is still
      logged.
- [ ] **Step 2-4.** Regenerate `.sqlx`. **Step 5: Commit**: `feat(economy): buying a prank`.

### Task 5: Timed effects (sweeper, undo, reconcile, supervisor)

**Files:** `crack-core/src/economy/effects.rs` (new), `crack-core/src/db/economy.rs`,
`crack-core/src/economy/earn.rs` (supervised start), and `crack-core/src/handlers/serenity.rs`
(`GuildCreate`, `VoiceStateUpdate`, `Ready`).

**Interfaces:**
- Pure `fn undo_plan(row: &ActionRow, now) -> UndoPlan`, where `UndoPlan` is one of:
  - `SetNick(Option<String>)`;
  - `SetMute(false)`;
  - `SetDeaf(false)`;
  - `Nothing` (drag, jail, mirror, lock: a lock ends by itself; reveal).
- DB:
  - `due_actions(&PgPool, now, limit: i64) -> sqlx::Result<Vec<(GuildId, ActionRow)>>`: across all
    guilds, active rows with `expires_at <= now`;
  - `ending_for(&PgPool, GuildId, UserId)`;
  - `end_action(&PgPool, id, state: ActionState /* Ended | Ending */, end_reason: &str) -> sqlx::Result<bool>`;
  - `pending_for_guild(&PgPool, GuildId)`;
  - `overdue_for_guild(&PgPool, GuildId, now)`.
- `async fn finish(data, discord, guild, row, reason: &str, now)` applies the undo plan.
  - `NotConnected` → `Ending`.
  - `UnknownMember` → `Ended("left")`.
  - `MissingPermissions` → keep `active`, warn; once `now - expires_at > 1h`, `Ended("undo_failed")`
    and error.
  - Success → `Ended(reason)`.
- `async fn sweep(data, discord, now)` finishes every due row with reason `expired`. It runs for
  guilds with the economy off too (spec 14).
- `async fn on_voice_join(data, discord, guild, user, now)` finishes that member's `ending` rows.
  It's called from `VoiceStateUpdate` when the member joined voice (old channel none → some),
  before rooms' handling.
- `async fn reconcile(data, discord, cache, guild, now)`, called at `GuildCreate` after the economy
  settings load:
  - pending rows (must-do a): immunity → `refund_action`. Others → "landed?" from the cached member:
    the nick equals the payload's name, mute/deaf set, in the jail channel. Landed → `activate_action`;
    not landed → `refund_action`;
  - overdue rows are finished;
  - `ending` rows whose member is in voice are finished.
- Supervisor (must-do b): `fn spawn_supervised(name: &'static str, fut)` spawns the future and a
  watcher that logs at **error** when it returns or panics (inspect the `JoinHandle`). Use it for the
  earn loop and the new 15 s sweeper loop (`start_sweeper(data, http)`, started once from `Ready`
  next to `earn::start`).

- [ ] **Step 1: Failing tests** (fake seam + db-tests):
  - `undo_plan_per_kind`;
  - `sweep_restores_the_old_nickname`;
  - `sweep_runs_with_the_economy_off`;
  - `mute_undo_for_an_offline_member_lands_on_their_next_join`;
  - `unknown_member_ends_as_left`;
  - `missing_permissions_retries_then_gives_up_after_an_hour`;
  - `reconcile_finishes_overdue_rows`;
  - `reconcile_refunds_a_pending_immunity_row`;
  - `reconcile_activates_a_landed_pending_rename_and_refunds_one_that_did_not_land`;
  - `a_supervised_task_that_ends_is_logged`: assert through a test hook. Make the logger an
    injectable `Fn(&str)` in the supervisor's test-only constructor.
- [ ] **Step 2-4.** Regenerate `.sqlx`. **Step 5: Commit**: `feat(economy): pranks end on time`.

### Task 6: What the audit log and voice say (lock, mod overrides, jail)

**Files:** `crack-core/src/economy/lock.rs` (new), `crack-core/src/economy/effects.rs`, and
`crack-core/src/handlers/serenity.rs` (the `GuildAuditLogEntryCreate` and `VoiceStateUpdate`
arms).

**Interfaces:**
- Pure `fn nick_change_rule(executor: UserId, target: UserId, bot: UserId) -> NickRule` returns
  `Revert`, `Ignore` or `Overridden`:
  - the executor is the target → `Revert` (only while a lock runs);
  - the bot → `Ignore`;
  - anyone else → `Overridden`.
- `async fn on_audit_entry(data, discord, guild, entry, bot, now)`. For `MEMBER_UPDATE` entries it
  reads `nick`, `mute` and `deaf` changes:
  - **nick:** `Revert` re-sets the locked nick through the seam. `Overridden` ends the target's
    active lock and name prank rows `overridden`, with no restore.
  - **mute/deaf:** a change by anyone other than the bot ends the target's active Mute/Deafen row
    `overridden`.
  - It is called from the audit arm **whether or not the mod log is on** (spec 14).
- `async fn on_voice_move(data, discord, guild, user, to: Option<ChannelId>, now)`: a member with
  an active Jail row who moved to a channel other than the jail (`to` some and not the jail) is
  moved back, at most once per 10 s per member (in-memory `DashMap<(GuildId, UserId), Instant>` on
  the registry). A disconnect (`to` none) is allowed.

- [ ] **Step 1: Failing tests:**
  - `nick_change_rule_table`;
  - `a_self_rename_under_lock_is_reverted`;
  - `a_mods_rename_ends_the_lock_and_is_kept`: the row ends `overridden`, and the sweeper later
    restores nothing;
  - `a_mods_unmute_ends_the_mute_row`;
  - `the_shops_own_nick_change_is_ignored`;
  - `jail_moves_a_wanderer_back_once_per_10s`;
  - `jail_lets_a_disconnect_be`.
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(economy): the lock, mod overrides and jail`.

### Task 7: The `/shop` prank commands, cleanse, reveal and `/economy cancel`

**Files:** `crack-core/src/commands/economy/shop.rs`, `crack-core/src/commands/economy/admin.rs`,
`crack-core/src/economy/mod.rs` (must-do c), `crack-core/src/economy/shop.rs` (cleanse, reveal),
and `crack-core/src/messaging/messages.rs`.

**Interfaces:**
- `/shop` subcommands. Each takes `note: Option<String>` and `signed: Option<bool>` (except
  cleanse, mirror and reveal):
  - `rename <member> <name>`;
  - `lock <member>`;
  - `clown <member>`;
  - `drag <member>`;
  - `mute <member>`;
  - `deafen <member>`;
  - `jail <member>`;
  - `mirror`, `cleanse` and `reveal`.

  Each command: `gate()` → validate the note and nick → gather `TargetFacts` (cache + DB) → `buy` →
  on success, post the card at `card_destination` and the `ShopAction` event (reply first, then
  post), and reply privately with the balance. A refusal is private (`refusal_text`).
- `async fn cleanse(data, discord, guild, user, now) -> Result<usize, BuyError>`: charge, finish
  every running prank on the user (reason `cleansed`), then done.
- `async fn reveal(data, guild, user, action_id: Option<i64>, now) -> Result<Revealed, BuyError>`.
  `None` = the most recent unsigned, unrevealed action aimed at the user. It refuses before
  charging when there is none (`NothingToReveal`) or when the action is already revealed
  (`AlreadyRevealed`). It sets `revealed_at`/`revealed_by` and returns the buyer, so the command
  posts a public "🔍 @user unmasked it: @buyer" in the invoking channel.
- `/economy cancel <member>` (`mod_gate`) finishes every running prank on the member (reason
  `mod_cancelled`), and posts `ShopCancelled`.
- Must-do c: `opt_out(true)` and `mod_grant` finish every running prank on the member (reasons
  `opt_out` and `immunity`) at the two `// PR 2:` markers.

- [ ] **Step 1: Failing tests** (pure and db, fake seam):
  - `cleanse_ends_running_pranks_and_restores`;
  - `reveal_picks_the_latest_unsigned_unrevealed`;
  - `reveal_refuses_before_charging_when_nothing_to_reveal`;
  - `revealing_twice_is_refused_before_charging`;
  - `opting_out_ends_running_pranks`;
  - `a_mod_grant_ends_running_pranks`;
  - `cancel_ends_running_pranks`;
  - `commands_registry_has_the_new_subcommands` (names and count).
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(economy): the prank commands, cleanse, reveal, cancel`.

### Task 8: Buttons on the cards: Rename them back, and Reveal

**Files:** `crack-core/src/economy/buttons.rs` (new), `crack-core/src/handlers/serenity.rs`
(routing), and `crack-core/src/economy/cards.rs`.

**Interfaces:**
- Custom ids: `econ:back:{action_id}` and `econ:reveal:{action_id}`; the modal id is
  `econm:back:{action_id}`. The prefixes `econ:` and `econm:` must not prefix-collide with `room:`,
  `roomm:`, the gp or the np prefixes. Add a test.
- Pure `fn parse_id(&str) -> Option<EconButton>` (round-trip, and malformed ids → None).
- The target's rename/clown card carries both buttons. Only the action's **target** may press
  them; anyone else gets a private refusal.
  - **Rename them back:** if the action is signed or revealed, it opens a modal with one text
    input (the new name, max 32), and the submit runs `buy(rename, target = original buyer)`.
    Otherwise it replies privately "Reveal who did it first" (spec 5.3).
  - **Reveal:** runs `reveal(Some(action_id))` and posts the public unmasking.
- The routes go in `serenity.rs` next to rooms', by prefix.

- [ ] **Step 1: Failing tests:**
  - `econ_ids_round_trip_and_reject_malformed`;
  - `econ_prefixes_do_not_collide`;
  - `rename_back_needs_a_known_buyer`, pure: a fn
    `fn rename_back_allowed(signed: bool, revealed: bool, presser: UserId, target: UserId) -> Result<(), BackRefusal>`;
  - `only_the_target_may_press`.
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(economy): rename-them-back and reveal buttons`.

### Task 9: The dashboard for pranks

**Files:** `crack-web/src/{settings.rs,lib.rs,page.rs,routes.rs,test_support.rs}`,
`crack-web/assets/settings.js` and `crack-core/src/messaging/messages.rs`.

- The view and form gain `rename_hours`, `short_effect_minutes`, `jail_channel` (a select of voice
  channels) and `announce_channel` (a select of text channels). The price table grows by itself
  from `ActionKind::ALL`.
- Under the jail picker, `DASH_ECONOMY_JAIL_NOTE` reads: "Members are moved here even if they
  can't see it."
- The section shows the economy preflight (Task 2) per action, in words, with what to grant and
  where.
- `apply_form` validates that the channels belong to the guild and have the right kind
  (`UnknownChannel`), and keeps `extra`.

- [ ] **Step 1: Failing tests:**
  - `economy_save_passes_the_new_fields`;
  - `economy_save_refuses_a_foreign_jail_channel`;
  - `settings_page_shows_the_jail_note_and_preflight`.
- [ ] **Step 2-4.** **Step 5: Commit**: `feat(web): prank settings on the dashboard`.

### Task 10: Docs, CHANGELOG, version

- [ ] `docs/economy.md`:
  - the prank catalogue, with what each does, how long it lasts and what it needs;
  - mirror, cleanse and reveal, and Rename them back;
  - how pranks end (restarts, offline targets, mods win);
  - the jail note;
  - the permission table.

  Remove "pranks arrive in the next update" everywhere.
- [ ] CHANGELOG **Added** entry in the house style.
- [ ] Version **0.33.0**, then `cargo build --workspace`; only the workspace version lines change in
  `Cargo.lock`.
- [ ] Commit: `v0.33.0: the economy, part 2 — the prank shop`.
