# Queue audit log — design

**Date:** 2026-09-30
**Status:** approved in conversation; this document is the written record.

## Goal

Record every change made to a guild's queue as a time series: **when, who, where
(voice and text channel), by which command, and what changed**. This covers changes
made through Discord commands, through the web dashboard, and by the bot on its own.

The owner wants four readers eventually. This spec is **sub-project 1 of 4**: the
record itself, read with SQL. The readers each get their own spec, built in this
order and shaped by what they actually need ("form follows function"):

1. **This spec:** the audit record, written to Postgres and read by the operator with SQL.
2. `/auditlog` for guild admins in Discord.
3. A history panel on the web dashboard.
4. Premium-gating the web dashboard. This is independent of 1–3.

Retention with probabilistic decay is a later sub-project too (see *Deferred*).

## Decisions (owner, 2026-09-30)

| Question | Answer |
|---|---|
| Readers | All four above, each built cleanly in turn |
| Bot-initiated changes | **Recorded**, with the actor marked as the bot. A track ending on its own and the next one starting is **not** recorded: `play_log` already has plays, and they would dominate the table. |
| Refused attempts | **Not recorded.** The log says what happened to the queue; refusals stay in the bot's log. |
| Where to record | **At the queue lease**: the `QueueGuard` carries the actor, and each queue primitive records its own effect. The rejected alternatives were recording at the command layer (it records intent, and every new entry point has to remember to call it) and diffing snapshots (it cannot say who). |
| Retention | Keep everything in v1. Measured on production, ~85 plays/day, so even at 3× that the log is about 100k rows/year. |
| Existing ordering source-scan test in `queue.rs` | **Delete it.** A clippy rule was considered and cannot express it (see *Enforcement*). |

## Design

### 1. Who acted: `Actor`, carried by the guard

```rust
pub struct Actor {
    /// `None` means the bot acted on its own.
    pub user: Option<UserId>,
    pub source: Source,
    /// The qualified command name, "dashboard move", or the BotReason's name.
    pub command: Cow<'static, str>,
    /// The text channel a command was issued in; None for web and bot.
    pub origin_channel: Option<GenericChannelId>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source { Slash, Prefix, Web, Bot }

pub enum BotReason { Autopause, Autoplay, Game, IdleTimeout, Kicked, JoinCleanup }
```

- There are exactly three constructors: `Actor::from_ctx(&Context)`, `Actor::web(UserId)`
  and `Actor::bot(BotReason)`. The fields are private, so no call site assembles an
  `Actor` by hand.
- `Data::lock_queue(guild, owner, actor)` gains the parameter, and the `QueueGuard`
  stores the actor. All ~35 call sites pass one, and the compiler finds any that don't.
  - `/gp`'s queue work passes `Actor::bot(BotReason::Game)`, except the member-issued
    `/gp start`, `/gp skip`, `/gp voteskip` and `/gp end`, which are the member's
    (`Actor::from_ctx`).
  - `track_end`'s autopause passes `Actor::bot(BotReason::Autopause)`, and its autoplay
    refill passes `Actor::bot(BotReason::Autoplay)`.
  - `remote::move_by_id` passes `Actor::web(user)`.
- **Disconnects are the four paths that drop the queue without the lease:**
  `/leave`, the idle timeout (`handlers/idle.rs`), the bot being kicked
  (`handlers/serenity.rs`), and a failed join's cleanup (`music_utils.rs`).
  - They all move behind one new function, `music::disconnect(data, manager, guild, actor)`.
    It counts the tracks it is about to discard, calls `Songbird::remove`, and records
    `Leave` only if that succeeded, with today's error handling kept at each site.
  - It does **not** take the lease, so `/leave` still works during a `/gp` game, as today.
  - A failed join's cleanup discards nothing observable. It is recorded only if the
    `Call` held tracks.

### 2. What happened: one record per queue change

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Add     { tracks: Vec<TrackRef>, at: AddAt },  // one row per call; a playlist is one row
    Remove  { track: TrackRef, index: usize },
    Move    { track: TrackRef, from: usize, to: usize },
    Skip    { track: Option<TrackRef> },
    Clear   { removed: usize },
    Shuffle { count: usize },
    Stop    { removed: usize },
    Pause,
    Resume,
    Leave   { discarded: usize },
}

#[derive(Serialize, Deserialize)]
pub struct TrackRef { pub title: Option<String>, pub url: Option<String> }

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddAt { Front, Back, Index(usize) }
```

- Each primitive in `music::queue` records through its guard **after** its change
  succeeds. A primitive that changes nothing records nothing: skipping an empty queue,
  a move to the same place, clearing nothing.
- **Not queue changes, so not recorded:** volume, seek, repeat, and the
  autoplay/autopause settings. They change how the queue plays, not what is in it.
  Pause and resume are recorded, because they already go through the lease.
- **`/voteskip`:** the skip is attributed to the member whose vote reached the
  threshold, which is the `ctx` that performs it.
- The voice channel is read from the `Call` the primitive already holds.
- `force_skip_top_track` moves from `commands/music/skip.rs` into `music::queue`. It is
  the one queue change outside that module.
- Track titles come from the `TrackData` every queued track carries, via
  `queue::new_track`. `Add` lists every track it queued. Nothing is capped in v1,
  because playlist size is already bounded upstream.
- ⚠️ **`/gp` answers:** a game's adds carry titles. Sub-project 1 has no readers, but
  **readers 2 and 3 must hide a guild's `bot:game` rows while its game is running**, for
  the same reason the dashboard hides the queue today. This line is the hand-off.

### 3. Getting it to Postgres

```rust
pub struct AuditEvent {
    pub at: DateTime<Utc>,
    pub guild_id: GuildId,
    pub voice_channel: Option<ChannelId>,
    pub actor: Actor,
    pub action: Action,
}
```

- `Data` gains `audit_tx: Option<mpsc::Sender<AuditEvent>>`, built the way
  `db_channel` is. `lock_queue` clones it into the guard.
- `QueueGuard::record(action, voice_channel)` does a `try_send` and **never awaits**:
  - a full channel drops the event with a `warn!` and increments a metric counter;
  - with no sender (no database), it logs the event at `info!`.
  A queue command never fails and never waits because of the audit log.
- One writer task drains the channel and inserts. An insert error is logged and the
  writer carries on.
- **Migration** `migrations/<ts>_queue_audit.sql`:

  ```sql
  CREATE TABLE IF NOT EXISTS queue_audit (
      id                BIGSERIAL PRIMARY KEY,
      at                TIMESTAMPTZ NOT NULL,
      guild_id          BIGINT NOT NULL,
      voice_channel_id  BIGINT,
      origin_channel_id BIGINT,
      actor_user_id     BIGINT,          -- NULL = the bot
      source            TEXT NOT NULL,   -- slash | prefix | web | bot
      command           TEXT NOT NULL,
      action            TEXT NOT NULL,   -- Action's tag
      detail            JSONB NOT NULL   -- Action, serialized
  );
  CREATE INDEX IF NOT EXISTS queue_audit_guild_at ON queue_audit (guild_id, at DESC);
  CREATE INDEX IF NOT EXISTS queue_audit_actor_at ON queue_audit (actor_user_id, at DESC);
  ```

  There are **no foreign keys**: a missing `user` or `guild_settings` row must never
  fail an insert. `detail` is written from the typed `Action` with `serde_json`, and
  `action` duplicates its tag so queries can filter without reading JSON.
- `at` is the time the primitive recorded, not the time of the insert.

### 4. Enforcement

- **`clippy.toml` `disallowed-methods`** gains songbird's queue changers:
  - on `TrackQueue`: `skip`, `stop`, `dequeue`, `modify_queue`, `pause`, `resume`;
  - `Songbird::remove` and `Songbird::leave`.

  Each entry's `reason` names the audited replacement. `music::queue` and
  `music::disconnect` hold the only `#[expect(clippy::disallowed_methods)]`, each with a
  reason, the same way the existing `Songbird::get` ban works. The enqueue family is
  already banned outside `music::queue`.
- **No source scans** ([enforce with types]). The ordering test
  `queue_query_list_offset_ordering_tests` is **deleted**. It guarded "resolve before
  taking the lease" in `queue_query_list_offset`. The only lint that could express
  that, `await_holding_invalid_type` on `QueueGuard`, would also fire on every
  legitimate `call.lock().await` made under the guard, so it can't be used. The
  property stays documented on the function. `play_history_wiring_tests` is untouched
  by this spec.

## Testing

- **A test recorder:** a guard built over an in-memory channel, so tests read back
  exactly what was recorded.
- **Every primitive** gets a test asserting the exact `Action` and `Actor` it records,
  and that a no-op records nothing.
- **Disconnect:** `music::disconnect` records `Leave { discarded }` with the count it
  discarded.
- **Actor constructors:** one test per constructor. `from_ctx` distinguishes slash from
  prefix.
- **Backpressure:** with a full channel, `record` returns at once and drops the event.
  Tested with a channel of capacity 1.
- **The writer:** one `DATABASE_URL`-gated test inserts an event and reads the row back,
  including `detail` round-tripping through JSON, like the other db tests (ct#564).
- **Every test is sabotaged** against the code it guards and must fail first.

## Deferred (their own specs later)

- **Readers:** `/auditlog` (2), the dashboard history panel (3). Both must honour the
  `/gp` hiding rule above.
- **Premium gating for the dashboard (4).**
- **Decay retention.**
  - Once a day, rows older than 30 days are folded into daily counts per guild, action
    and source, with no user IDs.
  - A raw row then survives with probability falling with age (e.g. `min(1, 30/age_days)`),
    decided by a hash of its id so re-runs are idempotent.
  - Totals for old days always come from the rollups; surviving raw rows are examples.
  - This is also a privacy gain. The rollup's shape waits for readers 2 and 3.
- **Privacy policy:** a sentence about the audit log goes into the cracktun.es policy
  when this reaches production.

[enforce with types]: the owner's standing rule, 2026-09-14: enforce with types and
clippy.toml, never with tests that scan source text.
