# Neutral ids, PR 1 (refs, settings, dashboard) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Server settings and the dashboard backend hold platform-neutral refs instead of serenity ids or bare Discord numbers. Nothing a user sees or the bot stores changes.

**Architecture:**
- **`crack-model`**, a new workspace crate with no platform dependency, holds `Ref<K>` and its kinds.
- **`crack_types::discord`** is the one place serenity ids and refs meet. It holds `ToRef`, checked back-conversions, snowflake helpers, and serde adapters that keep today's bare-number JSON.
- **Settings types hold refs.** The database layer converts refs to and from the unchanged `BIGINT` columns.
- **crack-web's `Backend` trait speaks refs**, and its Discord-backed implementation converts at the edge.

**Tech Stack:** Rust 2021 workspace, serde, thiserror, serenity (next branch), sqlx 0.9 (offline `.sqlx`), axum (crack-web), cargo-nextest, cargo-mutants.

**Spec:** `docs/superpowers/specs/2026-10-24-neutral-ids-and-messages-design.md`. §1–§4 and "Calls settled after the builder check" are this plan's authority. §5 is PR 2's and is out of scope here.

## Global Constraints

- **No database change.** No migration, and no SQL text change except deleting the `INSERT INTO authorized_users` query (Task 3). Every id column stays `BIGINT`/`BIGINT[]`.
- **No wire or user-visible change:**
  - Every type converted here serialises exactly as before: an id is a bare JSON number, a set is an array of numbers.
  - Dashboard URLs, JSON and HTML are unchanged.
  - The only allowed difference is `/settings get all`'s `{:?}` dump.
- **No panic on an id.** serenity's `GuildId::new` (and every id type's `new`) panics on `0` and `u64::MAX`. Every conversion from storage, config or a URL goes through `crack_types::discord::from_snowflake` or the checked back-conversions.
- **Snowflake rule:** canonical decimal digits (no sign, no leading zero, no whitespace), parsing to a `u64` with `1 <= n < u64::MAX`.
- **The default ignored channel stays:** `GuildSettings::new` seeds `ignored_channels` with `1165246445654388746`.
- **Refs at the seams only.** `Data`'s `GuildSettingsOperations` methods, `GuildSettingsMap` (`HashMap<GuildId, GuildSettings>`) and the poise command layer keep serenity ids and convert inside.
- **New crate rules:** `crack-model` declares `mutation-testing = []`, uses `version.workspace = true`, and copies `crack-types`' manifest metadata. Its normal dependencies are `serde` (derive) and `thiserror` only.
- **Memory cap on every local cargo test, nextest, mutants or clippy run:** prefix with `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue --`. systemd-run expands `$VARS`, so write paths in full.
- **Test database:** a Postgres at `postgresql://postgres:mysecretpassword@localhost:5432/postgres`. The ctor in `crack-core/src/db/user.rs` forces this URL. Check it with `docker --context default exec pg pg_isready`.
- **Verification commands** (all under the cap):
  - suite: `cargo nextest run --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - doctests: `cargo test --doc --workspace --features crack-core/db-tests,cracktunes/db-tests,crack-voting/db-tests,crack-core/crack-coin`
  - lint (CI's): `cargo fmt --all -- --check`; `cargo clippy --all -- -D clippy::all -D warnings --allow clippy::needless_return`; `cargo check -p crack-core --features crack-bf,crack-osint,crack-coin --locked`; `cargo clippy -p crack-core --features crack-coin --locked -- -D warnings`; `python3 scripts/check_waivers.py`
  - mutants: `python3 scripts/mutants_check.py --base <the commit the task started from>`. Exit 1 lists NEW misses: catch each with a test, or waive it with `#[cfg_attr(test, mutants::skip)]` plus a `//` reason on the line directly above.
- **Hand-sabotage what cargo-mutants can't mutate:** SQL text, plain field assignments, attribute arguments such as `#[serde(with = …)]`. Break it, watch a test fail, restore it.
- **Commits:**
  - never `git add -A`; add files by name;
  - end every message with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line;
  - `cargo fmt` before committing.
- **Never** run `cargo clean`, delete `~/.cache/cargo-target`, or pass `--in-place` to cargo-mutants.

## Review Focus

1. **A stored garbage id** (`0`, a negative number, `u64::MAX`) in a `BIGINT`/`BIGINT[]` settings column. Loading drops it with a `warn!` naming the column and keeps the rest; a bad `guild_id` returns an error. Never a panic. Tests: Task 4, Task 5.
2. **`/settings set welcome`'s reply** is `WelcomeSettings`' `Display` (pretty JSON) and must not change by a byte. Test: Task 4.
3. **Dashboard URLs at the edges.** `/g/0`, `/g/abc` and `/g/18446744073709551616` answer as today (404). `/g/18446744073709551615` (`u64::MAX`) answers 404, where today it would reach `GuildId::new` and panic. Test: Task 6.
4. **Music permissions.** A user in `denied_users` is still refused through `Data::check_music_permissions(GuildId, UserId)`, and a guild with no music permissions still allows everyone. Test: Task 4.
5. **Old settings JSON** (the `guild_settings_map` config option, or anything serialised before) still deserialises to the same values. That covers numeric ids, `ignored_channels` as numbers, and the removed `authorized_users`/`authorized_groups` keys still present. Tests: Task 3, Task 5.

---

### Task 1: The `crack-model` crate and its refs

**Files:**
- Create: `crack-model/Cargo.toml`, `crack-model/src/lib.rs`, `crack-model/src/ids.rs`
- Modify: `Cargo.toml` (workspace `members`), `Cargo.lock`, `.github/workflows/lint.yml` (one step)

**Interfaces:**
- Produces, from `crack_model` (re-exported at the crate root by `pub mod ids; pub use ids::*;`):
  - `#[non_exhaustive] pub enum Platform { Discord }`: serde `"discord"`, `pub const fn as_str(self) -> &'static str`, `FromStr<Err = RefError>`.
  - `pub trait Kind: sealed::Sealed + 'static { const NAME: &'static str; }`
  - The kinds `User`, `Community`, `Room`, `Message`, `Role`: uninhabited enums deriving `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`. `NAME` is the kind's name. The aliases are `UserRef`, `CommunityRef`, `RoomRef`, `MessageRef` and `RoleRef`.
  - `pub struct RawId(Arc<str>)` with `pub fn as_str(&self) -> &str`.
  - `#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)] pub struct Ref<K: Kind> { platform: Platform, raw: RawId, kind: PhantomData<K> }`. Field order gives ordering by platform, then raw text.
  - `impl<K: Kind> Ref<K>`:
    - `pub fn new(platform: Platform, raw: impl AsRef<str>) -> Result<Self, RefError>`
    - `pub fn platform(&self) -> Platform`
    - `pub fn raw(&self) -> &RawId`
    - `pub fn raw_for(&self, platform: Platform) -> Result<&str, RefError>`
    - `pub fn key(&self) -> String`
  - `Debug` is `"{Kind}({key})"`, `Display` is the key, and `FromStr<Err = RefError>` parses a key. serde is `{"platform": …, "id": …}`, and deserialising goes through `new`.
  - `#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub enum RefError`, with variants:
    - `BadRaw { kind: &'static str, raw: String }`
    - `BadKey { kind: &'static str, key: String }`
    - `UnknownPlatform(String)`
    - `WrongPlatform { kind: &'static str, expected: Platform, found: Platform }`

- [ ] **Step 1: Scaffold the crate.**
  - Create the manifest. `[features] mutation-testing = []`; dependencies `serde = { version = "1.0", features = ["derive"] }` and `thiserror = "2.0.20"`; dev-dependency `serde_json = "1.0"`.
  - Add `"crack-model"` to the root `members`.
  - Port the shape from `~/projects/transpose/crates/core/src/ids.rs`. Drop `Lifecycle`, `Thread`, `from_stored`, `parts`, and the Matrix, Stoat and Telegram variants.
  - Write the failing tests first, in `ids.rs`'s `#[cfg(test)] mod tests`:

```rust
#[test] fn a_ref_round_trips_through_its_key() {
    let r: UserRef = "discord:42".parse().unwrap();
    assert_eq!((r.platform(), r.raw().as_str(), r.key()), (Platform::Discord, "42", "discord:42".to_string()));
}
#[test] fn serde_is_the_platform_id_pair() {
    let r = UserRef::new(Platform::Discord, "42").unwrap();
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(json, r#"{"platform":"discord","id":"42"}"#);
    assert_eq!(serde_json::from_str::<UserRef>(&json).unwrap(), r);
}
#[test] fn an_empty_or_control_raw_is_refused() {
    assert!(matches!(UserRef::new(Platform::Discord, ""), Err(RefError::BadRaw { kind: "User", .. })));
    assert!(matches!(UserRef::new(Platform::Discord, "4\n2"), Err(RefError::BadRaw { .. })));
}
#[test] fn a_bad_key_is_refused() {
    assert!(matches!("42".parse::<UserRef>(), Err(RefError::BadKey { kind: "User", .. })));
    assert_eq!("slack:42".parse::<UserRef>().unwrap_err(), RefError::UnknownPlatform("slack".into()));
    assert!(matches!("discord:".parse::<UserRef>(), Err(RefError::BadRaw { .. })));
}
#[test] fn deserialising_a_bad_ref_fails() {
    assert!(serde_json::from_str::<UserRef>(r#"{"platform":"discord","id":""}"#).is_err());
    assert!(serde_json::from_str::<UserRef>(r#"{"platform":"slack","id":"1"}"#).is_err());
}
#[test] fn raw_for_returns_the_raw_id_on_its_platform() {
    assert_eq!(RoomRef::new(Platform::Discord, "7").unwrap().raw_for(Platform::Discord), Ok("7"));
}
#[test] fn debug_names_the_kind_and_display_is_the_key() {
    let r = CommunityRef::new(Platform::Discord, "5").unwrap();
    assert_eq!((format!("{r:?}"), r.to_string()), ("Community(discord:5)".into(), "discord:5".into()));
}
#[test] fn refs_order_by_raw_text_and_dedupe_when_equal() {
    let set: std::collections::BTreeSet<UserRef> =
        ["discord:2", "discord:10", "discord:1", "discord:2"].iter().map(|k| k.parse().unwrap()).collect();
    let keys: Vec<String> = set.iter().map(Ref::key).collect();
    assert_eq!(keys, ["discord:1", "discord:10", "discord:2"]);
}
```

- [ ] **Step 2: Run the tests and see them fail.**
  `cargo nextest run -p crack-model` (under the cap) fails to compile, because `Ref` does not exist yet.
- [ ] **Step 3: Implement `crack_model::ids` to the interface above.**
- [ ] **Step 4: Run the tests and see them pass.**
  `cargo nextest run -p crack-model` passes 8 tests.
- [ ] **Step 5: Add the platform-free check to `lint.yml`'s fmt-and-clippy job.**
  - The step is named `crack-model depends on no platform crate`.
  - It fails when `cargo tree -p crack-model -e normal --prefix none` lists a line starting with `serenity`, `poise`, `songbird`, `sqlx` or `reqwest`.
  - Prove it can fail: add `serenity = { workspace = true }` to crack-model locally, run the step's command and watch it exit non-zero, then revert.
- [ ] **Step 6: Run lint and mutants, then commit.**
  Run the lint commands and `mutants_check.py --base <task base>`. Then commit with `git add Cargo.toml Cargo.lock crack-model .github/workflows/lint.yml` and the message `feat(model): crack-model with platform-neutral refs`.

### Task 2: `crack_types::discord`, where serenity ids and refs meet

**Files:**
- Create: `crack-types/src/discord.rs`
- Modify: `crack-types/src/lib.rs` (`pub mod discord;`), `crack-types/Cargo.toml` (`crack-model = { path = "../crack-model" }`), `Cargo.lock`

**Interfaces:**
- Consumes: Task 1's `crack_model::{Ref, Kind, Platform, RefError, CommunityRef, UserRef, RoomRef, RoleRef, MessageRef}`.
- Produces, in `crack_types::discord`:
  - `pub trait ToRef { type Kind: Kind; fn to_ref(&self) -> Ref<Self::Kind>; }`. It is implemented for:
    - `GuildId` → `Community`
    - `UserId` → `User`
    - `ChannelId` → `Room`
    - `GenericChannelId` → `Room`
    - `RoleId` → `Role`
    - `MessageId` → `Message`
  - `#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)] pub enum IdError { WrongPlatform { kind: &'static str, found: Platform }, NotASnowflake { kind: &'static str, raw: String } }`
  - `pub fn snowflake<K: Kind>(r: &Ref<K>) -> Result<u64, IdError>`
  - `pub fn from_snowflake<K: Kind>(id: u64) -> Result<Ref<K>, IdError>`
  - Checked back-conversions, each `-> Result<_, IdError>`:
    - `guild_id(&CommunityRef) -> Result<GuildId, IdError>`
    - `user_id(&UserRef) -> Result<UserId, IdError>`
    - `channel_id(&RoomRef) -> Result<ChannelId, IdError>`
    - `generic_channel_id(&RoomRef) -> Result<GenericChannelId, IdError>`
    - `role_id(&RoleRef) -> Result<RoleId, IdError>`
    - `message_id(&MessageRef) -> Result<MessageId, IdError>`
  - serde adapter modules, for `#[serde(with = "crack_types::discord::<module>")]`. Each serialises bare `u64` numbers and fails deserialisation on a non-snowflake:
    - `as_snowflake` for `Ref<K>`
    - `as_snowflake_opt` for `Option<Ref<K>>`
    - `as_snowflake_set` for `HashSet<Ref<K>>`
  - Wherever a field was optional before, its serde `default` stays.

- [ ] **Step 1: Write the failing tests** in `discord.rs`'s tests:

```rust
#[test] fn a_guild_id_round_trips() {
    let g = GuildId::new(81384788765712384);
    assert_eq!(g.to_ref().key(), "discord:81384788765712384");
    assert_eq!(guild_id(&g.to_ref()), Ok(g));
}
#[test] fn every_id_kind_round_trips() {
    assert_eq!(user_id(&UserId::new(7).to_ref()), Ok(UserId::new(7)));
    assert_eq!(channel_id(&ChannelId::new(8).to_ref()), Ok(ChannelId::new(8)));
    assert_eq!(generic_channel_id(&GenericChannelId::new(9).to_ref()), Ok(GenericChannelId::new(9)));
    assert_eq!(role_id(&RoleId::new(10).to_ref()), Ok(RoleId::new(10)));
    assert_eq!(message_id(&MessageId::new(11).to_ref()), Ok(MessageId::new(11)));
}
#[test] fn zero_max_overflow_and_junk_are_not_snowflakes() {
    for raw in ["0", "18446744073709551615", "18446744073709551616", "-1", "+1", "12a", "007", " 1"] {
        let r = UserRef::new(Platform::Discord, raw).unwrap();
        assert_eq!(snowflake(&r), Err(IdError::NotASnowflake { kind: "User", raw: raw.into() }), "{raw}");
        assert!(user_id(&r).is_err(), "{raw}");
    }
    assert!(from_snowflake::<User>(0).is_err());
    assert!(from_snowflake::<User>(u64::MAX).is_err());
    assert_eq!(from_snowflake::<User>(42).unwrap().key(), "discord:42");
}
#[test] fn serde_adapters_keep_bare_numbers() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct S {
        #[serde(with = "as_snowflake")] g: CommunityRef,
        #[serde(with = "as_snowflake_opt", default)] c: Option<RoomRef>,
        #[serde(with = "as_snowflake_set")] s: HashSet<RoleRef>,
    }
    let s = S { g: GuildId::new(5).to_ref(), c: None, s: [RoleId::new(7).to_ref()].into() };
    assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"g":5,"c":null,"s":[7]}"#);
    assert_eq!(serde_json::from_str::<S>(r#"{"g":5,"s":[7]}"#).unwrap(), s);
    assert!(serde_json::from_str::<S>(r#"{"g":0,"s":[]}"#).is_err());
    assert!(serde_json::from_str::<S>(r#"{"g":5,"s":[0]}"#).is_err());
}
```

- [ ] **Step 2: Run them and see them fail** with `cargo nextest run -p crack-types discord` (under the cap): compile errors.
- [ ] **Step 3: Implement `crack_types::discord`** to the interface. `snowflake` applies the Global Constraints' snowflake rule to `raw_for(Platform::Discord)` (a non-Discord ref gives `WrongPlatform`). The back-conversions call `snowflake`, then serenity's `new`.
- [ ] **Step 4: Run them and see them pass** with `cargo nextest run -p crack-types`. All pass.
- [ ] **Step 5: Run lint and mutants, then commit** as `feat(types): crack_types::discord converts serenity ids and refs`.

### Task 3: Delete the dead authorization code

The spec's "what the check found", item 1: nothing reads it, and the loader resets it.

**Files:**
- Modify `crack-core/src/guild/settings.rs`:
  - remove the `authorized_users` and `authorized_groups` fields, their `PartialEq` lines, `new()` inits and `// FIXME` loader lines;
  - remove `authorized_users_default`, `authorize_user`, `deauthorize_user`, the six `check_*` methods, `MOD_VAL`/`ADMIN_VAL` once they are unused, and `UserPermission` with its impl (no users outside this file).
- Delete: `crack-core/src/commands/admin/authorize.rs`, `crack-core/src/commands/admin/deauthorize.rs`.
- Modify `crack-core/src/commands/admin/mod.rs`: remove their `mod`/`use` lines and the commented `"authorize"`/`"deauthorize"` subcommand entries.
- Modify `crack-core/src/messaging/message.rs`, `messages.rs`: remove `CrackedMessage::UserAuthorized` and `UserDeauthorized`, their `Display` arms, and any message constant only they use.
- Modify `crack-core/src/db/guild.rs`: remove the `authorized_users` loop in `write_settings`.
- Modify `crack-core/src/lib.rs`: remove `DataInner::authorized_users`, its `Display` line and its default.
- Delete `.sqlx/query-*.json` for the removed query. It is the one file whose `"query"` contains `INSERT INTO authorized_users`; find it with `grep -l 'INSERT INTO authorized_users' .sqlx/*.json`.
- **The tables and migrations stay.**

**Interfaces:**
- Produces: `GuildSettings` without `authorized_users`/`authorized_groups`. Later tasks must not reintroduce them.

- [ ] **Step 1: Write the failing test** `settings_json_with_the_removed_authorization_keys_still_loads` in settings.rs's tests:
  - Serialise `GuildSettings::new(GuildId::new(123), None, None)` (Task 5 changes this constructor; use today's) to a `serde_json::Value`.
  - Insert `"authorized_users": {"1": 2}` and `"authorized_groups": {}`.
  - Deserialise it back. It must equal the original.
  - Assert `!serde_json::to_string(&settings).unwrap().contains("authorized")`.
- [ ] **Step 2: Run it and see it fail** with `cargo nextest run -p crack-core settings_json_with_the_removed`. It fails on the `contains("authorized")` assertion.
- [ ] **Step 3: Delete everything listed under Files.** `cargo build --workspace` under the cap must then show no `dead_code` or `unused` warnings.
- [ ] **Step 4: Run the full suite and see it pass.** All pass; the count is lower only by tests of deleted code, and each one is named in the report.
- [ ] **Step 5: Run lint and mutants, then commit** as `refactor(settings): drop the never-loaded authorization fields and their commands`.

### Task 4: Nested settings hold refs

These are the log channels, the welcome settings and the music permissions.

**Files:**
- Modify `crack-core/src/guild/settings.rs`:
  - `LogSettings`, `WelcomeSettings` and their `From<…Read>`. These stay infallible `From`: a garbage id is dropped (see Step 3).
  - The `GuildSettings` methods around them: `get_music_channel`, `set_music_channel`, `get_log_channel`, `get_all_log_channel`, `get_join_leave_log_channel`, `set_all_log_channel`, `with_join_leave_log_channel`, `set_join_leave_log_channel`, `set_welcome_settings2`, `set_welcome_settings3`, `with_auto_role`, `set_auto_role`, `add_denied_music_user`.
  - Remove `get_log_channel_type_fe`.
- Modify `crack-core/src/guild/permissions.rs`: `GenericPermissionSettings`, its `From<…Read>`, `CommandChannel`, `CommandChannelRead` conversions. `command_check_music` moves out to `crack-core/src/commands/permissions.rs`, unchanged.
- Modify `crack-core/src/handlers/event_log.rs`: gains `log_channel_for(settings: &GuildSettings, event: &FullEvent) -> Option<RoomRef>`, the moved `get_log_channel_type_fe` body, and its test.
- Modify `crack-core/src/db/guild.rs`: `write_log_settings`, `write_welcome_settings`.
- Modify the callers listed by `grep -rnE 'get_(all|server|join_leave|member|voice)_log_channel|set_(all|server|join_leave|member|voice)_log_channel|with_join_leave_log_channel|with_channel_id|with_auto_role|set_auto_role|set_welcome_settings|get_log_channel|music_channel|get_music_permissions|add_denied_music_user|is_(user|role|channel)_allowed|auto_role' crack-core/src`:
  - `guild/operations.rs`, `lib.rs` (`check_music_permissions`, `add_denied_music_user`), `handlers/serenity.rs`, `handlers/track_end.rs`, `messaging/status.rs`, `messaging/buttons.rs`, `music/resume.rs`, `commands/permissions.rs`, `commands/music/nowplaying.rs`, `commands/settings/**`.

**Interfaces:**
- Consumes: Task 2's `ToRef`, `from_snowflake`, `snowflake`, `generic_channel_id`, `role_id`, `as_snowflake_opt`, `as_snowflake_set`.
- Produces:
  - `LogSettings`' six channel fields are `Option<RoomRef>` (serde `as_snowflake_opt`).
  - `WelcomeSettings { channel_id: Option<RoomRef>, message, auto_role: Option<RoleRef>, password }`.
  - `LogSettings::get_*_log_channel(&self) -> Option<RoomRef>` and `set_*_log_channel(&mut self, channel: RoomRef) -> &mut Self`.
  - `WelcomeSettings::with_channel_id(self, RoomRef)` and `with_auto_role(self, RoleRef)`.
  - `GenericPermissionSettings` sets are `HashSet<RoleRef>` (`allowed_roles`, `denied_roles`), `HashSet<UserRef>` (`allowed_users`, `denied_users`) and `HashSet<RoomRef>` (`allowed_channels`, `denied_channels`), each with serde `as_snowflake_set`.
  - `GenericPermissionSettings::is_role_allowed(&self, &RoleRef) -> bool`, `is_user_allowed(&self, &UserRef) -> bool`, `is_channel_allowed(&self, &RoomRef) -> bool`; its `add_*`/`remove_*` take the ref.
  - `CommandChannel { command, channel_id: RoomRef, guild_id: CommunityRef, permission_settings }`. Its `Default` is removed; any user builds one explicitly.
  - `GuildSettings` getters return `Option<RoomRef>`; setters take `RoomRef`/`RoleRef`/`Option<RoleRef>`; `add_denied_music_user(&mut self, user: UserRef)`.
  - **`GuildSettingsOperations` (operations.rs) and `Data`'s methods keep their serenity/`u64` signatures** (`get_music_channel -> Option<GenericChannelId>`, `get_auto_role -> Option<u64>`, `set_auto_role(u64)`, …) and convert inside. A `u64` that is not a snowflake is ignored with a `warn!`.

- [ ] **Step 1: Write the failing tests.**

  In settings.rs:

```rust
#[test]
fn the_welcome_reply_text_is_unchanged() {
    // set_welcome_settings.rs replies with this Display, so its bytes are user-visible.
    let w = WelcomeSettings::new()
        .with_channel_id(GenericChannelId::new(123).to_ref())
        .with_message("hi".into())
        .with_auto_role(RoleId::new(456).to_ref());
    assert_eq!(w.to_string(),
        "{\n  \"channel_id\": 123,\n  \"message\": \"hi\",\n  \"auto_role\": 456,\n  \"password\": null\n}");
}
#[test]
fn log_settings_serialise_channels_as_numbers() {
    let mut l = LogSettings::default();
    l.set_all_log_channel(GenericChannelId::new(9).to_ref());
    let v = serde_json::to_value(&l).unwrap();
    assert_eq!(v["all_log_channel"], 9);
    assert_eq!(serde_json::from_value::<LogSettings>(v).unwrap(), l);
}
#[test]
fn garbage_stored_ids_are_dropped_not_panicked_on() {
    let row = crate::db::LogSettingsRead { /* guild_id: 1, */ all_log_channel: Some(0),
        raw_event_log_channel: Some(-5), server_log_channel: Some(i64::MAX), /* the rest None */ .. };
    let l = LogSettings::from(row);
    assert_eq!(l.get_all_log_channel(), None);
    assert_eq!(l.get_server_log_channel(), Some(from_snowflake(i64::MAX as u64).unwrap()));
}
```

  Before writing it, pin the welcome test's expected string to today's output. Run the old `Display` on `channel_id: Some(123), message: Some("hi"), auto_role: Some(456)` and copy what it prints. If field order or spacing differs from the string above, today's output wins. (The row literal fills `LogSettingsRead`'s real fields; `i64::MAX` is a valid snowflake.)

  In permissions.rs, `music_permissions_still_deny_a_denied_user`:
  - put `UserId::new(7).to_ref()` in `denied_users`, with `default_allow_all_users: true`;
  - assert `!is_user_allowed(&UserId::new(7).to_ref())`;
  - assert `is_user_allowed(&UserId::new(8).to_ref())`;
  - assert a `GenericPermissionSettingsRead` with `denied_users: vec![7, 0]` converts to a set holding only user 7.

  In lib.rs tests (or wherever `Data` is tested), `check_music_permissions_reads_refs`:
  - a `Data` whose guild 5 settings deny user 7;
  - `check_music_permissions(GuildId::new(5), UserId::new(7))` is `false`;
  - user 8 is `true`;
  - guild 6, which has no settings, is `true`.

  In event_log.rs, the moved `test_get_log_channel_type_fe` keeps its assertions, now on `Option<RoomRef>`.
- [ ] **Step 2: Run them and see them fail** with `cargo nextest run -p crack-core --features db-tests settings:: permissions:: event_log`: compile errors or assertion failures.
- [ ] **Step 3: Convert the types to the interface.**
  - Loading from `*Read` rows goes through `from_snowflake`. A failure drops that one id and logs `warn!(column = "<table>.<column>", raw = <value>, "dropped a stored id that is not a snowflake")`.
  - Writing goes through `snowflake(&r)? as i64` inside the existing db functions. No SQL text changes.
- [ ] **Step 4: Update every caller** in the Files list. Discord callers convert with `generic_channel_id(&r)`, `role_id(&r)` and `.to_ref()`.
- [ ] **Step 5: Run the full suite and see it pass.**
- [ ] **Step 6: Hand-sabotage the serde attributes.** Delete `with = "…as_snowflake_opt"` from one `LogSettings` field and watch `log_settings_serialise_channels_as_numbers` fail. Delete it from `WelcomeSettings.channel_id` and watch the welcome test fail. Restore both.
- [ ] **Step 7: Run lint and mutants, then commit** as `refactor(settings): log, welcome and music-permission settings hold refs`.
  `command_check_music` moves unchanged. If its mutants come back NEW only because the file changed (a poise `Context` check no test can drive), waive each one with the reason `live-Discord glue, moved unchanged from guild/permissions.rs`.

### Task 5: `GuildSettings` holds no Discord type

**Files:**
- Modify `crack-core/src/guild/settings.rs`: `guild_id`, `guild_name`, `ignored_channels`, `new`, `new_gid`, the `Default` impl (remove it), `TryFrom<GuildSettingsRead>`, `load_or_create`, `load_if_exists`, `save`, and the serenity, poise and `FixedString` imports.
- Modify `crack-core/src/db/guild.rs`: `write_settings`, `get_settings`, `get_or_create`, and their callers of `GuildSettings::from`.
- Modify `crack-core/src/errors.rs`: add `CrackedError::InvalidId(crack_types::discord::IdError)`, `#[error("invalid stored id: {0}")]`.
- Modify `crack-core/src/handlers/event_log.rs`: the ignored-channel check.
- Modify `crack-core/src/config.rs`: build `guild_settings_map` from the config's `Vec<GuildSettings>`.
- Modify every `GuildSettings::new(`, `new_gid(` and `GuildSettings::default()` site. There are 33 constructor sites, listed by `grep -rn 'GuildSettings::new(\|GuildSettings::new_gid(\|GuildSettings::default()' crack-*`.
- Modify `crack-core/src/lib.rs`: `Data::add_denied_music_user` builds settings for the real guild instead of `GuildSettings::default`.

**Interfaces:**
- Consumes: Tasks 2–4.
- Produces:
  - `GuildSettings { pub guild_id: CommunityRef /* serde as_snowflake */, pub guild_name: String, pub ignored_channels: HashSet<RoomRef> /* serde as_snowflake_set */, .. }`
  - `pub fn new(guild_id: CommunityRef, prefix: Option<&str>, guild_name: Option<String>) -> GuildSettings` and `pub fn new_gid(guild_id: CommunityRef) -> GuildSettings`.
  - `impl TryFrom<GuildSettingsRead> for GuildSettings { type Error = IdError; }`
  - No `impl Default for GuildSettings`.
  - `GuildSettingsMap` and `GuildSettingsMapParam` stay `HashMap<GuildId, GuildSettings>`.
  - **Done when** `crack-core/src/guild/settings.rs` has no `serenity`, `poise` or `small_fixed_array` path outside its `#[cfg(test)]` module. (Test modules may build refs from serenity ids.)

- [ ] **Step 1: Write the failing tests** in settings.rs:

```rust
#[test]
fn settings_json_keeps_its_shape() {
    let s = GuildSettings::new(GuildId::new(123).to_ref(), None, Some("G".into()));
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["guild_id"], 123);
    assert_eq!(v["guild_name"], "G");
    assert_eq!(v["ignored_channels"], serde_json::json!([1165246445654388746u64]));
    assert_eq!(serde_json::from_value::<GuildSettings>(v).unwrap(), s);
}
#[test]
fn a_row_with_a_garbage_ignored_channel_keeps_the_rest() {
    // a GuildSettingsRead for guild 5 whose ignored_channels is vec![0, 77]
    let s = GuildSettings::try_from(row).unwrap();
    assert_eq!(s.ignored_channels, [GenericChannelId::new(77).to_ref()].into());
}
#[test]
fn a_row_with_a_garbage_guild_id_is_an_error() {
    // the same row with guild_id: 0
    assert!(GuildSettings::try_from(row).is_err());
}
```

  In event_log.rs, `an_event_in_an_ignored_channel_is_not_logged`: whatever unit decides "ignored" (today the `contains` at event_log.rs:92; extract a small `fn is_ignored(&GuildSettings, GenericChannelId) -> bool` if none exists) returns `true` for 1165246445654388746 and `false` for 1.

  Existing db-tests round-tripping settings through Postgres (`db/guild.rs` tests) keep their assertions; only construction changes.
- [ ] **Step 2: Run them and see them fail** with `cargo nextest run -p crack-core --features db-tests settings:: event_log db::guild`: compile errors.
- [ ] **Step 3: Implement the interface.**
  - `TryFrom` drops garbage `ignored_channels` entries with the Task 4 `warn!` (`column = "guild_settings.ignored_channels"`) and returns `Err` for a bad `guild_id`.
  - `GuildEntity::get_settings` (returns `Result<_, SerenityError>`) maps that error to `SerenityError::Other("invalid guild_id in guild_settings")`.
  - `CrackedError` paths map it to `InvalidId`.
  - `config.rs` skips a config entry whose `guild_id` fails `guild_id(&…)`, with a `warn!`.
- [ ] **Step 4: Update every call site:** `GuildSettings::new(guild_id.to_ref(), prefix, name.map(|n| n.to_string()))`. Test sites name a guild explicitly. `operations.rs` keeps its signatures.
- [ ] **Step 5: Run the full suite and see it pass.**
- [ ] **Step 6: Hand-sabotage.** Drop `with = "…as_snowflake"` from `guild_id` and watch `settings_json_keeps_its_shape` fail. In `new`, seed `ignored_channels` empty and watch the same test fail. Restore both.
- [ ] **Step 7: Run lint and mutants, then commit** as `refactor(settings): GuildSettings holds a CommunityRef, a String name and RoomRefs`.

### Task 6: The dashboard backend speaks refs

**Files:**
- Modify the crack-web sources: `crack-web/src/backend.rs`, `routes.rs`, `access.rs`, `watch.rs`, `hits.rs`, `page.rs`, `find.rs`, `settings.rs`, `history.rs` (if it names a guild), `lib.rs`, `test_support.rs`.
- Modify `crack-web/Cargo.toml`: `crack-model = { path = "../crack-model" }`.

**Interfaces:**
- Consumes: Task 1's refs; Task 2's `ToRef`, `from_snowflake`, `guild_id`, `user_id`, `channel_id`.
- Produces:
  - **`Backend`:** every `g: GuildId` parameter becomes `g: CommunityRef`, every `user: UserId`/`u: UserId` becomes `UserRef`, and both are taken by value as today.
  - **`GuildEntry`:** `GuildEntry { id: CommunityRef, name, channel }`.
  - **`Presence`:** `Presence { membership, user_channel: Option<RoomRef>, bot_channel: Option<RoomRef> }`.
  - **Routes:** `pub(crate) fn parse_guild(raw: &str) -> Option<CommunityRef>` parses a `u64` (accepting what `NonZeroU64::from_str` accepts today) and goes through `from_snowflake`. `pub(crate) fn user_id(s: Session) -> Option<(UserRef, String)>`.
  - **The hub:** `ViewSource::view(&self, guild: CommunityRef)`, with `Hub` keyed by `CommunityRef`.
  - **The memo:** `HitMemo` keyed by `(UserRef, CommunityRef)`.
  - **Pages:** `page.rs` functions take `&CommunityRef` and write `g.raw().as_str()` wherever they wrote the guild id.
  - **The Discord-backed `Backend` in `lib.rs`** converts with `guild_id(&g)`/`user_id(&u)`. A conversion error answers as that method's "not found/hidden" outcome does today. crack-core APIs keep their `GuildId` signatures.
  - **What stays serenity:**
    - **Allowed:** serenity may remain only in `lib.rs`, in `access.rs`'s cache and HTTP glue that builds a `Presence`, in `settings.rs`'s economy and room form code (their stored types are out of scope), and in tests.
    - **Done when** `backend.rs`, `routes.rs`, `watch.rs`, `hits.rs`, `page.rs` and `find.rs` import no `serenity` outside `#[cfg(test)]`.

- [ ] **Step 1: Write the failing test** `an_out_of_range_guild_is_not_found` in routes.rs tests:
  - `parse_guild("18446744073709551615")` is `None`, as are `"0"`, `"abc"` and `"18446744073709551616"`.
  - `parse_guild("5")` is `Some` with key `"discord:5"`.
  - A GET of `/g/18446744073709551615`, through the test router with a signed-in session, answers `404`. The existing `/g/0` test shows the pattern.
- [ ] **Step 2: Run it and see it fail** with `cargo nextest run -p crack-web parse_guild out_of_range`: a compile error, or `Some`.
- [ ] **Step 3: Convert crack-web to the interface.** Existing route, page, hub and memo tests keep their assertions on status codes, HTML and JSON; only their construction changes.
- [ ] **Step 4: Run the full suite and see it pass.**
- [ ] **Step 5: Run lint and mutants, then commit** as `refactor(web): the dashboard backend speaks refs`.

### Task 7: Version, changelog, final verification

**Files:**
- Modify: `Cargo.toml` (`[workspace.package] version = "0.37.0"`), `Cargo.lock`, `CHANGELOG.md`

- [ ] **Step 1: Bump the version.** Set it to `0.37.0` and run `cargo update --workspace` under the cap, so `Cargo.lock` carries the new member versions.
- [ ] **Step 2: Update the changelog.** Under `## Unreleased`, add a `### Internal` section after `### Fixed`, with three bullets:
  - **`crack-model`**, a new crate with no platform dependency, holds platform-neutral refs (`UserRef`, `CommunityRef`, `RoomRef`, `RoleRef`, `MessageRef`), and `crack_types::discord` converts them to and from serenity ids. CI checks that `crack-model` stays free of platform crates.
  - **Server settings hold refs** instead of serenity ids or bare numbers. Storage and every serialised form are unchanged. The never-loaded `authorized_users`/`authorized_groups` settings and the unregistered `/authorize` and `/deauthorize` commands are gone; their tables stay until v0.36.2 is out of the rollback window.
  - **The dashboard backend speaks refs.** A dashboard URL with the guild id `18446744073709551615` now answers 404 instead of panicking.
- [ ] **Step 3: Run every verification command** from Global Constraints: suite, doctests, all five lint commands. All pass.
- [ ] **Step 4: Commit** with `git add Cargo.toml Cargo.lock CHANGELOG.md` and the message `chore: v0.37.0`.
