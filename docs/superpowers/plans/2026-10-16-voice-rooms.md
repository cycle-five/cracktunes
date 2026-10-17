# Voice Rooms Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Temporary voice rooms: join a hub, get a room named after you with a control panel in its chat; the room is deleted once empty; settings on the dashboard. Ships as v0.28.0.

**Architecture:** A new `crack-core/src/rooms/` module. The decisions (overwrites, authority, guards, preflight, naming, ids) are pure functions over plain data. One `ops::act` entry point carries every action. A `RoomsDiscord` trait is the only door to channel/member/permission calls, and a recording fake stands in for it in tests. Messages go through the existing courier/`Transport`, rendered from `CrackedMessage::Room(Box<RoomCard>)`. Lifecycle hooks hang off `handlers/serenity.rs`. The dashboard gets a Manage-Server-only Settings page.

**Tech Stack:** Rust 2021, serenity `next` (git 37b9f43), poise, sqlx 0.9 / Postgres, axum (crack-web), tokio.

**Spec:** `docs/superpowers/specs/2026-10-16-voice-rooms-and-mod-log-design.md`. This plan is **PR 1**: spec sections 1, 3 (rooms), 4 (rooms part), 7 (PR 1). Read sections 1 and 5 before any task.

## Global Constraints

- Every feature is off by default. Rooms need a database; with `database_pool == None` enabling is refused (`RoomRefusal::NoDatabase`).
- Rooms are free: no plan or premium gate anywhere.
- Managed overwrite bits, exactly: `VIEW_CHANNEL | CONNECT | SPEAK | SEND_MESSAGES`. The bot never writes an allow of `VIEW_CHANNEL`.
- Limits from the spec: kick reason 1..=200 chars (trimmed); room name and template 1..=100 chars; member limit 0..=99; grace one of `[0, 10, 30, 60, 120, 300]` s, default 30; template default `{name}'s room`; one new room per member per 10 s; max 50 live rooms per server; 2 renames per 600 s per room; the expected-disconnect hold (PR 2) is not in this PR.
- Every user-visible string is a `pub const` in `crack-core/src/messaging/messages.rs` ([[localization-strings-in-one-file]]); logs stay inline.
- Sends, edits and deletes of messages go through `messaging::courier` / `Transport` (clippy.toml bans the direct serenity calls). Channel, member and permission calls go only through `rooms::discord::RoomsDiscord`.
- Typed serde only: no `serde_json::json!` / `Value` for data we own (JSONB columns are `sqlx::types::Json<T>` of typed structs).
- Ids parsed from custom ids reject `0` and `u64::MAX` before `ChannelId::new` (serenity ids are `NonMaxU64`; `new(u64::MAX)` panics).
- Migrations go in BOTH `migrations/` and `crack-core/test_migrations/`. `.sqlx` is regenerated with `scripts/sqlx_sync.py` (header of that file has the recipe), never `cargo sqlx prepare`.
- Never `git add -A`; add paths explicitly. Commits end with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`.
- Tests: sabotage each new test once (break the code, watch it fail, restore). Assert on what is SENT (recorded `RoomsDiscord` calls, `FakeTransport` ops), not only on return values. No source-scanning tests.
- Gate per task: `cargo +nightly fmt --all -- --check`; `cargo clippy --workspace --all-targets --features crack-core/db-tests -- -D clippy::all -D warnings --allow clippy::needless_return`; `cargo test -p <crate> <filter>`. DB tests need the local postgres from [[cracktunes-standing-rules]] (`docker --context default run -d --rm --name pg -e POSTGRES_PASSWORD=mysecretpassword -e POSTGRES_USER=postgres -e POSTGRES_DB=postgres -p 127.0.0.1:5432:5432 postgres:16-alpine`) and `--features crack-core/db-tests`.
- `docker` defaults to a remote context on this machine: always `docker --context default` for local containers.
- Format with `cargo +nightly fmt --all` (bare `rustfmt --edition 2024` reorders imports differently and CI fails).

## Review Focus

1. **Hub-hopping** (join hub, leave, rejoin within seconds; or join hub while owning a room): never two rooms for one member, never an orphan channel. Test: Task 8 `hub_rejoin_inside_cooldown_creates_one_room` and `owner_rejoining_hub_is_moved_to_existing_room`.
2. **The owner drops for a few seconds** (network blip) while others stay, or everyone drops and one comes back inside the grace: the room survives and ownership is unchanged. Test: Task 8 `rejoin_inside_grace_cancels_deletion`.
3. **Hostile display names** (`@everyone`, markdown, 300 chars, emoji, control chars, empty after stripping): channel name capped at 100 chars on a char boundary, never empty; the panel pings nobody. Test: Task 1 `render_room_name_*`; Task 5 `panel_mentions_nobody`.
4. **A modal submitted after the room changed** (target left, room deleted, ownership moved, the submitter lost authority): a clean private refusal, no panic, nothing sent to Discord. Test: Task 9 `submit_after_room_deleted_refuses_and_sends_nothing`, `submit_after_transfer_rechecks_authority`.
5. **Restart with rooms alive and people in the hub**: empty rooms deleted after the grace, occupied rooms kept with their owners, rows for vanished channels dropped, hub occupants get rooms. Test: Task 8 `reconcile_*` (four tests).

---

## File Structure

| File | Responsibility |
|---|---|
| `migrations/20261016120000_voice_rooms.sql` (+ copy in `crack-core/test_migrations/`) | `room_settings`, `rooms` tables |
| `crack-core/src/rooms/mod.rs` | module root, re-exports |
| `crack-core/src/rooms/settings.rs` | `RoomSettings`, validation, `render_room_name` |
| `crack-core/src/rooms/state.rs` | `RoomState`, `StoredOverwrite`, `OverwriteTarget` |
| `crack-core/src/db/rooms.rs` | SQL for both tables |
| `crack-core/src/rooms/overwrites.rs` | `room_overwrites`, `diff` |
| `crack-core/src/rooms/authority.rs` | `Standing`, `RoomAction`, `may`, `target_ok` |
| `crack-core/src/rooms/guard.rs` | Discord's permission algorithm over a baseline; joiner/permit guards |
| `crack-core/src/rooms/preflight.rs` | bot-permission findings, pure + cache glue |
| `crack-core/src/rooms/ids.rs` | button and modal custom ids |
| `crack-core/src/rooms/card.rs` | `RoomCard`, `RoomDone`, `RoomRefusal`, `render_card` |
| `crack-core/src/rooms/discord.rs` | `RoomsDiscord` trait + live impl |
| `crack-core/src/rooms/test_support.rs` | `FakeRoomsDiscord` |
| `crack-core/src/rooms/registry.rs` | in-memory rooms, settings cache, cooldown, timers, rename windows |
| `crack-core/src/rooms/facts.rs` | `GuildFacts` snapshot from the cache (+ HTTP fallback) |
| `crack-core/src/rooms/ops.rs` | `act`: every action |
| `crack-core/src/rooms/lifecycle.rs` | hub join, empty deletion, channel delete, reconcile |
| `crack-core/src/rooms/interact.rs` | button presses and modal submits |
| `crack-core/src/commands/rooms/{mod,room,admin}.rs` | `/room`, `/rooms` |
| `crack-web/src/settings.rs` + `crack-web/assets/settings.js` | dashboard Settings page |
| `docs/rooms-and-mod-log.md` | user/admin guide (rooms part) |

---

### Task 1: Storage, settings and room state types

**Files:**
- Create: `migrations/20261016120000_voice_rooms.sql`, `crack-core/test_migrations/20261016120000_voice_rooms.sql` (identical)
- Create: `crack-core/src/rooms/mod.rs`, `crack-core/src/rooms/settings.rs`, `crack-core/src/rooms/state.rs`, `crack-core/src/db/rooms.rs`
- Modify: `crack-core/src/lib.rs` (`pub mod rooms;`), `crack-core/src/db/mod.rs` (`pub mod rooms;`), `.sqlx/` (regenerated)

**Interfaces:**
- Produces:
  ```rust
  // rooms/settings.rs
  pub const GRACE_CHOICES: [u16; 6] = [0, 10, 30, 60, 120, 300];
  pub const DEFAULT_TEMPLATE: &str = "{name}'s room";
  pub const ROOM_NAME_MAX: usize = 100;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RoomSettings { pub guild: GuildId, pub enabled: bool, pub hub: Option<ChannelId>,
      pub name_template: String, pub default_limit: u8, pub empty_grace_secs: u16 }
  impl RoomSettings { pub fn off(guild: GuildId) -> Self; pub fn validate(&self, has_db: bool) -> Result<(), SettingsError>; }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum SettingsError { NoDatabase, NoHub, BadTemplate, BadLimit, BadGrace }
  pub fn render_room_name(template: &str, display_name: &str) -> String;
  // rooms/state.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
  #[serde(tag = "type", rename_all = "snake_case")]
  pub enum OverwriteTarget { Member { id: UserId }, Role { id: RoleId } }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  pub struct StoredOverwrite { pub target: OverwriteTarget, pub allow: Permissions, pub deny: Permissions }
  impl From<&PermissionOverwrite> for StoredOverwrite; impl StoredOverwrite { pub fn to_serenity(&self) -> PermissionOverwrite; }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct RoomState { pub owner: UserId, #[serde(default)] pub helpers: BTreeSet<UserId>,
      #[serde(default)] pub blocked: BTreeSet<UserId>, #[serde(default)] pub muted: BTreeSet<UserId>,
      #[serde(default)] pub permitted_users: BTreeSet<UserId>, #[serde(default)] pub permitted_roles: BTreeSet<RoleId>,
      #[serde(default)] pub blocked_roles: BTreeSet<RoleId>, #[serde(default)] pub locked: bool,
      #[serde(default)] pub chat_locked: bool, #[serde(default)] pub panel: Option<MessageId> }
  impl RoomState { pub fn new(owner: UserId) -> Self; }
  // db/rooms.rs
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct StoredRoom { pub channel: ChannelId, pub guild: GuildId, pub owner: UserId,
      pub state: RoomState, pub baseline: Vec<StoredOverwrite> }
  pub async fn load_settings(pool: &PgPool, guild: GuildId) -> sqlx::Result<Option<RoomSettings>>;
  pub async fn save_settings(pool: &PgPool, s: &RoomSettings) -> sqlx::Result<()>;   // upsert, updated_at = now()
  pub async fn insert_room(pool: &PgPool, r: &StoredRoom) -> sqlx::Result<()>;
  pub async fn save_state(pool: &PgPool, channel: ChannelId, owner: UserId, state: &RoomState) -> sqlx::Result<()>;
  pub async fn delete_room(pool: &PgPool, channel: ChannelId) -> sqlx::Result<()>;
  pub async fn rooms_in(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<StoredRoom>>;
  ```

- [ ] **Step 1: Write the migration.** Columns exactly as spec 1.1/1.2 (`rooms` has `channel_id BIGINT PRIMARY KEY, guild_id BIGINT NOT NULL, owner_id BIGINT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), state JSONB NOT NULL, baseline JSONB NOT NULL`, plus `CREATE INDEX IF NOT EXISTS rooms_guild_id ON rooms (guild_id)`). `room_settings` has CHECKs: `default_limit BETWEEN 0 AND 99`, `empty_grace_seconds IN (0,10,30,60,120,300)`. A header comment per table, in the style of `20261009140000_queue_snapshot.sql`. Copy to `crack-core/test_migrations/`. Verify there's no duplicate version: `ls migrations | sort | uniq -w14 -d` prints nothing.

- [ ] **Step 2: Write failing pure tests in `rooms/settings.rs` and `rooms/state.rs`:**
  - `render_room_name_fills_the_template`: `render_room_name("{name}'s room", "Saucey") == "Saucey's room"`.
  - `render_room_name_caps_at_100_chars_on_a_char_boundary`: a display name of 300 `"é"` gives a name with `chars().count() == 100`.
  - `render_room_name_strips_control_chars_and_never_returns_empty`: `render_room_name("{name}", "\u{0}\n\t")` → `"Room"` (fallback const `ROOM_NAME_FALLBACK` in messages.rs).
  - `render_room_name_keeps_mentions_as_text`: `"@everyone"` passes through unchanged (channel names do not ping; the panel guards pings, Task 5).
  - `validate_refuses_enabling_without_db_or_hub`: `enabled: true, hub: None` → `NoHub`; `has_db: false` → `NoDatabase`. A disabled config validates without hub or db.
  - `validate_refuses_out_of_range_values`: limit 100 → `BadLimit`; grace 45 → `BadGrace`; template `""`/`"   "`/101 chars/containing `'\u{7}'` → `BadTemplate`.
  - `room_state_round_trips_through_json` and `room_state_reads_a_row_missing_new_fields`: `{"owner":"1"}` deserializes to `RoomState::new(UserId::new(1))`.
  - `stored_overwrite_round_trips_through_serenity`: member and role targets, `allow`/`deny` preserved.

- [ ] **Step 3: Run them; expect compile failure.** `cargo test -p crack-core rooms::` → errors for missing items.

- [ ] **Step 4: Implement `settings.rs` and `state.rs`.** `render_room_name`: replace every `{name}` with the display name; drop `char::is_control` chars; trim; take the first 100 chars; empty → `ROOM_NAME_FALLBACK`. `validate` checks in order NoDatabase (only when enabled), NoHub (only when enabled), template, limit, grace.

- [ ] **Step 5: Write failing DB tests in `db/rooms.rs`** (`#[sqlx::test(migrator = "MIGRATOR")]` with the `db-tests` `cfg_attr` ignore used in `db/queue_snapshot.rs`; `MIGRATOR = sqlx::migrate!("./test_migrations")`):
  - `settings_survive_a_save_and_load` (all fields non-default); `missing_settings_load_as_none`.
  - `a_room_survives_insert_save_state_and_load` (state with every set non-empty, baseline with two overwrites).
  - `delete_room_removes_only_that_room`; `rooms_in_returns_only_that_guilds_rooms`.

- [ ] **Step 6: Implement `db/rooms.rs`** with `sqlx::query!` and `Json<RoomState>` / `Json<Vec<StoredOverwrite>>`. Ids are stored as `i64` (`get() as i64`), read back with a filter that drops non-positive values (log and skip the row: never `UserId::new(0)`).

- [ ] **Step 7: Regenerate `.sqlx`** per the `scripts/sqlx_sync.py` header (migrate a fresh local DB with `sqlx migrate run --source migrations/` first). Run the DB tests: `cargo test -p crack-core --features db-tests db::rooms` → 5 passed.

- [ ] **Step 8: Sabotage** each test once (e.g. drop `baseline` from the INSERT, cap at 99), then restore. **Commit** `feat(rooms): storage, settings and room state`.

---

### Task 2: The managed overwrite layer

**Files:** Create `crack-core/src/rooms/overwrites.rs`.

**Interfaces:**
- Consumes: `RoomState`, `StoredOverwrite`, `OverwriteTarget` (Task 1).
- Produces:
  ```rust
  pub const MANAGED: Permissions = VIEW_CHANNEL | CONNECT | SPEAK | SEND_MESSAGES;
  pub fn room_overwrites(guild: GuildId, baseline: &[StoredOverwrite], state: &RoomState) -> Vec<StoredOverwrite>; // sorted by target, no all-zero entries
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum OverwriteChange { Put(StoredOverwrite), Delete(OverwriteTarget) }
  pub fn diff(before: &[StoredOverwrite], after: &[StoredOverwrite]) -> Vec<OverwriteChange>; // sorted by target
  ```

Rules (spec 1.5): `@everyone` is `Role { id: RoleId::new(guild.get()) }`. Start from the baseline. While `locked`: on `@everyone` (created if absent) and every Role entry, CONNECT leaves allow and joins deny. Then each permitted role gets allow CONNECT (deny cleared), and the owner, helpers and permitted users get member allow CONNECT. While `chat_locked`: the same on SEND_MESSAGES, with member allow SEND_MESSAGES for the owner and helpers. Blocked roles get deny VIEW_CHANNEL|CONNECT (allow cleared of both). Muted users get member deny SPEAK. Blocked users get member deny VIEW_CHANNEL|CONNECT; blocked wins over permitted. Member entries keep any non-managed bits the baseline gave that member.

- [ ] **Step 1: Write failing tests:**
  - `unlocked_default_state_is_the_baseline` (owner only, nothing set → output == sorted baseline minus all-zero entries).
  - `lock_moves_connect_to_deny_on_everyone_and_every_role_allow` (baseline: `@everyone` deny VIEW; role 18+ allow VIEW|CONNECT → 18+ ends allow VIEW, deny CONNECT; `@everyone` deny VIEW|CONNECT).
  - `lock_lets_owner_helpers_and_permitted_users_connect`; `permitted_role_keeps_connect_while_locked`.
  - `chat_lock_moves_send_to_deny_and_exempts_owner_and_helpers`.
  - `blocked_user_is_denied_view_and_connect_even_if_permitted`; `muted_user_is_denied_speak_only`; `blocked_role_denied_view_and_connect`.
  - `never_widens_view`: for a table of states × baselines, every output entry's `allow & VIEW_CHANNEL` ⊆ that target's baseline allow.
  - `non_managed_baseline_bits_survive` (a role with allow MANAGE_MESSAGES keeps it through lock + chat lock).
  - `diff_puts_changed_and_new_targets_and_deletes_vanished_ones`; `diff_of_equal_sets_is_empty`.

- [ ] **Step 2:** Run `cargo test -p crack-core rooms::overwrites` → fails to compile.
- [ ] **Step 3:** Implement with a `BTreeMap<OverwriteTarget, (Permissions, Permissions)>`.
- [ ] **Step 4:** Tests pass; sabotage (skip the role loop under lock; allow VIEW for permitted users) → caught; restore.
- [ ] **Step 5: Commit** `feat(rooms): the managed overwrite layer`.

---

### Task 3: Authority and permission guards

**Files:** Create `crack-core/src/rooms/authority.rs`, `crack-core/src/rooms/guard.rs`.

**Interfaces:**
- Produces:
  ```rust
  // authority.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Standing { Owner, Helper, Moderator, Occupant, Outsider }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Reason(String);  // only via Reason::parse
  impl Reason { pub const MAX: usize = 200; pub fn parse(raw: &str) -> Result<Self, RoomRefusal>; pub fn as_str(&self) -> &str; }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum RoomAction { Kick { target: UserId, reason: Reason }, LetIn { target: UserId },
      Mute { target: UserId, reason: Option<Reason> }, Unmute { target: UserId }, Lock, Unlock, ChatLock, ChatUnlock,
      Limit(u8), Rename(String), Transfer { target: UserId }, HelperAdd { target: UserId }, HelperRemove { target: UserId },
      AllowUser { target: UserId }, AllowRole { role: RoleId }, BlockRole { role: RoleId }, UnblockRole { role: RoleId },
      Claim, Panel }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TargetStanding { Itself, TheBot, Owner, Helper, Moderator, Plain }
  pub fn standing(user: UserId, state: &RoomState, is_moderator: bool, in_room: bool) -> Standing;
  pub fn is_moderator(guild_perms: Permissions) -> bool; // ADMINISTRATOR | MANAGE_CHANNELS | MOVE_MEMBERS, any
  pub fn may(who: Standing, action: &RoomAction, owner_present: bool) -> Result<(), RoomRefusal>;
  pub fn target_ok(who: Standing, action: &RoomAction, target: TargetStanding) -> Result<(), RoomRefusal>;
  // guard.rs
  pub struct BaselineView<'a> { pub guild: GuildId, pub everyone: Permissions,
      pub role_perms: &'a HashMap<RoleId, Permissions>, pub overwrites: &'a [StoredOverwrite] }
  pub fn member_permissions(v: &BaselineView<'_>, user: UserId, roles: &[RoleId]) -> Permissions;
  pub fn member_can_join(v: &BaselineView<'_>, user: UserId, roles: &[RoleId]) -> bool; // VIEW_CHANNEL && CONNECT
  pub fn role_can_connect(v: &BaselineView<'_>, role: RoleId) -> bool;               // a member holding only that role
  ```
  `RoomRefusal` is defined in Task 5's `card.rs`. **Define it here first** (in `rooms/card.rs`, the enum only), and Task 5 adds rendering. Variants: `NotInRoom, NotAllowed, ProtectedTarget, TargetNotInRoom, NotBlocked, CannotJoinHere, RenameLimited { wait_secs: u32 }, BadLimit, BadName, ReasonRequired, ReasonTooLong, MissingChoice, OwnerPresent, AlreadyOwner, TooManyForMenu, MissingPermissions, DiscordFailed, NoDatabase, RoomsOff`. (`MissingChoice`: a modal submitted without the member/role/mode its action needs.)

The `may` table is spec 1.4. Claim: Occupant, Helper or Moderator, only when `!owner_present`; Owner → `AlreadyOwner`; owner present → `OwnerPresent`. Helper: Kick, LetIn, Mute, Unmute, Lock, Unlock, ChatLock, ChatUnlock, Limit, Panel. Anything else for a non-owner, non-moderator → `NotAllowed`. `target_ok`: Kick and Mute refuse Itself, TheBot, Moderator; refuse Owner unless `who == Moderator`; refuse Helper when `who == Helper`. Every other targeted action refuses TheBot, and Transfer/HelperAdd refuse Itself. `member_permissions` is Discord's documented algorithm: `everyone | roles`, ADMINISTRATOR → all; then the `@everyone` overwrite, the union of role denies, the union of role allows, then the member overwrite. A role missing from `role_perms` counts as empty.

- [ ] **Step 1: Write failing tests:** one test per `may` row (`owner_may_do_everything_but_claim`, `helper_may_only_moderate`, `occupant_may_only_claim_when_owner_absent`, `outsider_may_nothing`, `moderator_may_everything`); `kick_refuses_self_bot_moderator_and_owner_unless_moderator`; `helper_cannot_kick_helper`; `unmute_letin_refuse_only_the_bot`; `reason_parse_trims_and_bounds` (`"  "` → `ReasonRequired`; 201 chars → `ReasonTooLong`; 200 ok); `admin_role_gets_everything`; `member_overwrite_beats_role_overwrites`; `role_allow_beats_everyone_deny`; `member_can_join_needs_view_and_connect` (18+-gated baseline: a member without the 18+ role → false, with it → true); `role_can_connect_respects_category_deny`.
- [ ] **Step 2:** Run → compile failure. **Step 3:** Implement. **Step 4:** Pass; sabotage (swap role allow/deny order; let Helper Transfer) → caught. **Step 5: Commit** `feat(rooms): authority and permission guards`.

---

### Task 4: Permission preflight

**Files:** Create `crack-core/src/rooms/preflight.rs`.

**Interfaces:**
- Consumes: `StoredOverwrite`, `guard::member_permissions` (Task 3).
- Produces:
  ```rust
  pub const NEED: Permissions = VIEW_CHANNEL | CONNECT | MANAGE_CHANNELS | MANAGE_ROLES | MOVE_MEMBERS | SEND_MESSAGES | EMBED_LINKS;
  pub const NEED_IN_HUB: Permissions = VIEW_CHANNEL | CONNECT | MOVE_MEMBERS;
  pub const OPTIONAL: Permissions = PIN_MESSAGES;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Where { Category(ChannelId), Hub(ChannelId), Server }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Finding { Missing { at: Where, perms: Permissions }, Optional { at: Where, perms: Permissions }, LockMayFail { perms: Permissions } }
  pub struct PreflightInput { pub hub: ChannelId, pub category: Option<ChannelId>,
      pub bot_in_category: Permissions, pub bot_in_hub: Permissions, pub category_overwrites: Vec<StoredOverwrite> }
  pub fn preflight(i: &PreflightInput) -> Vec<Finding>;   // empty = ready
  pub fn for_guild(cache: &Cache, guild: GuildId, hub: ChannelId) -> Option<Vec<Finding>>; // None: cache cannot answer
  ```
  With no category, `bot_in_category` is the bot's server-level permissions and `Where::Server` is used. `LockMayFail.perms` = (union of allow|deny over Role entries of `category_overwrites`) minus `bot_in_category` minus `MANAGED`. Empty → no finding.

- [ ] **Step 1: Write failing tests:** `all_granted_is_ready`; `missing_move_members_in_category_is_named_there`; `missing_in_hub_is_reported_separately`; `no_category_reports_server`; `pin_is_only_optional`; `baseline_bit_the_bot_lacks_warns_lock_may_fail` (role allow MANAGE_MESSAGES, bot lacks it → `LockMayFail { perms: MANAGE_MESSAGES }`); `managed_bits_never_warn`.
- [ ] **Step 2–4:** Fail → implement (`for_guild` uses `guild.user_permissions_in(channel, bot_member)` for the two channels, with `music::perms::roles_resolvable` first; any miss → `None`) → pass → sabotage.
- [ ] **Step 5: Commit** `feat(rooms): permission preflight`.

---

### Task 5: Custom ids and the room card

**Files:** Create `crack-core/src/rooms/ids.rs`; fill `crack-core/src/rooms/card.rs`. Modify `crack-core/src/messaging/message.rs` (variant + Display arm), `crack-core/src/messaging/render.rs` (dispatch arm), `crack-core/src/messaging/messages.rs` (strings).

**Interfaces:**
- Produces:
  ```rust
  // ids.rs
  pub const ROOM_PREFIX: &str = "room:"; pub const ROOM_MODAL_PREFIX: &str = "roomm:";
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RoomButton { Kick, Mute, Unmute, LetIn, Lock, Unlock, ChatLock, ChatUnlock, Limit, Rename, Transfer, Helpers, WhoCanJoin, Claim, Help }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum RoomModal { Kick, Mute, Unmute, LetIn, Limit, Rename, Transfer, Helpers, WhoCanJoin }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ButtonId { pub kind: RoomButton, pub room: ChannelId }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ModalId { pub kind: RoomModal, pub room: ChannelId }
  impl ButtonId { pub fn custom_id(&self) -> String; pub fn parse(id: &str) -> Option<Self>; }
  impl ModalId  { pub fn custom_id(&self) -> String; pub fn parse(id: &str) -> Option<Self>; }
  pub mod field { pub const MEMBER: &str = "member"; pub const REASON: &str = "reason"; pub const VALUE: &str = "value";
      pub const MODE: &str = "mode"; pub const ROLE: &str = "role"; }
  // card.rs
  pub struct PanelView { pub room: ChannelId, pub owner: UserId, pub helpers: Vec<UserId>, pub locked: bool, pub chat_locked: bool, pub limit: u8 }
  pub enum RoomDone { Kicked(UserId), LetIn(UserId), Muted(UserId), Unmuted(UserId), Locked, Unlocked, ChatLocked, ChatUnlocked,
      LimitSet(u8), Renamed(String), Transferred(UserId), HelperAdded(UserId), HelperRemoved(UserId),
      AllowedUser(UserId), AllowedRole(RoleId), RoleBlocked(RoleId), RoleUnblocked(RoleId), Claimed, PanelPosted }
  pub enum RoomCard { Panel(PanelView), Guide, Done(RoomDone), Refused(RoomRefusal),
      Status { settings: RoomSettings, findings: Option<Vec<Finding>>, dashboard: Option<String> }, Switched { on: bool } }
  pub fn render_card(card: &RoomCard, cx: &RenderCx) -> Rendered;
  // message.rs
  CrackedMessage::Room(Box<crate::rooms::card::RoomCard>)
  ```
  Id format `room:<kind>:<channel>` / `roomm:<kind>:<channel>`. Kebab kinds: `kick mute unmute let-in lock unlock chat-lock chat-unlock limit rename transfer helpers who claim help`. Parsing follows `NowPlayingButton::parse`: reject 0 and `u64::MAX`, and reject trailing parts.

  Panel (spec 1.7): the embed title is the owner's mention-free label. Use the `{owner}` mention inside the description, since embeds do not ping, and keep `Mentions::None`. The fields are Owner, Helpers, Locked/Chat, Limit. The reference body lists each control as `**Kick** · /room kick — disconnect someone and keep them out; asks for a reason.` (copy in messages.rs, one const per line). It closes with "This room is deleted when everyone leaves." and "Deafen isn't available: Discord can't deafen someone in just one channel." Three button rows as the spec lists them, with labels emoji + word. The Lock/Unlock and Lock chat/Unlock chat buttons are drawn by state. `Guide` is the long private help. `Done`/`Refused` are one-line private replies; `Refused(MissingPermissions)` names the category. `Status` lists the settings, then each `Finding` in words ("Give CrackTunes **Move Members** on <#category>"), or "CrackTunes has everything it needs.", or "Couldn't check permissions right now." when `findings` is `None`.

- [ ] **Step 1: Write failing tests:** `button_ids_round_trip_every_kind`, `modal_ids_round_trip_every_kind`, `ids_reject_zero_max_garbage_and_trailing_parts` (incl. `"room:kick:0"`, `"room:kick:18446744073709551615"`, `"room:kick:1:2"`, `"room:nope:1"`, `"np:pause:1"`); `panel_has_three_rows_and_every_button_targets_this_room`; `panel_draws_unlock_when_locked_and_lock_when_not`; `panel_mentions_nobody` (`Rendered.mentions == Mentions::None` and owner name `@everyone` appears only inside the embed); `panel_lists_every_control_with_its_slash_command` (each of `/room kick`, `/room mute`, … appears in the description); `status_names_missing_permission_and_place`; `every_refusal_renders_nonempty_text`.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (swap two kinds' strings; drop the room id from one button).
- [ ] **Step 5: Commit** `feat(rooms): custom ids and the room card`.

---

### Task 6: The Discord seams

**Files:** Create `crack-core/src/rooms/discord.rs`, `crack-core/src/rooms/test_support.rs` (`#[cfg(test)]`). Modify `crack-core/src/messaging/transport.rs` (`Press::open_modal`, `DiscordModalPress`), `crack-core/src/messaging/test_support.rs` (`FakePress` records modals).

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum DiscordError { MissingPermissions, UnknownChannel, Other(String) }
  pub struct NewRoom { pub guild: GuildId, pub name: String, pub parent: Option<ChannelId>, pub user_limit: u8 }
  pub struct CreatedRoom { pub channel: ChannelId, pub overwrites: Vec<StoredOverwrite> }
  #[async_trait] pub trait RoomsDiscord: Send + Sync {
      async fn create_voice_channel(&self, r: &NewRoom) -> Result<CreatedRoom, DiscordError>;  // NO permission_overwrites sent
      async fn delete_channel(&self, ch: ChannelId) -> Result<(), DiscordError>;
      async fn put_overwrite(&self, ch: ChannelId, o: &StoredOverwrite) -> Result<(), DiscordError>;
      async fn delete_overwrite(&self, ch: ChannelId, t: OverwriteTarget) -> Result<(), DiscordError>;
      async fn set_user_limit(&self, ch: ChannelId, limit: u8) -> Result<(), DiscordError>;
      async fn rename(&self, ch: ChannelId, name: &str) -> Result<(), DiscordError>;
      async fn move_member(&self, guild: GuildId, user: UserId, to: Option<ChannelId>) -> Result<(), DiscordError>; // None disconnects
      async fn pin(&self, ch: ChannelId, msg: MessageId) -> Result<(), DiscordError>;
  }
  pub struct LiveRoomsDiscord { pub http: Arc<Http> }
  // test_support.rs
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Call { Create(String), Delete(u64), Put(StoredOverwrite), DeleteOverwrite(OverwriteTarget),
      Limit(u64, u8), Rename(u64, String), Move(u64, Option<u64>), Pin(u64, u64) }
  #[derive(Default)] pub struct FakeRoomsDiscord { /* calls, scripted failures per call kind, next channel id, created overwrites */ }
  impl FakeRoomsDiscord { pub fn calls(&self) -> Vec<Call>; pub fn fail_next(&self, kind: CallKind, e: DiscordError); pub fn with_created_overwrites(self, o: Vec<StoredOverwrite>) -> Self; }
  // transport.rs: Press gains
  async fn open_modal(&self, modal: CreateModal<'static>) -> Result<(), TransportError>;
  pub struct DiscordModalPress<'a> { pub http: &'a Http, pub interaction: &'a ModalInteraction } // impl Press; open_modal returns Err(Other("a modal cannot open a modal"))
  // FakePress records PressOp::Modal(custom_id)
  ```
  `DiscordError::from(serenity::Error)`: HTTP 403 with code 50013 → `MissingPermissions`; 10003 (Unknown Channel) → `UnknownChannel`. Follow `http_utils::is_unknown_message`'s style. `LiveRoomsDiscord` uses `GuildId::create_channel` (kind Voice, parent, user_limit), `ChannelId::create_permission`/`delete_permission`, `EditChannel` (`user_limit`, `name`), `GuildId::move_member`/`disconnect_member`, `ChannelId::pin`. Each call carries an audit-log reason `"CrackTunes voice room"`.

- [ ] **Step 1: Write failing tests:** `discord_error_maps_50013_and_10003` (construct `serenity::Error::Http` values as `http_utils` tests do); `fake_records_calls_in_order_and_fails_on_script`; `fake_press_records_an_opened_modal`. The live impl has no tests, and the TuneTitan checklist covers it.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage the mapping.
- [ ] **Step 5: Commit** `feat(rooms): the Discord seam, and Press can open a modal`.

---

### Task 7: Registry, facts, and `ops::act`

**Files:** Create `crack-core/src/rooms/registry.rs`, `crack-core/src/rooms/facts.rs`, `crack-core/src/rooms/ops.rs`. Modify `crack-core/src/lib.rs` (`DataInner.rooms: crate::rooms::registry::RoomRegistry`, in `Default`).

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces:
  ```rust
  // registry.rs
  pub struct LiveRoom { pub guild: GuildId, pub channel: ChannelId, pub state: RoomState, pub baseline: Vec<StoredOverwrite>,
      pub limit: u8, pub renames: RenameWindow }
  pub struct RenameWindow(VecDeque<Instant>);
  impl RenameWindow { pub const MAX: usize = 2; pub const WINDOW: Duration = Duration::from_secs(600);
      pub fn try_take(&mut self, now: Instant) -> Result<(), Duration /* wait */>; }
  #[derive(Default)] pub struct RoomRegistry { /* rooms: DashMap<ChannelId, Arc<Mutex<LiveRoom>>>, owners: DashMap<(GuildId, UserId), ChannelId>,
      settings: DashMap<GuildId, RoomSettings>, timers: DashMap<ChannelId, tokio::task::AbortHandle>,
      cooldown: Throttle<(GuildId, UserId)> (10 s), hub_creating: DashSet<(GuildId, UserId)> */ }
  impl RoomRegistry {
      pub fn settings(&self, g: GuildId) -> RoomSettings;          // RoomSettings::off(g) if absent
      pub fn set_settings(&self, s: RoomSettings);
      pub fn get(&self, ch: ChannelId) -> Option<Arc<Mutex<LiveRoom>>>;
      pub fn insert(&self, room: LiveRoom);
      pub fn remove(&self, ch: ChannelId) -> Option<Arc<Mutex<LiveRoom>>>;  // also clears owner index + timer
      pub fn owned_by(&self, g: GuildId, u: UserId) -> Option<ChannelId>;
      pub fn reindex_owner(&self, g: GuildId, old: UserId, new: UserId, ch: ChannelId);
      pub fn count_in(&self, g: GuildId) -> usize;
  }
  // facts.rs: a snapshot of what decisions need, read once per interaction
  pub struct GuildFacts { pub guild: GuildId, pub bot: UserId, pub everyone: Permissions,
      pub role_perms: HashMap<RoleId, Permissions>, pub roles: HashMap<UserId, Vec<RoleId>>,
      pub voice: HashMap<UserId, (ChannelId, bool /*is_bot*/)>, pub display: HashMap<UserId, String> }
  impl GuildFacts {
      pub fn humans_in(&self, ch: ChannelId) -> Vec<UserId>;     // sorted
      pub fn in_room(&self, u: UserId, ch: ChannelId) -> bool;
      pub fn guild_perms(&self, u: UserId) -> Permissions;        // everyone | roles, ADMIN → all
  }
  pub async fn gather(cache: &Cache, http: &Http, guild: GuildId, extra: &[UserId]) -> Option<GuildFacts>; // HTTP get_member for `extra` users missing from the cache
  // ops.rs
  pub struct RoomCx<'a> { pub data: &'a Data, pub discord: &'a dyn RoomsDiscord, pub transport: &'a dyn Transport }
  pub async fn act(cx: &RoomCx<'_>, facts: &GuildFacts, room: ChannelId, actor: UserId, action: RoomAction, now: Instant) -> Result<RoomDone, RoomRefusal>;
  pub async fn post_panel(cx: &RoomCx<'_>, room: &mut LiveRoom) -> Option<MessageId>;   // send + best-effort pin; replaces state.panel
  pub async fn redraw_panel(cx: &RoomCx<'_>, room: &LiveRoom);                          // edit in place; UnknownMessage → post_panel
  ```
  `act` order, exactly as spec 1.6:
  1. Look up the room (`NotInRoom` if absent) and lock its mutex.
  2. `standing` → `may` → `target_ok`, with the target standing derived from `facts`. A Kick/Mute/Transfer target not in the room → `TargetNotInRoom`; LetIn of a non-blocked user → `NotBlocked`.
  3. Guards (`member_can_join` / `role_can_connect` on `BaselineView` from facts + `room.baseline`) for HelperAdd, AllowUser, AllowRole, Transfer → `CannotJoinHere`.
  4. Compute the new state on a clone.
  5. Discord effects: Kick disconnects the target if present (`move_member(.., None)`). Then `diff(room_overwrites(old), room_overwrites(new))` is applied in order. Limit → `set_user_limit`. Rename → `RenameWindow::try_take`, then `rename`. Panel → `post_panel`.
  6. Any `DiscordError` stops here: return `MissingPermissions`/`DiscordFailed`, state unchanged, nothing persisted.
  7. `db::rooms::save_state`. A DB failure is logged at warn and does not undo Discord: memory follows Discord, and reconcile corrects.
  8. Commit the clone into the room, and `reindex_owner` on Transfer/Claim.
  9. `redraw_panel`.
  10. `Ok(RoomDone)`.

- [ ] **Step 1: Write failing tests** (build `Data` with `DataInner { ..Default::default() }` as in `guild/operations.rs` tests, insert a `LiveRoom`, use `FakeRoomsDiscord` + `FakeTransport`, hand-built `GuildFacts`):
  - `kick_disconnects_then_denies_and_returns_kicked`: calls == `[Move(target, None), Put(member deny VIEW|CONNECT)]`, state.blocked contains target, panel edited (`Op::Edit`).
  - `kick_of_someone_not_in_room_refuses_and_sends_nothing`.
  - `a_refused_overwrite_leaves_state_unchanged` (`fail_next(Put, MissingPermissions)` on Lock → `Err(MissingPermissions)`, `state.locked == false`, no panel edit).
  - `lock_puts_only_the_changed_targets`.
  - `rename_third_inside_ten_minutes_is_refused_with_wait` (two OK, third `RenameLimited { wait_secs > 0 }`, two `Rename` calls only).
  - `transfer_moves_ownership_and_owner_index`; `claim_only_when_owner_absent`.
  - `helper_add_refuses_member_the_baseline_keeps_out` (`CannotJoinHere`, no calls).
  - `limit_sets_user_limit`; `panel_reposts_and_records_the_new_id`.
  - `rename_window_*` pure tests on `RenameWindow`.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (persist before Discord; skip the target-in-room check) → caught.
- [ ] **Step 5: Commit** `feat(rooms): registry, facts and ops::act`.

---

### Task 8: Lifecycle: hub join, emptying, deletion by hand, reconcile

**Files:** Create `crack-core/src/rooms/lifecycle.rs`. Modify `crack-core/src/handlers/serenity.rs` (dispatch: `VoiceStateUpdate` → `rooms::lifecycle::on_voice_state` before the existing bot-only handler; `ChannelDelete` → `on_channel_delete`; `on_guild_create` → load room settings into the registry, then `reconcile_guild` after the queue resume).

**Interfaces:**
- Consumes: Tasks 1–7.
- Produces:
  ```rust
  pub enum VoiceChange { Joined(ChannelId), Left(ChannelId), Moved { from: ChannelId, to: ChannelId } }
  pub fn classify(old: Option<ChannelId>, new: Option<ChannelId>) -> Option<VoiceChange>;
  pub async fn on_voice_state(data: &Data, ctx: &serenity::all::Context, old: Option<&VoiceState>, new: &VoiceState);
  pub struct Hub { pub channel: ChannelId, pub parent: Option<ChannelId>, pub category_overwrites: Vec<StoredOverwrite> } // read from the cache by the glue
  pub async fn hub_join(cx: &RoomCx<'_>, facts: &GuildFacts, settings: &RoomSettings, hub: &Hub, user: UserId, now: Instant) -> HubOutcome;
  pub enum HubOutcome { MovedToOwn(ChannelId), Created(ChannelId), CoolingDown(Duration), Refused(&'static str /* log reason */) }
  pub fn schedule_if_empty(data: &Data, discord: Arc<dyn RoomsDiscord>, facts: &GuildFacts, room: ChannelId, grace: Duration);
  pub async fn delete_room(data: &Data, discord: &dyn RoomsDiscord, room: ChannelId); // channel, then row; UnknownChannel → row only
  pub async fn on_channel_delete(data: &Data, channel: ChannelId);
  pub async fn reconcile_guild(data: &Data, ctx: &serenity::all::Context, guild: &serenity::all::Guild);
  ```
  `hub_join` follows spec 1.3 steps 1–9 in order, plus a **sync check** after step 5 (spec 5's first assumption is unverified, and a wrong guess there must never open an 18+ category to everyone):
  1. `owned_by` → move them into their room.
  2. `cooldown.allow` failed → `CoolingDown(remaining)`; `on_voice_state` spawns a re-check after it.
  3. Joiner guard: `member_can_join` over `hub.category_overwrites`.
  4. Cap of 50.
  5. `create_voice_channel` with `NewRoom { parent: hub.parent, user_limit: settings.default_limit }`.
     **Sync check:** if `created.overwrites` (sorted) differs from `hub.category_overwrites` (sorted), `put_overwrite` each category overwrite onto the room. If any put fails, `delete_channel` and return `Refused("category overwrites could not be copied")`. Otherwise the baseline is the category's overwrites, and the mismatch is logged at warn once per process.
  6. `insert_room` with `baseline = created.overwrites`. On failure, delete the channel.
  7. Apply `diff(&created.overwrites, &room_overwrites(guild, &created.overwrites, &state))`.
  8. `move_member(guild, user, Some(room))`. On failure, `delete_room`.
  9. `post_panel`, then register.

  `hub_creating` guards against a double event for one user. `schedule_if_empty`: zero humans → spawn a sleep of `grace`, then re-gather facts, still empty → `delete_room`. The `AbortHandle` is stored, and any join to the room aborts it. `classify` ignores self-mute/deafen-only updates (same channel → `None`).

- [ ] **Step 1: Write failing tests** (fakes; `tokio::time::pause()` for timers):
  - `classify_*` (join, leave, move, same-channel is None).
  - `hub_join_creates_persists_overwrites_moves_and_posts_in_order` (calls `[Create(name), …Put…, Move(user, Some(room)), Pin(..)]`, `FakeTransport` `Send(room)`, registry has the room owned by user). Use `with_created_overwrites` to give a baseline.
  - `create_sends_no_permission_overwrites` (the `NewRoom` carries none: assert the fake's recorded `NewRoom` fields).
  - `unsynced_creation_copies_category_overwrites` (fake returns `overwrites: vec![]`, category has an `@everyone` deny VIEW → a `Put` of exactly that before the `Move`).
  - `unsynced_creation_that_cannot_copy_deletes_the_room` (`fail_next(Put, MissingPermissions)` → `Delete(room)`, no `Move`, no row).
  - `failed_move_deletes_channel_and_row`.
  - `owner_rejoining_hub_is_moved_to_existing_room` (Review Focus 1).
  - `hub_rejoin_inside_cooldown_creates_one_room` (Review Focus 1).
  - `joiner_the_category_keeps_out_gets_no_room`.
  - `fifty_rooms_is_the_cap`.
  - `empty_room_is_deleted_after_grace` and `rejoin_inside_grace_cancels_deletion` (Review Focus 2).
  - `bots_alone_count_as_empty`.
  - `unknown_channel_on_delete_drops_row_anyway`.
  - `channel_deleted_by_hand_drops_room_and_timer`.
  - `reconcile_drops_rows_for_vanished_channels`, `reconcile_schedules_empty_rooms`, `reconcile_keeps_occupied_rooms_and_owners`, `reconcile_gives_hub_occupants_rooms` (Review Focus 5). Write the reconcile core as `reconcile(cx, facts, stored: Vec<StoredRoom>, existing: &HashSet<ChannelId>, settings, hub: &Hub)` so it is testable without a `Guild`; `reconcile_guild` is the thin glue.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (skip the timer abort; reverse create/insert order) → caught.
- [ ] **Step 5:** Wire `handlers/serenity.rs`. No unit test for the routing line; it is on the TuneTitan checklist. **Commit** `feat(rooms): lifecycle — hub join, empty deletion, reconcile`.

---

### Task 9: Buttons and modals

**Files:** Create `crack-core/src/rooms/interact.rs`. Modify `crack-core/src/handlers/serenity.rs` (route `Interaction::Component` with `ROOM_PREFIX` and `Interaction::Modal` with `ROOM_MODAL_PREFIX`).

**Interfaces:**
- Consumes: Tasks 5–8.
- Produces:
  ```rust
  pub async fn handle_press(data: &Data, ctx: &serenity::all::Context, i: &ComponentInteraction);
  pub async fn handle_submit(data: &Data, ctx: &serenity::all::Context, i: &ModalInteraction);
  pub(crate) async fn respond_press(cx: &RoomCx<'_>, press: &dyn Press, facts: &GuildFacts, id: ButtonId, presser: UserId, now: Instant);
  pub(crate) fn modal_for(kind: RoomModal, room: ChannelId, facts: &GuildFacts, state: &RoomState, presser: UserId) -> Result<CreateModal<'static>, RoomRefusal>;
  pub struct Submitted { pub member: Option<UserId>, pub role: Option<RoleId>, pub reason: Option<String>, pub value: Option<String>, pub mode: Option<String> }
  pub fn read_submit(i: &ModalInteraction) -> Submitted;                    // pull fields by `ids::field` names
  pub fn action_for(kind: RoomModal, s: &Submitted) -> Result<RoomAction, RoomRefusal>;
  pub(crate) async fn respond_submit(cx: &RoomCx<'_>, press: &dyn Press, facts: &GuildFacts, id: ModalId, s: Submitted, submitter: UserId, now: Instant);
  ```
  Press flow:
  1. Parse the id. A bad id, or a guild that doesn't match, → private `Refused`.
  2. Buttons that need input (Kick, Mute, Unmute, LetIn, Limit, Rename, Transfer, Helpers, WhoCanJoin): run a **pre-check** of `may` for a representative action of that kind. Refused → a private `respond` (a first response, ephemeral). Allowed → `open_modal(modal_for(..))` as the first response. Never acknowledge first.
  3. Lock, Unlock, ChatLock, ChatUnlock, Claim: `acknowledge` → `act` → a private followup only on refusal (the redrawn panel is the answer).
  4. Help: a private `respond(Guide)`.

  Modals:
  - Kick: string select of `facts.humans_in(room)` minus the presser (labels from `facts.display`, values = user ids, max 25 → else `TooManyForMenu`), plus a required paragraph REASON (`min_length(1)`, `max_length(200)`).
  - Mute: the same select, plus an optional reason.
  - Unmute: the select is limited to `state.muted`; LetIn's to `state.blocked`.
  - Transfer: the occupants select.
  - Limit: a VALUE text input, 1–2 chars. Rename: VALUE, 1–100.
  - Helpers: MODE select (`add`/`remove`) + a user select MEMBER.
  - WhoCanJoin: MODE (`allow-user`/`allow-role`/`block-role`/`unblock-role`) + optional user select MEMBER + optional role select ROLE.

  Submit: `acknowledge` (a deferred update; `act` makes Discord calls and may exceed the 3 s window) → read fields → `action_for` → re-gather facts → `act` → private `followup` with `Done` or `Refused`.

- [ ] **Step 1: Write failing tests** (`FakePress`, fakes):
  - `kick_button_opens_a_modal_as_first_response` (ops == `[Modal("roomm:kick:<room>")]`, no Acknowledge before it).
  - `kick_button_from_an_outsider_refuses_privately_without_modal`.
  - `kick_modal_lists_only_humans_in_the_room_minus_presser`.
  - `kick_modal_requires_a_reason` (the built `CreateModal` serialized with `serde_json::to_value`, test-only inspection of a third-party payload, has the reason input `required: true`, `max_length: 200`).
  - `lock_button_acknowledges_acts_and_redraws`.
  - `help_answers_privately`.
  - `action_for_*`: missing member → `MissingChoice`; limit `"100"` or `"abc"` → `BadLimit`; empty reason → `ReasonRequired`; WhoCanJoin `allow-role` without a role → `MissingChoice`; Rename `""` → `BadName`.
  - `submit_after_room_deleted_refuses_and_sends_nothing` (Review Focus 4).
  - `submit_after_transfer_rechecks_authority` (Review Focus 4).
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (acknowledge before the modal; skip the re-check on submit).
- [ ] **Step 5:** Route in `handlers/serenity.rs` beside `np:` (Modal routing is new: add an arm for `Interaction::Modal(mi)` whose custom id starts with `ROOM_MODAL_PREFIX`). **Commit** `feat(rooms): buttons and modals`.

---

### Task 10: `/room` and `/rooms`

**Files:** Create `crack-core/src/commands/rooms/{mod.rs,room.rs,admin.rs}`. Modify `crack-core/src/commands/mod.rs` (`pub mod rooms;` and `.chain(rooms::commands())` in `all_commands`).

**Interfaces:**
- Consumes: `ops::act`, `facts::gather`, `card`, `preflight::for_guild`, `db::rooms`, registry settings.
- Produces: `pub fn commands() -> Vec<crate::Command>` returning `room()` and `rooms()`.
  - `/room` (`guild_only`, `slash_command`, **no** `default_member_permissions`), subcommands per spec 1.7: `kick member reason` (reason a required `String`), `letin member`, `mute member reason?`, `unmute member`, `lock`, `unlock`, `chatlock`, `chatunlock`, `limit n` (`#[min = 0] #[max = 99] u8`), `rename name`, `transfer member`, `helper` with `add member` / `remove member`, `allow` with `member` / `role` subcommands, `block role`, `unblock role`, `claim`, `panel`. Each resolves the room with `pub(crate) fn resolve_room(facts: &GuildFacts, registry: &RoomRegistry, invoker: UserId, channel: GenericChannelId) -> Option<ChannelId>`: the invoker's voice channel if it is a room, else the invocation channel if it is a room, else none (→ `NotInRoom`). It then calls `act` and replies ephemerally with `Done`/`Refused`.
  - `/rooms` (`guild_only`, `default_member_permissions = "MANAGE_GUILD"`, `required_permissions = "MANAGE_GUILD"`): `setup hub` (voice channel option; sets `enabled: true, hub`, validates, saves to DB and registry, replies `Status`), `off` (`enabled: false`, saves, `Switched { on: false }`; live rooms keep running until empty), `status` (`Status` with `preflight::for_guild` and the dashboard link `"{WEB_PUBLIC_ORIGIN}/g/{guild}/settings"` when that env var is set).

- [ ] **Step 1: Write failing tests:** `room_is_a_registered_guild_command_open_to_everyone` (`default_member_permissions` empty, `guild_only`, registered, subcommand names exactly the list above); `rooms_is_manage_guild_only` (pattern of `np_buttons.rs` tests); `kick_subcommand_reason_is_required` (the `reason` parameter's `required` flag); `resolve_room_prefers_voice_then_chat_then_none`.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (`required: false` on reason) → caught. Also run the existing `every_registered_command_is_reachable`.
- [ ] **Step 5: Commit** `feat(rooms): /room and /rooms`.

---

### Task 11: Dashboard Settings (rooms)

**Files:** Create `crack-web/src/settings.rs`, `crack-web/assets/settings.js`. Modify `crack-web/src/backend.rs`, `crack-web/src/lib.rs` (`LiveBackend`), `crack-web/src/routes.rs` (routes + asset), `crack-web/src/page.rs` (`settings_page`, Settings link next to History), `crack-web/src/test_support.rs` (`FakeBackend`), `crack-web/assets/app.css`.

**Interfaces:**
- Consumes: `RoomSettings`, `SettingsError`, `Finding`, `render_room_name`, `GRACE_CHOICES` from crack-core.
- Produces:
  ```rust
  // settings.rs
  #[derive(Debug, Clone, Serialize)] pub struct ChannelOption { pub id: String, pub name: String }
  #[derive(Debug, Clone, Serialize)] pub struct RoomSettingsView { pub enabled: bool, pub hub: Option<String>, pub name_template: String,
      pub default_limit: u8, pub empty_grace_secs: u16, pub voice_channels: Vec<ChannelOption>, pub findings: Option<Vec<String>> /* rendered */ }
  #[derive(Debug, Clone, Deserialize)] pub struct RoomSettingsForm { pub enabled: bool, pub hub: Option<String>, pub name_template: String,
      pub default_limit: u8, pub empty_grace_secs: u16 }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SaveRefused { Invalid(SettingsError), UnknownChannel, Failed }
  // Backend gains
  fn room_settings(&self, g: GuildId) -> impl Future<Output = RoomSettingsView> + Send;
  fn save_room_settings(&self, user: UserId, g: GuildId, f: RoomSettingsForm) -> impl Future<Output = Result<RoomSettingsView, SaveRefused>> + Send;
  ```
  Routes:
  - `GET /g/{guild}/settings` → `settings_page`. Access is `history_access`, the same Manage Server rule; a comment says so. Hidden → 404, Unavailable → 503, Forbidden → `forbidden_page`.
  - `POST /g/{guild}/settings/rooms`: JSON only (`is_json`, else 415, as `/control`), the same access → `Json(view)`, or `error_json(400, message)` for `Invalid`/`UnknownChannel`, 503 for `Failed`.

  `hub` must parse to a voice channel of this guild (LiveBackend checks the cache) → else `UnknownChannel`. `LiveBackend::save_room_settings` validates with `has_db = data.database_pool.is_some()`, saves to DB, updates the registry, and logs `user` at info.

  The page is server-rendered: escaped values, a `<select>` of voice channels, a template input with a preview `<output>` that `settings.js` fills via the same rule (`{name}` → "Saucey"), and a limit number input. Grace is a `<select>` of `GRACE_CHOICES`. The findings list or "ready" line comes from the same strings as `/rooms status`, plus a link to the guide on GitHub (`docs/rooms-and-mod-log.md`). `settings.js`: DOM APIs only (Trusted Types CSP), `fetch` POST with `Content-Type: application/json`, shows the result or the error text. The queue page shows "Settings" beside "History" under the same condition (`history_link`).

- [ ] **Step 1: Write failing route tests** (pattern of the existing `routes.rs` tests with `FakeBackend`):
  - `settings_page_404s_for_non_members_403s_for_non_managers` and `settings_page_lists_voice_channels_and_current_values`.
  - `posting_settings_requires_json`.
  - `a_manager_saves_and_the_backend_gets_exactly_that_form` (assert the recorded form).
  - `invalid_settings_are_400_with_the_reason`.
  - `queue_page_links_settings_only_for_managers`.
  - `settings_js_is_served_as_javascript`.
- [ ] **Step 2–4:** Fail → implement → pass → sabotage (skip the access check on POST) → caught.
- [ ] **Step 5: Commit** `feat(web): dashboard settings for voice rooms`.

---

### Task 12: Docs, changelog, version

**Files:** Create `docs/rooms-and-mod-log.md` (rooms part; a "Mod log — coming in v0.29.0" stub heading). Modify `CHANGELOG.md` (`## Unreleased` → `### Added`), `Cargo.toml` (`[workspace.package] version = "0.28.0"`), `Cargo.lock` (via `cargo check`).

- [ ] **Step 1:** Write the guide. For admins: setup via the dashboard or `/rooms setup`, the permission table from preflight, and where to grant them (the hub's category). For room owners: the same list as the panel. Then what gets deleted and when, and why there is no deafen.
- [ ] **Step 2:** CHANGELOG entry: what members see, what admins do, and that it is off by default.
- [ ] **Step 3:** Bump the version; `cargo check --workspace` updates `Cargo.lock`.
- [ ] **Step 4: Full gate:** fmt check, clippy (as in Global Constraints), and `cargo test --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests` against the local postgres. Record the pass count.
- [ ] **Step 5: Commit** `v0.28.0: voice rooms` (docs + changelog + version).

---

## TuneTitan checklist (after the PR is green; for the owner's hand test)

Spec section 5 plus the glue the tests cannot see:
1. `/rooms setup` with a hub; `/rooms status` shows the missing permissions until they are granted.
2. Join the hub → a room is created in the category, synced to it, you are moved in, and the panel is posted. Is it pinned?
3. Kick from the panel: a modal with a member select and a required reason. On desktop and on mobile.
4. Mute from the panel: is the muted person silenced at once while still connected?
5. Lock with an 18+-style category: a non-permitted member cannot join; the owner and helpers can.
6. Everyone leaves → the room is gone after the grace. Restart the bot with one empty and one occupied room → empty deleted, occupied kept.
7. Delete a room by hand → no error spam; `/room` in it afterwards says "not in a room".
8. The dashboard Settings page: save, reload, values persist; a non-manager gets 403.
