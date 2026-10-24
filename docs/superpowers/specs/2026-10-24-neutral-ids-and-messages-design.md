# Neutral ids and messages (transpose steps 1–3)

Status: approved in chat by the owner, 2026-10-24. Two PRs, stacked.

## Intent

The topology arc (ct#632) asked for crack-core's seams, with this goal:

> "Separation of concerns, discord specific as segregated as can be, or at least
> all useful business logic detangled from discord specific" (the owner)

The bot-transpose exercise (github.com/CycleFive/bot-transpose, `docs/design.md`)
then worked out the types a bot would have if Discord were one platform among
several. Its §6 named three steps worth doing even on Discord alone:
1. refs;
2. settings without Discord types;
3. a neutral card in place of serenity's builders.

Before porting them, they were checked against cracktunes, and the owner decided:

> "Yes, let's keep the database as is for now, this is moving the code structure
> in the right direction but we need not engineer someting we aren't going to be
> using, all the other pieces will be used *and* be in a more extensiable form.
> You have a go to do the refactor steps 1 - 3."

So this arc changes how the code is structured. It does not change what the bot
does, what it stores, or what the dashboard sends.

## Owner decisions

| Question | Decision |
|---|---|
| Ids | Text, typed by role (`Ref<K>`), tagged by platform |
| Storage | **Unchanged.** Ids stay `BIGINT`. The `(platform, raw)` column pair becomes its own release when a second platform is actually next |
| Dashboard | Uses the neutral types |
| Next platform | Stoat, then Matrix. Nothing in this arc builds either |
| What to bring over from the PoC | Only what cracktunes uses now. See "Left in the PoC" |

## What the check against cracktunes found

The PoC's §6 got these wrong about cracktunes. This design follows the code.

1. **Most Discord ids in `GuildSettings` are dead.**
   - `authorized_users` and `authorized_groups`:
     - `/authorize` and `/deauthorize` are commented out of `admin`'s subcommands.
     - The loader resets both to empty (`// FIXME`, `crack-core/src/guild/settings.rs:488`).
     - None of the six `check_*` helpers has a caller.
     - `db/guild.rs` still writes `authorized_users` rows on save, but it only ever has an empty map to write.
   - `ignored_channels` has one reader (`handlers/event_log.rs:92`).
   - The music permissions (`GenericPermissionSettings`, six bare `u64` sets) are read by `cmd_check_music`. Nothing outside tests writes them.
2. **The dashboard's settings page never edits `GuildSettings`.** It edits voice rooms, the mod log and the economy. Each has its own settings type and already uses the view / form / refused pattern with string ids.
3. **cracktunes already has the intent layer:** `CrackedMessage` → `render()` → `Rendered` (the messaging arc, v0.22–v0.24). Discord leaks into it in two places:
   - **serenity types inside `CrackedMessage`:** `Mention`, `UserId`, `RoleId`, `GenericChannelId`, `GuildId`, `CreateEmbed`, `HttpError`, and songbird's `ControlError`. Discord also reaches in through the payloads it boxes: `ModLogEvent`, `GpCard`, `RoomCard`, `NowPlayingCard` and `EchoLine`.
   - **builders written by hand:** about 30 files build `CreateEmbed`, buttons or select menus directly, outside the renderer.
4. **The schema is wide and rollback is image-only.**
   - `guild_id` appears in 15 migrations, with 7 foreign keys into `guild_settings`.
   - Rollback swaps the bot image and keeps the migrate image, so the previous image must read and write every column.

The topology map of master (ct#632) measures the cost:
- `GuildSettings` is tainted only by its `GuildId` field (settings.rs:331). It is the sole carrier for 42 units.
- `CrackedMessage` has 46 cuts.
- `render` is tainted through `CrackedMessage`, `Rendered` and the card renderers.
- `Rendered` is tainted by `CreateEmbed` and `CreateComponent`.

## Goals

1. **A crate that cannot depend on Discord** holds the bot's neutral vocabulary:
   - refs (PR 1);
   - the message output types (PR 2).
2. **Server settings hold no Discord types and no bare Discord numbers** (PR 1).
3. **The dashboard backend speaks refs** (PR 1).
4. **Messages hold refs, and rendering produces neutral output** (PR 2).
   - Messages and every payload they box hold refs.
   - Rendering produces neutral output.
   - serenity's builders appear only in one Discord conversion module, plus the transports that send.
5. **Each PR reports the topology diff** against master: what became clean, and which carriers are gone.

## Non-goals

- **The database is untouched.** No migration and no schema change. Every id column stays `BIGINT`.
- **No wire change.** Dashboard URLs, JSON and HTML are unchanged, and ids on the wire stay the strings they are today.
- **No second platform.** No Stoat or Matrix code.
- **No user-visible change.** The one exception is `/settings get all`, which prints `GuildSettings` with `{:?}`, so its ids now show as `Community(discord:123…)`.
- **The poise command layer keeps serenity's ids.**
  - Refs go at the seams: settings, messages and their payloads, persistence, and the dashboard backend.
  - The command layer's 1,290 `GuildId` uses are not converted.
- **Later steps stay later.** Not in this arc:
  - the error split (`CrackedError`, step 6);
  - gates and standing (step 4);
  - the queue leaving songbird (step 5);
  - moving crack-core modules into crates.
- **The other settings types are not converted.** Refs in the voice-room, mod-log and economy settings types are a later "when touched".

## Anti-goals (a failure even if it compiles)

- Anything a user sees, a row the bot writes, or a URL or JSON field changing (beyond the `/settings get all` dump).
- A release that cannot roll back to v0.36.2 on the same database.
- Refs spread through hot paths as clones everywhere. They belong at the seams, and the Discord shell keeps its `Copy` ids.
- Code moved between files without tests, so the required mutants check blocks the PR (a moved function counts as a NEW miss). Change types in place, and move only what must move.
- A panic on a stored id. `GuildId::new(0)` and `GuildId::new(u64::MAX)` both panic (serenity ids are `NonMaxU64`), so every conversion from storage or text is checked.

## Constraints

- **Workspace member rules:**
  - a new member declares the umbrella feature `mutation-testing = []` (.cargo/mutants.toml);
  - it inherits `version.workspace = true` and follows the manifest pattern of `crack-types`.
- **The Dockerfile** copies the whole tree (`COPY . .`), so a new member needs no Docker change.
- **Local runs:** every test and mutants run goes under
  `systemd-run --user --scope --quiet -p MemoryMax=12G -p MemorySwapMax=0 -p OOMPolicy=continue`.
- **Proving tests can fail:** `scripts/mutants_check.py --base <task base>` after each task. A NEW miss is caught by a test, or waived with `#[cfg_attr(test, mutants::skip)]` and a `//` reason directly above it. SQL text, lock order and plain field assignments are sabotaged by hand.
- **CI:** the `Build` and `Mutants` workflows currently fail before compiling. `ikalnytskyi/action-setup-postgres@v8` calls `apt-key`, which is gone from `ubuntu-latest`. That is a separate CI fix waiting on the owner. Until it lands, local runs are the evidence, and the PR bodies say so.
- **Commits:** the owner's trailer only; never `git add -A`; `cargo fmt`; `clippy -D warnings` on `--all-targets --all-features`.

## Design

### 1. `crack-model`, a new crate (PR 1)

- **Holds:** the neutral vocabulary.
- **Dependencies:** `serde` and `thiserror` only.
- **Enforced in CI:** a `lint.yml` step fails if `cargo tree -p crack-model -e normal` names `serenity`, `poise`, `songbird`, `sqlx` or `reqwest`. That is the PoC's no-platform-crate check.

*My call:* a new crate, not a module in `crack-types`. crack-types depends on serenity, songbird, poise, rspotify, rusty_ytdl and reqwest, so nothing in it can be shown Discord-free, nor can anything that depends on it. The name `crack-model` is mine.

`crack_model::ids`, ported from the PoC's `ids.rs`, trimmed to what cracktunes uses:

```rust
#[non_exhaustive] pub enum Platform { Discord }          // serde "discord"; as_str; FromStr
pub trait Kind: sealed::Sealed + 'static { const NAME: &'static str; }
// kinds (uninhabited) and aliases:
User => UserRef, Community => CommunityRef, Room => RoomRef, Message => MessageRef, Role => RoleRef
pub struct RawId(Arc<str>);                               // as_str
pub struct Ref<K: Kind> { platform, raw: RawId, PhantomData<K> }
impl<K: Kind> Ref<K> {
    pub fn new(platform: Platform, raw: impl AsRef<str>) -> Result<Self, RefError>; // non-empty, no control chars
    pub fn platform(&self) -> Platform;
    pub fn raw(&self) -> &RawId;
    pub fn raw_for(&self, platform: Platform) -> Result<&str, RefError>;          // the only checked way out
    pub fn key(&self) -> String;                                                   // "discord:123"
}
// Clone, Eq, Ord, Hash by (platform, raw); Debug "User(discord:1)"; Display = key;
// FromStr parses a key; serde is {"platform": "discord", "id": "123"}.
pub enum RefError { BadRaw{kind, raw}, BadKey{kind, key}, UnknownPlatform(String), WrongPlatform{kind, expected, found} }
```

**Left out on purpose:**
- platform lifecycle, Telegram, Matrix and Stoat variants;
- the `Thread` kind (cracktunes treats threads as channels: `GenericChannelId`);
- storage row shapes.

**`Platform` gets one variant now**, with `#[non_exhaustive]`, so a match outside the crate already needs the arm Stoat will fill.

### 2. The Discord side: `crack_types::discord` (PR 1)

All conversions between serenity ids and refs live here:

```rust
pub trait ToRef { type Kind: Kind; fn to_ref(&self) -> Ref<Self::Kind>; }
// GuildId→Community, UserId→User, ChannelId→Room, GenericChannelId→Room,
// RoleId→Role, MessageId→Message.
pub fn guild_id(r: &CommunityRef) -> Result<GuildId, IdError>;   // also user_id, channel_id,
                                                                 // generic_channel_id, role_id, message_id
pub fn snowflake<K: Kind>(r: &Ref<K>) -> Result<u64, IdError>;   // for storage: i64 casts at the db layer
pub fn from_snowflake<K: Kind>(id: u64) -> Result<Ref<K>, IdError>;
pub enum IdError { WrongPlatform{kind, found}, NotASnowflake{kind, raw} }
```

- **What counts as a snowflake:** decimal digits only, parsing to a `u64` in `1..u64::MAX`. That means not 0 and not `u64::MAX`, the two values serenity's constructors panic on.
- **`IdError` belongs to crack-types.** The neutral `RefError` holds no Discord rule.
- **The orphan rule** puts `ToRef` here: crack-types defines the trait, so it may implement it for serenity's types. No `From` impls.

### 3. Settings (PR 1, step 2)

**`GuildSettings`:**
- `guild_id: GuildId` becomes `guild_id: CommunityRef`. The field name stays, so call sites change by type, not by name.
- `guild_name: FixedString` becomes `String`.
- `ignored_channels: HashSet<u64>` becomes `HashSet<RoomRef>`.
- `authorized_users` and `authorized_groups` are deleted, along with the code that serves only them:
  - `authorize_user`, `deauthorize_user` and the six `check_*` helpers;
  - the unregistered `/authorize` and `/deauthorize` command files;
  - `CrackedMessage::UserAuthorized` and `UserDeauthorized`;
  - the `authorized_users` row write in `db/guild.rs`.
- **The tables stay**, so v0.36.2 can still roll back onto the same database.
- `GuildSettings::new` takes a `CommunityRef`.
- `GuildSettingsMap` stays keyed by `GuildId`: it is the Discord shell's index of neutral values.

**Nested settings:**
- `LogSettings` channel fields become `Option<RoomRef>`.
- `WelcomeSettings`' `channel_id` becomes `Option<RoomRef>`, and `auto_role` becomes `Option<RoleRef>`.
- Getters that return `GenericChannelId` return refs instead. Discord callers convert with §2.
- `GenericPermissionSettings`' six `u64` sets become sets of `RoleRef`, `UserRef` and `RoomRef`, and its checks take refs. The same goes for `CommandChannel`.

**Methods that take a serenity type leave `GuildSettings`:**
- `get_log_channel_type_fe(&FullEvent)` becomes a function in `handlers::event_log`.
- `add_denied_music_user` takes a `UserRef`.
- Done when `guild/settings.rs` and `guild/permissions.rs` have no serenity or poise import outside `#[cfg(test)]`. If a check that is truly Discord-only remains, it moves out.

**Storage:**
- The `*Read` row types keep their `i64` fields and SQL text.
- Their `From`/`TryFrom` impls convert through `from_snowflake`, and saving converts with `snowflake`.
- **Bad stored ids:** an invalid id inside a set or an optional field is dropped with a `warn!` naming the column. An invalid `guild_id` makes that row fail to load with an error. Neither ever panics.
- `.sqlx` is unchanged, because no SQL text changes.

**Serde shape:**
- `GuildSettings` serialises `guild_id` and `ignored_channels` as ref pairs.
- Nothing reads that today: no config sets `guild_settings_map`, and `save_json` has no caller.
- The CHANGELOG notes the new shape for the `guild_settings_map` config option.

### 4. Dashboard (PR 1)

- **The `Backend` trait** (crack-web/src/backend.rs) speaks refs:
  - `GuildId` becomes `CommunityRef`, and `UserId` becomes `UserRef`;
  - `Presence`'s channels become `RoomRef`;
  - `GuildEntry.id` becomes a `CommunityRef`;
  - the event hub (watch.rs) is keyed by `CommunityRef`.
- **`parse_guild`** keeps its rule (`/g/0`, `/g/abc` and overflow are not guilds) and returns a `CommunityRef` built through §2, so the routes answer exactly as today.
- **The Discord-backed implementation** (crack-web/src/lib.rs) converts at its edge with `crack_types::discord`. So do the OAuth session, whose user id becomes a `UserRef` where the session is read, and the bot's hub publisher.
- **What the browser gets is unchanged:** view types keep their `String` ids (the ref's raw id), and URLs keep the bare snowflake.

*My call:* the wire format changes with the storage format, when a second platform needs it, not before.

### 5. Messages (PR 2, step 3)

PR 2 gets its own plan, written against PR 1's code. This section fixes its shape.

**Neutral output types in `crack-model`.** They are a superset of what cracktunes sends today, and nothing more:

```rust
pub struct Rendered { content: Option<String>, cards: Vec<Card>, rows: Vec<Row>, mentions: Mentions }
pub struct Card { title, url, description, author: Option<Author>, fields: Vec<Field>,
                  thumbnail, footer: Option<Footer>, timestamp, colour: Option<u32> }
pub struct Row(Vec<Control>);
pub enum Control { Button{id, label, emoji, style, disabled}, Link{url, label, emoji},
                   Select{id, placeholder, options, min, max, disabled},
                   PickUsers{..}, PickRoles{..} }
pub enum Mentions { None, Users }
pub enum Mention { User(UserRef), Room(RoomRef), Role(RoleRef) }   // Display: <@id>, <#id>, <@&id>
```

- **`Rendered` keeps its name and becomes neutral.** The 35 files that use it change constructors, not concepts.
  - Its `to_reply`, `to_message`, `to_edit`, `to_followup` and `to_interaction_message` move into a Discord conversion module, `crack-core/src/messaging/discord.rs`.
  - That module also converts `Card` → `CreateEmbed` and `Row` → `CreateActionRow`.
  - *My call:* I told the owner "`Rendered` becomes private to the Discord side". Keeping the name for the neutral type and making the conversion the Discord side is the same separation, with less churn.
- **Clipping stays where it is.** `messaging::format::clip` and its limits are Discord's numbers, and they stay put. Per-platform limits come with a second platform.
- **`Mention`'s spelling is the markdown that Discord and Stoat share** for users and channels.
  - *My call:* the PoC passed the spelling in from the adapter. A platform that mentions some other way (Matrix's HTML pills) renders from the ref itself.
  - `CrackedMessage`'s `Display` keeps producing the message text, unchanged byte for byte.

**`CrackedMessage` and its payloads hold refs:**
- Every `UserId`, `RoleId`, `GenericChannelId` and `GuildId` becomes a ref. Every `mention: Mention` field goes away, because the text mentions from the ref.
- `CreateEmbed(Box<CreateEmbed>)` becomes `Card(Box<Card>)`.
- `ErrorHttp(HttpError)` and `SeekFail { error: ControlError }` carry the error's text.
- `CrackedError(CrackedError)` carries the error's rendered text, built where the message is made. That leaves `CrackedError` itself untouched until step 6.
- The boxed payloads (`NowPlayingCard`, `QueuedCard`, `EchoLine`, `Controls`, the `GpCard` family, the `RoomCard` family, `ModLogEvent`) hold refs. Their render functions return the neutral `Rendered`.
- `GpCategory` and `GpReveal` derive poise's `ChoiceParameter`, which is Discord's command surface. The derive moves to mirror enums in the `/gp` command module, which convert into the domain enums.

**The hand-built builder sites move to `Card` and `Row`:**
- music commands: `queue`, `doplay`, `dosearch`, `spotify`, `volume`, `remove`;
- `status`, `ping`, `debug`, `utils.rs`, `poise_ext.rs`, `http_utils.rs`, `music/ops/edit.rs`, `handlers/serenity.rs`;
- `/gp`'s ui, voice rooms' card, the mod log, and the economy's buttons and shop.

**Stays Discord:**
- modals (`CreateModal`, `CreateInputText`), which are input, not output;
- interaction acknowledgements and the transports that send (courier, transport, status, interface press handling).

**Done when:**
- `CrackedMessage`, `render`, `Rendered` and every boxed payload are clean in the topology map;
- outside `messaging/discord.rs` and the transports, no file names `CreateEmbed`, `CreateButton`, `CreateSelectMenu`, `CreateActionRow` or `CreateComponent`. A clippy `disallowed-types` entry enforces this where clippy allows.

## PRs, versions and order

| PR | Contents | Version | Base |
|---|---|---|---|
| 1 | §1–§4: `crack-model` ids, the Discord conversions, settings, the dashboard backend | v0.37.0 | master |
| 2 | §5: neutral messages and output | v0.38.0 | PR 1's branch (stacked) |

- **Both PRs stop at "open".** The owner reviews and merges.
- **PR 2's body gives the stacked merge order:** retarget PR 2 to master before merging PR 1 with `--delete-branch`.
- *My call:* minor bumps, because a new crate and a new data model are more than a patch, even with no user-visible change.
- **CHANGELOG:** each PR adds an `Unreleased` entry under "Internal".

## Measuring

- **Before:** `scripts/topology.sh extract` on master `63a15373`, with ct#632's final tool. The extract is taken from the ct#632 worktree, pointed at this tree.
- **After each PR:** extract again, run `diff`, and put the headline numbers in the PR body:
  - units that went clean;
  - carriers that went away;
  - the state of `GuildSettings`, `CrackedMessage`, `render` and `Rendered`;
  - confirmation that `crack-model` has no Discord at all.
- **The map is a measure, not a gate.** A unit that stays tainted for a reason this arc doesn't own (`CrackedError`, step 6) is reported with that reason.

## Testing

- **TDD per task.** Existing tests keep their assertions; only their construction changes. A changed assertion needs a stated reason.
- **Conversions:**
  - a valid snowflake round-trips through `ToRef` and back;
  - 0, `u64::MAX`, overflow, non-digits, an empty string and another platform are each refused;
  - the serde pair and the key both round-trip.
- **Storage:** the settings round-trip db-tests keep passing through Postgres, and a row with an invalid id loads without panicking.
- **Output, PR 2:** for every `Card` and `Control` field, the conversion's serenity JSON matches what the old builder code produced. Every existing render test (descriptions, colours, clipping, allowed mentions, ephemeral flags, extra embeds) passes unchanged.
- **Per task:** the mutants diff check runs against the task's base.
- **Before each PR:** the full suite with the db-tests and crack-coin features, `clippy -D warnings`, `fmt` and `check_waivers.py`.

## Left to the builder

- Private helper names, and the order call sites are converted.
- How the `lint.yml` crate check is written.
- Whether a conversion is a free function or a method, where both read naturally.
- Test names.

## Left in the PoC

- `Platform` lifecycle and the extra platforms.
- `Scope` (Stoat has servers too; only Matrix needs a room scope).
- `PlatformCaps` and fallback planning.
- `Pressables`, the rich-text tree and HTML rendering.
- `Gate` and `Standing`, `Queue` and voice, and `Directory`.
- The storage row shapes and `schema.sql`.

## Follow-ups (not this arc)

- **The `(platform, raw)` storage migration**, in expand-then-contract releases, when Stoat is next. The dashboard's wire JSON gains the platform with it.
- **Dropping the `authorized_users` table** once v0.36.2 is past its rollback window.
- **Refs in the other settings types** (voice rooms, mod log, economy) and their dashboard forms.
- **Per-platform text limits.**
- **Steps 4–6** of bot-transpose §6.
- **Correcting bot-transpose §6** with what the check above found.
