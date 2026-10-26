# Split crack-core

Status: the owner approved the direction, the order and rule 6 in chat on 2026-10-25. This document waits for the owner's review.

## Intent

crack-core is one crate holding almost everything the bot does. The owner:

> "We very badly need to detangle the core module, which is what this is supposed to me leading to"

> "I'm going based on what we just experienced and the results of the topology tool. Those signals tell me that this monolith core needs to broken apart because it's becoming extremely unweildy and expensive to deal with."

"What we just experienced" is the mutants bill from v0.37.0–v0.38.0. Every mutant rebuilt all of crack-core and ran its whole test binary: about 16 s per mutant locally, and about 2 minutes on a CI runner. Every ordinary change pays the same rebuild, only less visibly.

The topology arc (ct#632) set the longer goal:

> "Separation of concerns, discord specific as segregated as can be, or at least all useful business logic detangled from discord specific"

This arc serves the first quote: it cuts the crate apart along its features. It also moves toward the second, but doesn't require it (see rule 6).

## What the owner decided

| Question | Decision |
|---|---|
| Why | Unwieldy and expensive: build and test cost, and a tangle no one can hold in their head |
| Order | 1. errors and messages; 2. economy; 3. the logging, with voice rooms; 4. music. Owner: "That order works." |
| Why that order | The owner expects the most change in "the economy and the music. The music is the primary function of the bot … with Saucey's server the ecnomomy will be getting used, and probably also the logging." Music goes last because it's the riskiest move, not the least important |
| "The logging" | Read as the **mod log** (Saucey's voice-room and moderation events), not the music queue's audit log. I stated this reading in chat and the owner did not correct it |
| Database | Unchanged, as in the transpose arc |
| Discord in feature crates | Allowed (serenity types), never poise, `Data` or `Context`. See rule 6. Owner: "I agree with your line, the economy would probably need to be an entirely different thing on another platform anyway, it's very specific to not just discord functions but also culture, so that's fine." |
| Feature work during the split | Continues in parallel. I stated this as an assumption and the owner accepted the order without objecting. Each PR lands in a day or two and freezes no area |
| Mutation testing | A tool, not a gate (owner, 2026-10-25; ct#639). No local mutants runs in tasks |

## What the map shows

Source: the topology map of master after v0.38.0 (`scripts/topology.sh extract` on the tree of `315713a2`). Line counts are non-test lines. Build timings come from ct#632's run on 2026-10-24: master before v0.37.0, dev profile, 20 jobs. Re-measure them when step 1 starts.

**Size and cost.**
- crack-core has about 56k non-test lines (116k with tests).
- It takes 66 s to compile from scratch. Every other workspace crate takes under 5 s.
- A rebuild of crack-core through the binary takes 68 s.

**One loop.** 17 of crack-core's top-level modules form one strongly connected set: each can reach every other through references. Only `config`, `macros`, `metrics` and `owners` are outside it. As things stand, no module can move into its own crate: cargo forbids the cycle that would create. The first job is cutting references that point the wrong way, not moving files.

**Where the loop closes.** These references, counted by the map, point from a lower layer to a higher one:

| From → to | Refs | What they are |
|---|---|---|
| `db` → `economy` | 219 | `db/economy.rs` uses economy's wallet, settings and immunity types |
| `db` → `guild`, `rooms`, `modlog`, `music` | 89, 19, 9, 42 | each `db/<feature>.rs` uses that feature's types |
| `errors` → `messaging` | 56 | only `messages::FAIL_*` string constants |
| `messaging` → crate root, `music`, `utils`, `commands`, … | ~160 | `Context`/`Data`/`DataInner` in delivery code; `music::remote::Control` in buttons; gp state in `message.rs` |
| `economy` → `commands`, `rooms`, `music`, `handlers` | 18, 17, 19, 2 | see step 2 |
| `music` → `commands`, `handlers` | 10, 6 | `music_utils::connected_call`, `gp_is_active`, `track_end::update_queue_messages` |

The table shows the pattern. **Each `db/<feature>.rs` belongs to its feature**, and moving the two together removes most of `db`'s wrong-way references. **`messaging` is two things**:
- a neutral part: the message enum, strings, formatting and rendering;
- a Discord delivery part: the courier, transport, buttons and poise send helpers.

**Discord.** 41k of the 56k lines carry Discord in their signatures. Only 14.5k are clean.

**The god object is narrower than it looks.** `DataInner` has 28 fields. Each feature reads only a few of them, and each already has its own registry:

| Module | `DataInner` fields it reads |
|---|---|
| `economy` | `database_pool`, `economy`, `rooms`, `songbird` |
| `coin` | `database_pool`, `economy` |
| `rooms` | `database_pool`, `modlog`, `rooms` |
| `modlog` | `database_pool`, `modlog` |
| `messaging` | `failure_notices`, `np_presses`, `status_slots` |
| `music` | 11 fields: `audit_tx`, `autoplay_buffer`, `bot_settings`, `ct_client`, `database_pool`, `db_channel`, `guild_cache_map`, `http_client`, `playback_owners`, `queue_locks`, `songbird` |

## The target shape

```
cracktunes (the binary, in crack-cli/)
  └─ crack-core          the Discord layer: poise commands, event handlers, Data,
       │                 message delivery (courier, transport, buttons), glue
       ├─ crack-gp       step 4c: the /gp game (depends on crack-music)
       ├─ crack-music    step 4b: the player
       ├─ crack-rooms    step 3: voice rooms + mod log + their queries
       ├─ crack-economy  step 2: economy + coin glue + their queries
       ├─ crack-guild    step 4a: guild settings, the per-guild cache, their queries
       └─ crack-common   step 1: errors, user-facing strings, neutral messages and rendering
            └─ crack-model   neutral ids and output types (exists)
```

The crate names are working names; see "Left to the builder".

When this is done, crack-core is the Discord layer, about 25k non-test lines. A change, a test run or a mutant in a feature's logic builds that feature's crate and the crates below it, never crack-core.

## Rules for every crate the split creates

1. **It depends only on what is below it:**
   - crack-common, crack-model and crack-types;
   - external crates;
   - feature crates carved *before* it.
   
   It never depends on crack-core (cargo would refuse the cycle anyway) or on poise. Prefer no feature-to-feature dependency at all.
2. **It never names `Data`, `DataInner` or poise's `Context`.** Functions take the narrow handles they use: the pool, the feature's registry, songbird or the http client. The table above shows how few those are. crack-core keeps `Data`, with a field for each feature's registry.

   Today about 64 functions in `economy/`, `coin/`, `rooms/` and `modlog/` take `&Data`. The heaviest files are `economy/mod.rs`, `shop`, `effects`, `theme`, `buttons` and `lock`, `rooms/lifecycle`, `ops` and `interact`, and `modlog/live` and `batch`. Converting them is part of steps 2 and 3. (The `Context` those files name is serenity's, which rule 6 allows.)
   
   My call: each feature's registry also holds a clone of the pool (a `PgPool` is a cheap handle) and is the handle its functions take. A call site then changes from `data` to `&data.economy`. Where a function really needs several features' state, it stays in crack-core as glue.
3. **A feature owns its queries.** `db/<feature>.rs` moves with the feature. The schema and migrations don't change, and `.sqlx/` stays at the workspace root, where the `query!` macros already find it. crack-voting already compiles its own `query!` macros against that root `.sqlx/`.
4. **Tests move with the code they test.** That way `cargo test -p crack-economy`, and a mutant in crack-economy, never build crack-core. cargo-mutants already tests each mutant with its own crate only (`test_workspace` is left at its default in `.cargo/mutants.toml`). Database-backed tests need the template-database support that is now `crack-core/src/test_db.rs`, so it has to move to where every crate can use it.

   A test left behind in crack-core stops catching mutants in the new crate, because a mutant is tested only with its own crate's tests.

   The test schema is `crack-core/test_migrations/`, which 10 test modules load with `sqlx::migrate!("./test_migrations")`. That path is relative to each crate's manifest. My call: keep one copy, and have new crates point at it by relative path. No copies.
5. **Features:** every new crate defines `mutation-testing` (`.cargo/mutants.toml` requires it of every member, even if empty), plus whichever of `db-tests` and `crack-coin` it needs. crack-core's features forward to them.
6. **Discord (owner's decision, 2026-10-25).** A feature crate may use serenity types where the feature's job is a Discord action: rooms' channels and permission overwrites, economy's mutes, nicknames and theme songs. Being Discord-free is **measured** with the topology tool, not required. Code that is already clean, such as the wallet, ledger and settings, should stay clean.

   This narrows what I first said in chat ("one crate per feature … with no Discord"). The map shows about 80% of each feature's lines carry Discord in their signatures, much of it because the feature *is* a Discord action. Requiring Discord-free features would roughly triple the work and turn this arc into the transpose arc's later steps.

   The owner agreed, and added that the economy is specific to Discord's culture as well as its functions: on another platform it "would probably need to be an entirely different thing anyway".
7. **User-facing strings stay in one file** (the owner's convention, #537). They move to crack-common's `messages.rs`. The cost is that changing any string rebuilds every crate above crack-common. That's accepted.
8. **No behaviour change.**
   - Rendered output stays byte-identical, and the existing byte pins prove it.
   - Wire contracts are unchanged (`wire_contract`).
   - The same commands are registered.
9. **CI checks the boundary.** For each new crate, `lint.yml` gets a `cargo tree` step: no `poise`. It is modelled on the existing "crack-model depends on no platform crate" step. The root `clippy.toml` (the serenity builder bans) already applies to every workspace crate.

   So moved files import serenity directly, or through crack-types, never `poise::serenity_prelude`. That's a mechanical change in most moved files, including `errors.rs`, which uses poise only for that prelude.

## Steps

One PR per step. Each PR lands before the next one starts: sequential, not stacked. That way in-flight feature work rebases onto one small change at a time.

### Step 1: crack-common (errors, strings, neutral messages)

**Moves:**
- `errors.rs`: `CrackedError` and the result aliases. Its only internal references are the `FAIL_*` strings, which move with it.

  It wraps external errors: sqlx, serenity, songbird, reqwest, rusty_ytdl, rspotify and audiopus. So crack-common depends on those crates. That costs no extra build time, because an external crate compiles once whichever workspace crates use it. The music-source variants may move to the music crate in step 4.
- From `messaging/`: `messages.rs`, `message.rs` (`CrackedMessage`), `format.rs` and `render.rs`, plus whichever of `cards.rs`, `placeholder.rs` and `track_failed.rs` turn out neutral once their references are checked.
- The neutral helpers they need from `utils` (for example `duration_to_string`).

**Stays in crack-core:** `courier.rs`, `transport.rs`, `interface.rs`, `buttons.rs`, `status.rs`, `discord.rs` (`ToDiscord`), `throttle.rs`. These are delivery code and use `Context`, `Data` or `music::remote::Control`.

**The design call: features render their own payloads.**
- `CrackedMessage` has about 125 variants. A few name types defined in features:
  - `Gp(Box<GpCard>)`, `Room(Box<RoomCard>)` and `ModLog(Box<ModLogEvent>)`;
  - variants using gp's `GpReveal` and `GpClip`;
  - `format.rs`'s use of `music::audit::TrackRef`.
- Those variants leave the shared enum. Each feature renders its own payload to crack-model's `Rendered`/`Card`, and delivery takes the result.
- The render code lives with the feature. It stays in crack-core until that feature is carved.
- The mechanism is the builder's choice: the existing `CrackedMessage::Card(Box<Card>)` variant, or a small trait in crack-common.
- Byte pins show nothing changed.
- These variants are small to move. Outside `messaging/`, they're built in four places: `commands/music/gp/ui.rs` twice, `commands/rooms/admin.rs` and `commands/rooms/room.rs`. Only `render.rs` matches on them.

Step 1 may land as two PRs: `errors` and the strings first, then the neutral messaging.

**Done when:**
- crack-common builds on its own;
- on the map, `errors` and the moved messaging modules are outside the loop;
- the full suite and the byte pins pass.

### Step 2: crack-economy

**Moves:**
- `economy/`: about 6.0k lines;
- `coin/`: 1.1k lines, behind the `crack-coin` feature;
- `db/economy.rs` and `db/economy/coin.rs`: about 3.1k lines.

That's about 10k non-test lines. db's 219 wrong-way references become internal.

**Stays in crack-core:** `commands/economy/` (the poise commands, 3.1k lines) and the glue below.

**References to cut first** (non-test, from the map):

| From | To | Refs | Where |
|---|---|---|---|
| `economy::buttons` | `commands::economy::shop` (`PrankEnv`, `Answer`, `run_prank`, `revealed_reply_text`, `unmasked_text`) | 11 | buttons.rs:205–390 |
| `economy::theme` | `commands::music_utils` (`connected_call`, `join_permitted`), `gp_is_active` | 6 | theme.rs:536–594 |
| `economy::target` | `rooms::guard` (`BaselineView`, `member_can_join`), `rooms::state::StoredOverwrite` | 8 | target.rs:176–187 |
| `economy::preflight`, `economy::shop` | `rooms::card::perm_names` | 5 | preflight.rs:131, shop.rs:180 |
| `economy::theme` | `rooms::settings::RoomSettings` | 2 | theme.rs:484 |
| `economy::theme`, `economy::target`, `economy::preflight` | `music` (queue, perms, `audit::Actor`, `BotReason`, `roles_resolvable`) | 19 | theme.rs:574–799, target.rs:199, preflight.rs:93 |
| `economy::theme` | `handlers::guarded::add_track_handler` | 2 | theme.rs:679 |

**How to cut them:** the code that crosses the boundary stays in crack-core as glue. That covers theme playback, running a prank, the target guard's room checks and the permission preflight. Where the decision logic really belongs to economy, a small trait defined in crack-economy and implemented in crack-core is allowed instead. Glue is preferred; a trait has to cut a real reference.

`modlog` → `economy` (7: `ActionKind`, its label, `SHOP_AUDIT_REASON_PREFIX`) becomes a downward reference and needs no change in this step.

**Done when:**
- `cargo test -p crack-economy` and `cargo mutants --list -p crack-economy` run without building crack-core;
- the suite passes;
- the timings are recorded.

### Step 3: crack-rooms (voice rooms and the mod log)

**Moves:** `rooms/` (4.4k lines), `modlog/` (2.3k), `db/rooms.rs` and `db/modlog.rs`, about 6.9k lines in all. rooms and modlog reference each other both ways: rooms → modlog 39, modlog → rooms 11. So they go together.

**Stays in crack-core:** `commands/rooms`, `commands/modlog`, and the event handlers that feed voice events in.

**References to settle:**
- `modlog` → `economy` (7). Either crack-rooms depends on crack-economy, or the mod-log event carries its own label. Builder's call.
- The Discord permission helpers that three features share: `rooms::card::perm_names` and `music::perms::roles_resolvable`. They can move down (crack-common already depends on serenity) or stay as glue.
- Once both crates exist, economy's glue that crosses into rooms (the target guard) may move into one of them. That isn't required.

**Done when:** the same checks as step 2 pass for crack-rooms.

### Step 4: music, in three PRs (design pass of 2026-10-26)

After step 3 the map showed music is far more tangled than rooms were. It reads guild settings (volume, autoplay, echoes) through methods on `Data`, reaches into the floating status message and /gp's state, and part of it is built on poise's `Context`. Guild settings, which every feature reads, turned out clean: no poise and almost no `Data`. So step 4 became three PRs. I proposed this order in chat and the owner approved it on 2026-10-26: "Yes, thank you, go ahead and continue on in this manner." It moves guild settings out of "Later, unscheduled".

- **4a: crack-guild.** Guild settings, the per-guild session cache and their queries. Small, low-risk, and it unblocks music. Detailed below.
- **4b: crack-music, the player.** `music/`, `sources/`, `http_utils.rs` and the player's tables (`queue_audit`, `play_log`, `play_history`, `queue_snapshot`, `playlist`, `metadata`, `musicreco`, `track_reaction`). Two PRs, seams then the move; designed below, after 4a landed.
- **4c: /gp.** `commands/music/gp`'s game logic and `db/gp.rs`, after 4b; one PR into `crack-gp`, designed below.

songbird lives in 4b's crate, so the Matrix-seam rule applies there: no new state in songbird's queue or track handles.

#### 4a: crack-guild

**Moves** (about 2.6k non-test lines and about 100 tests):
- `guild/settings.rs`, `guild/permissions.rs`, `guild/plan.rs`;
- `guild/cache.rs`: `GuildCache` is per-guild session state that both music (autoplay, skip votes, queue messages) and `/clean` (the bot's recent messages) use, so it goes below both;
- `guild/operations.rs`: the `GuildSettingsOperations` trait, its implementation and `get_guilds`;
- `db/guild.rs`.

**References the movable files make into crack-core:** only `errors`, `CrackedResult` and `CrackedMessage` (all in crack-common now), the `Error` alias (crack-common has the same one), and each other. `cache.rs` names `poise::serenity_prelude`, which becomes plain serenity.

**The handle.** `GuildData { database_pool: Option<PgPool>, guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>, bot_prefix: String }`, built by `Data::guild_data()`, as steps 2 and 3 did:
- `database_pool` and `guild_settings_map` keep `DataInner`'s names, so the trait's bodies move word for word. `guild_settings_map` is already an `Arc`, so the handle shares the map the commands read.
- `bot_prefix` replaces `self.bot_settings.get_prefix()`, the trait's only other read of `Data` (four sites). `BotConfig` is crack-core's and doesn't move; the prefix it returns never changes after boot, so reading it once when the handle is built gives the same value. `GuildData::default()` uses `DEFAULT_PREFIX`, which already lives in `guild/settings.rs`, so it matches `Data::default()`.

**The trait stays, and `Data` keeps implementing it.** My call:
- crack-guild implements `GuildSettingsOperations` for `GuildData`.
- crack-core's `impl GuildSettingsOperations for Data` becomes one-line delegations to `self.guild_data()`.
- So the 73 call sites in about 40 files, crack-web's included, don't change.

The alternative, changing each call to `data.guild_data().get_volume(..)`, spreads the PR across every settings command and conflicts with parallel work. The delegations cost about 130 lines in crack-core, and a crack-core test pins that the getters and setters reach the shared map.

**Autoplay's switch leaves the trait.** `get_autoplay` and `set_autoplay` don't read guild settings. They read `guild_cache_map`, and `set_autoplay` also clears the music recommender's `autoplay_buffer`. They are the session-only switch (Ruling 48), not the stored `autoplay` setting, which `get_autoplay_setting` and `set_autoplay_setting` keep in the trait.

They become inherent `impl Data` methods in crack-core's `music/autoplay.rs`, next to `AutoplayBuffer`, with the same bodies and their three tests, and they move with the player in 4b. All their callers are music code and keep calling `data.get_autoplay(..)`. Only imports that become unused change. So `GuildData` doesn't need `guild_cache_map` or anything from music.

**Stays in crack-core:**
- the `Data` delegations and `Data::guild_data()`;
- the source-scanning test `every_command_that_saves_settings_also_guards_first`, which reads crack-core's `commands/`;
- the settings commands, `config.rs`'s boot load, and the event handlers;
- the empty, unreferenced `guild/cam_rules.rs`. Deleting it is not this PR's job.

**Tests move with the code.** The trait's tests construct a `GuildData` instead of a `DataInner`, and their assertions stay the same. The database tests load `../crack-core/test_migrations`, as crack-rooms' do.

**Shims, as before:**
- crack-core's `guild` module re-exports `crack_guild::{cache, permissions, plan, settings}`;
- `guild::operations` re-exports crack-guild's and holds the `Data` implementation;
- `db/mod.rs` has `pub use crack_guild::db::guild;`.

**Dependencies:** crack-guild depends on crack-common, crack-model, crack-types and serenity. It doesn't depend on crack-economy or crack-rooms. 4b's crack-music will depend on it.

**Done when:** the same checks as step 3 pass for crack-guild, plus the boundary step in `lint.yml`, and v0.42.0 is on TuneTitan.

#### 4b: crack-music, the player (design pass of 2026-10-26, after 4a landed)

Measured on the map of 4a's branch. `music/`, `sources/` and `http_utils.rs` hold 6,328 non-test lines of functions:
- 932 lines name poise's `Context`: `query.rs`'s `mode_*` (386), `queue.rs`'s logged entry points (486), and three constructors;
- 826 lines name `Data` or `DataInner`;
- the rest name neither.

So most of the player can move, but not as it stands. The ops already separate *doing* from *showing*: each op returns `Done<T> { outcome, Settle, call }`, and a surface settles it afterwards. Only `Settle::after` needs crack-core's status message and queue messages. That is the seam.

**Two PRs**, as "How each step is done" foresaw:
- **4b-1, the seams (v0.43.0):** inside crack-core, no crate. It cuts every reference from the code that will move to `Data`, `DataInner`, poise's `Context` and crack-core's Discord layer.
- **4b-2, the move (v0.44.0):** `git mv` into `crack-music`, with shims.

Each lands in a day, so music is never frozen for long.

**What moves in 4b-2:**
- `sources/` and `http_utils.rs`;
- in `music/`: `audit`, `audit_view`, `autoplay` (the buffer), `autoplay_probe`, `disconnect`, `lease`, `ops`, `perms`, `remote` and `search`;
- `queue.rs`'s primitives (the `Primitive` rows of its `SURFACE` table and their helpers);
- `query.rs`'s `NewQueryType` with its query builders;
- the player's tables: `db/{queue_audit, play_log, play_history, queue_snapshot, playlist, metadata, musicreco, track_reaction}`, plus `db/worker_pool.rs` (`MetadataMsg` and its writer) and `db/user.rs`, which `playlist` reads;
- the clean helpers the player calls:
  - `utils::{TrackData, track_data, get_track_handle_metadata, get_requesting_user, MUSIC_SEARCH_SUFFIX}`;
  - `commands::music_utils::connected_call`;
  - `connection::get_voice_channel_for_user`.

**What stays in crack-core as glue:**
- `music/context.rs` (`QueryContext`) and `query.rs`'s `mode_*`;
- `queue.rs`'s logged entry points (the `Logs` and `Delegates` rows);
- `music/resume.rs` whole. Boot and shutdown orchestration reads the status slot, /gp, the join permit and the track-end handlers;
- the status message, the courier's `post`/`reply`, `track_end`, `yt_search_select`, `join_permitted` and `set_global_handlers_with`.

**The handle.** `MusicData`, built by `Data::music_data()`, with `DataInner`'s field names so bodies move verbatim:
- `database_pool`, `songbird`, `ct_client`, `http_client`, `audit_tx`, `db_channel`, `autoplay_buffer`, `guild_cache_map`, `playback_owners` and `queue_locks`;
- `guild: GuildData`, for the volume op's two settings calls;
- `surface: Arc<dyn Surface>` (next paragraph).

`playback_owners` and `queue_locks` become `Arc<DashMap<…>>` in `DataInner`, as `rooms` and `modlog` did, so a handle shares them. Every other field is already shared by cloning.

`lease.rs`'s four methods (`playback_owner`, `claim_playback`, `release_playback`, `lock_queue`) and autoplay's `get_autoplay`/`set_autoplay` become `impl MusicData`. `Data` keeps them as one-line delegations, as 4a did, so their 80-odd callers in /gp, the handlers and the commands don't change.

**The `Surface` trait: what the player asks of the Discord layer above it.** It has two methods:
- `settle(settle, cx, call, anchor)` is today's `Settle::after` body, moved verbatim into crack-core's implementation: refresh the queue messages, re-render now playing, or show finished.
- `echo(guild, transport, msg)` is the courier's `post(…, Destination::Echo(guild), …)`, which `remote` calls twice.

`Settle::after` and `Done::settle_now`/`settle_after` keep their signatures and call `cx.data.surface`, so no surface (command, button, dashboard) changes.
- **Why a trait:** the spec's anti-goal is a trait "with one implementation, added for later". This one cuts four real references (status, `track_end`, the courier and `Data`). It also has a second, test implementation that records what was settled. So crack-music's op tests can assert *what* an op asks to refresh without a Discord layer.
- **No `Default` in production:** a `MusicData` exists in production only through `Data::music_data()`, which supplies crack-core's surface. A silent no-op surface would drop every refresh.

**poise's constructors become crack-core extension traits,** as step 3 did with `ToPoiseReply`. The orphan rule forbids inherent methods on another crate's type. The bodies don't change, and callers only import the trait.
- `OpCx::from_ctx` (14 sites) and `Actor::from_ctx` (12) become `OpCxFromCtx` and `ActorFromCtx`;
- `Done::reply_then_settle` (4 sites) becomes `ReplyThenSettle`;
- `NewQueryType`'s eight `mode_*` (19 sites) become `QueryModes`.

**`OpCx.data` becomes a `MusicData`.** Because it keeps `Data`'s field and method names, op bodies don't change.
- `remote`, `search` and `disconnect` take `&MusicData`.
- crack-web and the commands pass `&data.music_data()`.
- The volume op's two settings calls become `data.guild.get_volume`/`set_volume`.

**`queue.rs` splits.** The primitives move. The logged entry points stay in a crack-core `music/queue.rs`, which starts with `pub use crack_music::queue::*;`. Its source-scanning `SURFACE` tests stay in crack-core and read both files, since they check how the two halves fit.

**crack-testing drops its unused `poise` dependency** (no file in it names poise). Otherwise crack-music would reach poise through it and fail rule 1's lint.

**Tests:** they move with the code (rule 4). Op tests that assert on what a settle rendered stay in crack-core with the real surface. Op tests that assert what was asked to settle move, with the recording surface.

**Done when:** 4b-2's crack-music passes the same checks as crack-rooms and crack-guild, and `cargo test -p crack-music` builds no crack-core.

#### 4c: crack-gp, the game (design pass of 2026-10-26, after 4b-2 landed)

Measured on the map of 4b-2's branch. /gp is about 4.9k non-test lines of functions:
- **`gp/commands.rs`** (848): the poise commands, almost all built on `Context`.
- **`gp/playback.rs`** (688): /gp's Discord delivery. It posts and edits cards through the courier, answers components, and registers track handlers. 392 of its lines read `Data`.
- **`gp/state.rs`** (1,213): the game's state machine, about 30 `gp_*` methods on `Data` that read only `gp_games` and the lease.
- **`gp/ui.rs`** (770): the cards. It names neither poise nor `Data`.
- **`gp_persist.rs`**: the saved form (`GpGame::to_saved`/`from_saved`), the writer, and four methods on `Data` that read `gp_persist` and `gp_games`. Boot-time resume (`gp_resume_guild`, `abandon_resume` and `announce`) reads songbird, the join permit and the track-end handlers.
- **`gp_prompts.rs`** (the prompt deck) and **`db/gp.rs`** name neither poise nor `Data`.

So the game moves and its Discord delivery stays, as with the player. Unlike 4b, nothing that moves posts to Discord, so no `Surface`-style trait is needed.

**One PR (v0.45.0) into a new crate, `crack-gp`.** It depends on crack-music for the lease that a game claims.
- Why not inside crack-music: /gp has its own stream of changes (a contributor's PRs arrive there) and its own 3.4k-line test file. In its own crate, a /gp change rebuilds and tests crack-gp alone.
- Cost: a feature crate depends on another feature crate. Rule 1 allows that for crates carved earlier; crack-rooms → crack-economy is the precedent.

**What moves:** `gp/state.rs`, `gp/ui.rs`, `gp_prompts.rs` (with `gp_prompts.json`), `gp_persist.rs` minus resume, `db/gp.rs`, and the state-machine and UI tests in `gp/test.rs`.

**What stays in crack-core as glue:** `gp/commands.rs`, `gp/playback.rs`, resume (split out of `gp_persist.rs` into a crack-core `gp_resume.rs`), `gp/test_delivery.rs`, and the tests in `gp/test.rs` that exercise playback or commands.

**The handle.** `GpData { gp_games, gp_persist, music: MusicData }`, built by `Data::gp_data()`.
- `gp_games` becomes `Arc<DashMap<…>>` in `DataInner`, as the lease maps did.
- The `gp_*` methods in `state.rs` and `gp_persist.rs` become `impl GpData`, with the same bodies. `Data` keeps them as one-line delegations, so commands, playback, the handlers and the music glue don't change.
- `GpData` gets two private one-liners, `claim_playback` and `release_playback`, which call `self.music`. That keeps the game's bodies verbatim.

**Tests** that build `Data` to reach game state build a `GpData` over crack-music's `test_music_data()`. crack-gp enables crack-music's `test-support` as a dev-dependency.

**Lessons from 4b's reviews, applied up front:**
- Prove the cut by compiling the moved crate, not by grep.
- Give any new `test-support` file its mutants exclusion in the same commit.
- Grep crack-core's tests for every function that moves.
- /gp's `#[expect(clippy::disallowed_methods)]` sites are in `commands.rs` and `playback.rs`, which stay.

**Done when:** crack-gp passes the same checks as crack-music, and `cargo test -p crack-gp` builds no crack-core.

### Later, unscheduled

- Renaming crack-core to crack-discord.
- crack-web depending on the feature crates instead of crack-core.

## How each step is done

1. **Cut the wrong-way references inside crack-core first,** with behaviour unchanged and the suite passing. This includes replacing `&Data` parameters with the feature's handle (rule 2). The map confirms the module set has no references upward.

   For steps 2 and 3, this is likely its own PR, landing before the move.
2. **Move the files and keep old paths working.** Move with `git mv`, so history follows, and add the crate to the workspace. In crack-core, `pub use crack_x as x;` keeps `crate::x::…` paths working. That keeps the diff, and the conflicts with parallel feature work, small. (My call.)

   The shims stay. They don't affect the loop measure, because the module now lives in another crate. Removing them is optional cleanup for later.
3. **Move the tests with the code.**
4. **Measure** (next section) and put the numbers in the PR body.
5. **Release and deploy.** Bump the minor version, following the transpose arc's precedent. Tag the release, and the Release workflow publishes it. Deploy to TuneTitan. Production needs the owner's go-ahead, as always.

## Measures

For every PR, record before and after:
- `scripts/topology.sh extract`, then `diff`: which modules left the loop, and crack-core's non-test lines.
- `scripts/topology.sh timings`: crack-core's from-scratch compile, the rebuild through the binary, and each new crate's compile.
- `cargo mutants --list` counts per crate. This takes seconds; no mutants runs are required.

My estimates of the non-test lines moving out of crack-core, not targets:

| Step | Lines |
|---|---|
| 1 | 3–4k |
| 2 | about 10k |
| 3 | about 7k |
| 4 | about 10k |

That leaves crack-core at about 25k lines.

**Success:**
1. Outside crack-core's own Discord layer, the loop is gone.
2. A change, a test run or a mutant in economy, rooms/mod-log or music logic builds and tests only its crate.
3. No behaviour change shipped.

## Constraints

- No behaviour change, and the database is unchanged.
- Feature work goes on in parallel: each PR is small enough to land in a day or two.
- Testing:
  - Build and Lint are the required checks. The `mutants` label is optional.
  - No local mutants runs in tasks.
  - Local test and clippy runs go under the systemd-run memory cap.
  - Never `cargo clean` the shared target, and never pass `--in-place` to cargo-mutants.
- Typed serde only. That doesn't change here, but moved code keeps it.

## Non-goals

- No database, migration or `.sqlx` layout changes.
- Poise commands and event handlers don't leave crack-core.
- Feature crates aren't required to be Discord-free (rule 6).
- No crate leaves the workspace. Unlike crack-bf and crack-osint, these get no repositories or version lines of their own.
- No second localization step: strings stay `pub const`.
- No new platform.

## Anti-goals: failures even if it compiles

- A long-lived branch that freezes economy or music.
- A trait or abstraction with one implementation, added "for later". The owner: "we need not engineer someting we aren't going to be using." A trait has to cut a real reference.
- A behaviour change hidden in a move, such as a "while I'm here" fix in a move PR.
- Rebuilding the expensive gate: no new required CI job that runs mutants, or anything else heavy, per crate.

## Left to the builder

- **Crate names.** crack-common, crack-economy, crack-rooms and crack-music are working names. crack-core already has a feature called `crack-music` (on by default), so pick a music crate name that doesn't collide with it.
- **Borderline messaging files:** decide which go down by their references on the map.
- **Each reference to cut:** glue or a trait, under the preference above.
- **Where the template-database test support lives.**
- **Whether crack-web switches to depending on feature crates.**
