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
       ├─ crack-music    step 4 (outline only)
       ├─ crack-rooms    step 3: voice rooms + mod log + their queries
       ├─ crack-economy  step 2: economy + coin glue + their queries
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

### Step 4: music (outline only; gets its own short design pass when step 3 lands)

**Scope:**
- `music/` (6.1k lines), `sources/` (1.1k) and `http_utils.rs`;
- the music tables' queries: `db/gp.rs`, `queue_audit`, `play_log`, `play_history`, `queue_snapshot`, `playlist`, `metadata`, `musicreco` and `track_reaction`, about 2.3k lines;
- the /gp game logic now under `commands/music/gp`.

**Likely shape:** two PRs, first the player (sources, query, queue, audit), then /gp.

**Known references:** `music` → `commands` (10) and `music` → `handlers` (6), listed in the table above. Music reads 11 `DataInner` fields.

songbird lives here, so the Matrix-seam rule applies: no new state in songbird's queue or track handles.

**Why a separate pass:** music is the biggest and riskiest move, and the map will look different after steps 1–3.

### Later, unscheduled

- Guild settings: `guild/` plus `db/guild.rs`, which every feature reads.
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
