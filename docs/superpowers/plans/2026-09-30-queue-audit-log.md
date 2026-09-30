# Queue Audit Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every change to a guild's queue, whether from a command, the web dashboard or
the bot itself, is written to a `queue_audit` Postgres table with who, where, when,
which command and what changed.

**Architecture:**
- `lock_queue` takes an `Actor`, and the `QueueGuard` carries it along with a clone of
  an audit `Sender`.
- Each queue primitive in `music::queue` calls `guard.record(..)` after its change
  succeeds. `record` is a non-blocking `try_send` to a writer task that inserts rows.
- Disconnects, which drop the queue without the lease, go through one audited
  `music::disconnect`.
- `clippy.toml` bans every songbird queue changer outside those two places.

**Tech Stack:** Rust workspace; songbird, serenity `next`, poise, tokio `mpsc`, sqlx 0.9
(postgres, chrono, json, offline `.sqlx` cache), and serde.

**Spec:** `docs/superpowers/specs/2026-09-30-queue-audit-log-design.md`

## Global Constraints

- **Typed serde only.** No `serde_json::json!` or `serde_json::Value` for data we own.
  `detail` is written from the typed `Action` via `sqlx::types::Json(&action)`.
- **No source-scan tests.** This is the owner's standing rule. Enforce with types and
  `clippy.toml`.
- **Sabotage every test.** Each new test must be seen to FAIL against a deliberate break
  of the code it guards before it counts. Record the break in the task report.
- **Recording never blocks or fails a queue operation.** Use `try_send`, never `.send().await`.
- **Successes only.** Record after the change succeeds, and a no-op records nothing.
- **Bot actions are recorded.** A track ending naturally is **not** recorded.
- **Not recorded:** volume, seek, repeat, and the autoplay/autopause settings.
- **Existing bans are excepted with `#[allow(clippy::disallowed_methods)]`** plus a comment
  saying why, as the repo already does. Match that style.
- **Migration:** add `20260930120000_queue_audit.sql` to **both** `migrations/` and
  `crack-core/test_migrations/`. They are copies, not a symlink.
- **Local Docker:** plain `docker` on this machine targets a remote host. Always use
  `docker --context default` for throwaway containers.
- **Never `git add -A`.** Stage named paths.
- **Commit trailer**, exactly and alone:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`

## Rulings made while planning

- **No metric for a dropped event.** The spec says "`warn!` and a metric". But
  `crate::metrics` is feature-gated (`crack-metrics`) and nothing serves it, so a
  counter would be invisible. A dropped event gets a `warn!` only. If this is wrong,
  one line adds the counter later.
- **`Actor::from_ctx` delegates to `Actor::for_command(user, is_prefix, name, channel)`**, which
  is testable without a poise `Context`. `from_ctx` itself is a three-line adapter.
- **`AddAt::Index`** is used only by `queue_query_list_offset`, which inserts at an offset.

## Review Focus

1. **A playlist add is one row, not N.** Each enqueue entry point records one `Add`
   listing every track it queued. Pinned in Task 4, `a_batch_add_is_one_record_listing_every_track`.
2. **A move to the same position, or skipping or clearing an empty queue, records
   nothing.** Pinned in Task 4, `no_ops_record_nothing`.
3. **A full audit channel never delays a command.** Pinned in Task 1,
   `a_full_channel_drops_without_waiting`.
4. **The web mover is the recorded actor, not the bot.** Pinned in Task 3,
   `a_dashboard_move_is_recorded_as_the_mover`.
5. **`/leave` still works during a `/gp` game** (disconnect does not take the lease).
   Pinned in Task 5, `disconnect_does_not_wait_for_the_lease`.

---

## File Structure

| File | Responsibility |
|---|---|
| `crack-core/src/music/audit.rs` (new) | `Actor`, `Source`, `BotReason`, `Action`, `TrackRef`, `AddAt`, `AuditEvent`, `track_ref`, `emit` |
| `crack-core/src/db/queue_audit.rs` (new) | `insert_audit_event`, `spawn_audit_writer` |
| `migrations/20260930120000_queue_audit.sql` (new, plus its copy in `crack-core/test_migrations/`) | the table and indexes |
| `crack-core/src/music/lease.rs` | `lock_queue(.., actor)`, `QueueGuard { actor, audit_tx }`, `QueueGuard::record` |
| `crack-core/src/lib.rs` | `DataInner.audit_tx` field and its default |
| `crack-core/src/config.rs` | spawns the writer when there is a pool |
| `crack-core/src/music/queue.rs` | primitives record; `force_skip_top_track` moves here; the ordering source-scan module is deleted |
| `crack-core/src/music/disconnect.rs` (new) | `disconnect(data, manager, guild, actor)` |
| lock and disconnect call sites | pass an `Actor` |
| `crack-core/src/music/remote.rs`, `crack-web/src/{backend,lib,routes,test_support}.rs` | the dashboard move carries its user |
| `clippy.toml` | bans songbird's queue changers and disconnects |
| `docs/queue-audit.md` (new) | operator notes and example SQL |

---

### Task 1: The audit types, `track_ref`, and `emit`

**Files:**
- Create: `crack-core/src/music/audit.rs`
- Modify: `crack-core/src/music/mod.rs` (add `pub mod audit;`)

**Interfaces:**
- Produces:
  - `pub struct Actor` (private fields), with getters `user() -> Option<UserId>`,
    `source() -> Source`, `command() -> &str` and `origin_channel() -> Option<GenericChannelId>`;
  - constructors `Actor::from_ctx(&crate::Context<'_>)`,
    `Actor::for_command(UserId, bool /*is_prefix*/, impl Into<Cow<'static, str>>, Option<GenericChannelId>)`,
    `Actor::web(UserId)` and `Actor::bot(BotReason)`;
  - `pub enum Source { Slash, Prefix, Web, Bot }` and `Source::as_str(&self) -> &'static str`;
  - `pub enum BotReason { Autopause, Autoplay, Game, IdleTimeout, Kicked, JoinCleanup }`;
  - `pub enum Action` (spec §2), plus `Action::name(&self) -> &'static str`, which returns
    the serde tag;
  - `pub struct TrackRef { pub title: Option<String>, pub url: Option<String> }` and
    `pub enum AddAt { Front, Back, Index(usize) }`;
  - `pub struct AuditEvent { pub at: DateTime<Utc>, pub guild_id: GuildId, pub voice_channel: Option<ChannelId>, pub actor: Actor, pub action: Action }`;
  - `pub fn track_ref(track: &TrackHandle) -> TrackRef`;
  - `pub fn emit(tx: Option<&mpsc::Sender<AuditEvent>>, event: AuditEvent)`.

- [ ] **Step 1: Write the failing tests** in `audit.rs` under `#[cfg(test)] mod test`

```rust
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn the_constructors_set_source_user_and_command() {
        let slash = Actor::for_command(UserId::new(7), false, "skip", Some(GenericChannelId::new(3)));
        assert_eq!((slash.source(), slash.user(), slash.command()), (Source::Slash, Some(UserId::new(7)), "skip"));
        assert_eq!(slash.origin_channel(), Some(GenericChannelId::new(3)));
        let prefix = Actor::for_command(UserId::new(7), true, "skip", None);
        assert_eq!(prefix.source(), Source::Prefix);
        let web = Actor::web(UserId::new(8));
        assert_eq!((web.source(), web.user(), web.command()), (Source::Web, Some(UserId::new(8)), "dashboard move"));
        assert_eq!(web.origin_channel(), None);
        let bot = Actor::bot(BotReason::IdleTimeout);
        assert_eq!((bot.source(), bot.user(), bot.command()), (Source::Bot, None, "idle timeout"));
    }

    #[test]
    fn every_action_names_its_own_serde_tag() {
        #[derive(serde::Deserialize)]
        struct Tag { action: String }
        let t = TrackRef { title: Some("a".into()), url: None };
        let all = vec![
            Action::Add { tracks: vec![t.clone()], at: AddAt::Back },
            Action::Remove { track: t.clone(), index: 1 },
            Action::Move { track: t.clone(), from: 1, to: 2 },
            Action::Skip { track: Some(t.clone()) },
            Action::Clear { removed: 2 },
            Action::Shuffle { count: 3 },
            Action::Stop { removed: 1 },
            Action::Pause,
            Action::Resume,
            Action::Leave { discarded: 4 },
        ];
        for a in all {
            let tag: Tag = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
            assert_eq!(tag.action, a.name());
        }
    }

    #[test]
    fn a_move_serializes_with_its_fields() {
        let a = Action::Move { track: TrackRef { title: Some("t".into()), url: Some("https://x".into()) }, from: 3, to: 1 };
        let back: Action = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }

    fn event() -> AuditEvent {
        AuditEvent {
            at: chrono::Utc::now(),
            guild_id: GuildId::new(1),
            voice_channel: None,
            actor: Actor::bot(BotReason::Autopause),
            action: Action::Pause,
        }
    }

    #[test]
    fn emit_sends_the_event() {
        let (tx, mut rx) = mpsc::channel(4);
        emit(Some(&tx), event());
        assert_eq!(rx.try_recv().unwrap().action, Action::Pause);
    }

    #[tokio::test]
    async fn a_full_channel_drops_without_waiting() {
        let (tx, mut rx) = mpsc::channel(1);
        emit(Some(&tx), event());
        // The channel is full. This must return at once, not wait for room.
        tokio::time::timeout(std::time::Duration::from_millis(50), async { emit(Some(&tx), event()) })
            .await
            .expect("emit must not wait on a full channel");
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err(), "the second event was dropped");
    }

    #[test]
    fn no_sender_is_not_an_error() {
        emit(None, event());
    }
}
```

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test -p crack-core --lib music::audit`
Expected: a compile FAIL, because `audit` is empty.

- [ ] **Step 3: Implement `audit.rs`**

```rust
//! The queue audit log: who changed a guild's queue, how, and when. Recorded by
//! the queue primitives through their `QueueGuard` (see `music::lease`) and by
//! `music::disconnect`; written to `queue_audit` by `db::queue_audit`.
//! Spec: docs/superpowers/specs/2026-09-30-queue-audit-log-design.md

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GenericChannelId, GuildId, UserId};
use songbird::tracks::TrackHandle;
use std::borrow::Cow;
use tokio::sync::mpsc;

/// Where an action came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Slash,
    Prefix,
    Web,
    Bot,
}

impl Source {
    /// The stored spelling; matches the serde name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Slash => "slash",
            Source::Prefix => "prefix",
            Source::Web => "web",
            Source::Bot => "bot",
        }
    }
}

/// Why the bot changed a queue with nobody asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotReason {
    Autopause,
    Autoplay,
    Game,
    IdleTimeout,
    Kicked,
    JoinCleanup,
}

impl BotReason {
    fn name(self) -> &'static str {
        match self {
            BotReason::Autopause => "autopause",
            BotReason::Autoplay => "autoplay",
            BotReason::Game => "gp",
            BotReason::IdleTimeout => "idle timeout",
            BotReason::Kicked => "disconnected",
            BotReason::JoinCleanup => "join cleanup",
        }
    }
}

/// Who acted. Built only by the constructors below, so every recorded action
/// names a source that matches how it was built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    user: Option<UserId>,
    source: Source,
    command: Cow<'static, str>,
    origin_channel: Option<GenericChannelId>,
}

impl Actor {
    /// The member running this command, in the channel they ran it from.
    #[must_use]
    pub fn from_ctx(ctx: &crate::Context<'_>) -> Self {
        let is_prefix = matches!(ctx, poise::Context::Prefix(_));
        Self::for_command(
            ctx.author().id,
            is_prefix,
            ctx.command().qualified_name.clone(),
            Some(ctx.channel_id()),
        )
    }

    /// A command, spelled out. `from_ctx` is the usual way in.
    #[must_use]
    pub fn for_command(
        user: UserId,
        is_prefix: bool,
        name: impl Into<Cow<'static, str>>,
        origin_channel: Option<GenericChannelId>,
    ) -> Self {
        Self {
            user: Some(user),
            source: if is_prefix { Source::Prefix } else { Source::Slash },
            command: name.into(),
            origin_channel,
        }
    }

    /// A signed-in member acting from the web dashboard.
    #[must_use]
    pub fn web(user: UserId) -> Self {
        Self {
            user: Some(user),
            source: Source::Web,
            command: Cow::Borrowed("dashboard move"),
            origin_channel: None,
        }
    }

    /// The bot acting on its own.
    #[must_use]
    pub fn bot(reason: BotReason) -> Self {
        Self {
            user: None,
            source: Source::Bot,
            command: Cow::Borrowed(reason.name()),
            origin_channel: None,
        }
    }

    #[must_use]
    pub fn user(&self) -> Option<UserId> {
        self.user
    }
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }
    #[must_use]
    pub fn origin_channel(&self) -> Option<GenericChannelId> {
        self.origin_channel
    }
}

/// A track as the log remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Where an add put its tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddAt {
    Front,
    Back,
    Index(usize),
}

/// What happened to the queue. Stored whole as `detail`, with its tag also in
/// the `action` column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Add { tracks: Vec<TrackRef>, at: AddAt },
    Remove { track: TrackRef, index: usize },
    Move { track: TrackRef, from: usize, to: usize },
    Skip { track: Option<TrackRef> },
    Clear { removed: usize },
    Shuffle { count: usize },
    Stop { removed: usize },
    Pause,
    Resume,
    Leave { discarded: usize },
}

impl Action {
    /// The serde tag, for the `action` column.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Action::Add { .. } => "add",
            Action::Remove { .. } => "remove",
            Action::Move { .. } => "move",
            Action::Skip { .. } => "skip",
            Action::Clear { .. } => "clear",
            Action::Shuffle { .. } => "shuffle",
            Action::Stop { .. } => "stop",
            Action::Pause => "pause",
            Action::Resume => "resume",
            Action::Leave { .. } => "leave",
        }
    }
}

/// One row of `queue_audit`.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub at: DateTime<Utc>,
    pub guild_id: GuildId,
    pub voice_channel: Option<ChannelId>,
    pub actor: Actor,
    pub action: Action,
}

/// Title and URL of a queued track, read without waiting.
///
/// The primitives run inside `modify_queue`'s synchronous closure, so this
/// uses `try_read`. The metadata lock is written once, as the track is built,
/// so contention is not expected; if it happens, the track is recorded without
/// a title rather than the queue waiting.
#[must_use]
pub fn track_ref(track: &TrackHandle) -> TrackRef {
    let data = crate::utils::track_data(track);
    match data.aux_metadata.try_read() {
        Ok(meta) => TrackRef {
            title: meta.as_ref().and_then(|m| m.title.clone()),
            url: meta.as_ref().and_then(|m| m.source_url.clone()),
        },
        Err(_) => TrackRef { title: None, url: None },
    }
}

/// Hand an event to the writer. Never waits and never fails: a full channel
/// drops the event with a warning, and with no writer (no database) the event
/// goes to the log instead.
pub fn emit(tx: Option<&mpsc::Sender<AuditEvent>>, event: AuditEvent) {
    match tx {
        Some(tx) => {
            if let Err(e) = tx.try_send(event) {
                tracing::warn!("queue audit: dropped an event ({e})");
            }
        },
        None => tracing::info!(
            guild = %event.guild_id,
            user = ?event.actor.user(),
            source = event.actor.source().as_str(),
            command = event.actor.command(),
            action = event.action.name(),
            "queue audit (no database)"
        ),
    }
}
```

If `crate::utils::track_data` is private, make it `pub(crate)`. It is the approved
accessor behind `get_track_handle_metadata`.

- [ ] **Step 4: Run the tests to watch them pass**

Run: `cargo test -p crack-core --lib music::audit`
Expected: 6 passed.

- [ ] **Step 5: Sabotage, one at a time, restoring each**

  1. In `Action::name`, return `"moved"` for `Move`. `every_action_names_its_own_serde_tag` must FAIL.
  2. In `emit`, replace `try_send(event)` with `blocking_send(event)` inside the
     `Some` arm, which blocks on a full channel. `a_full_channel_drops_without_waiting`
     must FAIL (timeout, or a panic from blocking in the runtime).
  3. In `Actor::for_command`, swap the `Slash`/`Prefix` branches.
     `the_constructors_set_source_user_and_command` must FAIL.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/music/audit.rs crack-core/src/music/mod.rs crack-core/src/utils.rs
git commit -m "feat(audit): the queue audit types, track_ref and a non-blocking emit

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: The table, the writer, and startup wiring

**Files:**
- Create: `migrations/20260930120000_queue_audit.sql` and a byte-identical `crack-core/test_migrations/20260930120000_queue_audit.sql`
- Create: `crack-core/src/db/queue_audit.rs`; Modify: `crack-core/src/db/mod.rs` (`pub mod queue_audit;`)
- Modify: `crack-core/src/lib.rs` (the `DataInner.audit_tx` field and its `Default`)
- Modify: `crack-core/src/config.rs` (spawn the writer next to `gp_persist`)
- Modify: `.sqlx/` (regenerated)

**Interfaces:**
- Consumes: `music::audit::{AuditEvent, Action, Actor}` (Task 1).
- Produces:
  - `pub audit_tx: Option<tokio::sync::mpsc::Sender<crate::music::audit::AuditEvent>>`
    on `DataInner`, defaulting to `None`;
  - `pub async fn insert_audit_event(pool: &PgPool, e: &AuditEvent) -> sqlx::Result<()>`;
  - `pub fn spawn_audit_writer(pool: PgPool) -> mpsc::Sender<AuditEvent>`, with a
    capacity of 1024 (`AUDIT_CHANNEL_CAPACITY`).

- [ ] **Step 1: The migration** (write it to both paths, identical)

```sql
-- The queue audit log: every change to a guild's queue, who made it and how.
-- Spec: docs/superpowers/specs/2026-09-30-queue-audit-log-design.md
-- No foreign keys, on purpose: a missing user or guild_settings row must never
-- fail an insert.
CREATE TABLE IF NOT EXISTS queue_audit (
    id                BIGSERIAL PRIMARY KEY,
    at                TIMESTAMPTZ NOT NULL,
    guild_id          BIGINT NOT NULL,
    voice_channel_id  BIGINT,
    origin_channel_id BIGINT,
    actor_user_id     BIGINT,
    source            TEXT NOT NULL,
    command           TEXT NOT NULL,
    action            TEXT NOT NULL,
    detail            JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS queue_audit_guild_at ON queue_audit (guild_id, at DESC);
CREATE INDEX IF NOT EXISTS queue_audit_actor_at ON queue_audit (actor_user_id, at DESC);
```

- [ ] **Step 2: Write the failing db test** in `db/queue_audit.rs`

```rust
#[cfg(test)]
mod test {
    use super::*;
    use crate::music::audit::{Action, Actor, AuditEvent, TrackRef};
    use serenity::all::{GenericChannelId, GuildId, UserId};

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn an_event_round_trips_through_the_table(pool: PgPool) {
        let action = Action::Move {
            track: TrackRef { title: Some("t".into()), url: Some("https://x".into()) },
            from: 3,
            to: 1,
        };
        let e = AuditEvent {
            at: chrono::Utc::now(),
            guild_id: GuildId::new(11),
            voice_channel: None,
            actor: Actor::for_command(UserId::new(22), false, "movesong", Some(GenericChannelId::new(33))),
            action: action.clone(),
        };
        insert_audit_event(&pool, &e).await.unwrap();
        let row = sqlx::query!(
            r#"SELECT guild_id, actor_user_id, origin_channel_id, source, command, action,
                      detail AS "detail!: sqlx::types::Json<Action>"
               FROM queue_audit"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (row.guild_id, row.actor_user_id, row.origin_channel_id),
            (11, Some(22), Some(33))
        );
        assert_eq!((row.source.as_str(), row.command.as_str(), row.action.as_str()), ("slash", "movesong", "move"));
        assert_eq!(row.detail.0, action);
    }
}
```

- [ ] **Step 3: Implement `db/queue_audit.rs`**

```rust
//! Writes the queue audit log. See `music::audit`.

use crate::music::audit::AuditEvent;
use sqlx::{postgres::PgPool, types::Json};
use tokio::sync::mpsc;

/// Room for bursts, such as a playlist add while a clear runs. `emit` drops
/// rather than waits when this is full.
pub const AUDIT_CHANNEL_CAPACITY: usize = 1024;

/// Insert one event.
pub async fn insert_audit_event(pool: &PgPool, e: &AuditEvent) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO queue_audit
             (at, guild_id, voice_channel_id, origin_channel_id, actor_user_id,
              source, command, action, detail)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        e.at,
        e.guild_id.get() as i64,
        e.voice_channel.map(|c| c.get() as i64),
        e.actor.origin_channel().map(|c| c.get() as i64),
        e.actor.user().map(|u| u.get() as i64),
        e.actor.source().as_str(),
        e.actor.command(),
        e.action.name(),
        Json(&e.action) as _,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Start the writer and return its sender. An insert that fails is logged and
/// the writer carries on: the log is best-effort, never a reason to stop.
pub fn spawn_audit_writer(pool: PgPool) -> mpsc::Sender<AuditEvent> {
    let (tx, mut rx) = mpsc::channel::<AuditEvent>(AUDIT_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        while let Some(e) = rx.recv().await {
            if let Err(err) = insert_audit_event(&pool, &e).await {
                tracing::warn!("queue audit: insert failed in {}: {err}", e.guild_id);
            }
        }
    });
    tx
}
```

- [ ] **Step 4: Add `audit_tx` to `DataInner`** in `crack-core/src/lib.rs`, next to `gp_persist`

```rust
    /// Where queue audit events go (see `music::audit`). `None` without a
    /// database, and then `emit` logs them instead.
    pub audit_tx: Option<tokio::sync::mpsc::Sender<crate::music::audit::AuditEvent>>,
```

Add `audit_tx: None,` to `impl Default for DataInner`, next to `gp_persist: None,`.

- [ ] **Step 5: Wire it in `config.rs`**, after `let gp_persist = ...`

```rust
    // The queue audit log's writer. Without a database, events go to the log.
    let audit_tx = database_pool
        .clone()
        .map(crate::db::queue_audit::spawn_audit_writer);
```

Then add `audit_tx,` to the `DataInner { .. }` literal, next to `gp_persist,`.

- [ ] **Step 6: Regenerate `.sqlx` against a throwaway Postgres, then run the test red and green**

```bash
bash <<'EOF'
set -euo pipefail
cd /home/lothrop/projects/cracktunes
port=55433
if ss -ltn | grep -q ":$port "; then echo "port $port is busy; pick another"; exit 1; fi
docker --context default run --rm -d --name ct-audit-prepare \
  -e POSTGRES_PASSWORD=prepare -p 127.0.0.1:$port:5432 postgres:16-alpine
for i in $(seq 1 60); do
  docker --context default exec ct-audit-prepare pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
export DATABASE_URL="postgres://postgres:prepare@127.0.0.1:$port/postgres"
cargo sqlx migrate run --source migrations/
cargo sqlx prepare --workspace -- --tests --all
cargo test -p crack-core --lib --features db-tests queue_audit
EOF
```

Expected: `an_event_round_trips_through_the_table ... ok`.

**Sabotage** (with the container still up): change `e.action.name(),` to `"x",` in the
insert, and re-run only the test with the same `DATABASE_URL`. It must FAIL on the
`action` assertion. Restore the line. Then stop the container with
`docker --context default stop ct-audit-prepare`.

- [ ] **Step 7: Confirm the offline build**

Run: `SQLX_OFFLINE=true cargo check --workspace --all-targets`
Expected: it succeeds. `git status` shows new files under `.sqlx/`.

- [ ] **Step 8: Commit**

```bash
git add migrations/20260930120000_queue_audit.sql crack-core/test_migrations/20260930120000_queue_audit.sql \
  crack-core/src/db/queue_audit.rs crack-core/src/db/mod.rs crack-core/src/lib.rs crack-core/src/config.rs .sqlx
git commit -m "feat(audit): the queue_audit table and its writer

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: The guard carries the actor; every lock site names one

**Files:**
- Modify: `crack-core/src/music/lease.rs`
- Modify: every `lock_queue(` caller:
  - `commands/music/{pause,clear,skip,shuffle,voteskip,remove,resume,stop,gp}.rs`
  - `handlers/track_end.rs`
  - `music/{queue,remote}.rs`
  - `messaging/interface.rs` (test code)
- Modify: `crack-web/src/{backend,lib,routes,test_support}.rs` (the move carries its user)
- Modify: `crack-core/src/music/queue.rs`: **delete** the whole
  `mod queue_query_list_offset_ordering_tests` (lines ~1512–1642, from its
  `#[cfg(test)]` through its closing `}`). Owner's ruling: the only lint that could
  replace it (`await_holding_invalid_type`) fires on every legitimate
  `call.lock().await` under the guard.

**Interfaces:**
- Consumes: `Actor`, `Action`, `AuditEvent`, `emit` (Task 1) and `DataInner.audit_tx` (Task 2).
- Produces:
  - `Data::lock_queue(&self, guild_id: GuildId, as_: PlaybackOwner, actor: Actor) -> Result<QueueGuard, CrackedError>`;
  - `QueueGuard::actor(&self) -> &Actor`;
  - `QueueGuard::record(&self, voice: Option<songbird::id::ChannelId>, action: Action)`;
  - `remote::move_by_id(data, http, guild_id, mover: UserId, id, to_upcoming)`;
  - crack-web `Backend::move_track(&self, user: UserId, g: GuildId, id: Uuid, to: usize)`.

- [ ] **Step 1: Write the failing tests** in `lease.rs`'s test module

```rust
    #[tokio::test]
    async fn a_guard_records_as_its_actor_into_the_audit_channel() {
        use crate::music::audit::{Action, Actor, BotReason};
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let d = Data(Arc::new(DataInner { audit_tx: Some(tx), ..Default::default() }));
        let guard = d
            .lock_queue(G, PlaybackOwner::Free, Actor::bot(BotReason::Autopause))
            .await
            .unwrap();
        guard.record(None, Action::Pause);
        let e = rx.try_recv().expect("recorded");
        assert_eq!((e.guild_id, e.action, e.actor), (G, Action::Pause, Actor::bot(BotReason::Autopause)));
    }
```

And in `crack-web/src/routes.rs` tests (the existing move tests use `FakeBackend`):

```rust
    #[tokio::test]
    async fn a_dashboard_move_is_recorded_as_the_mover() {
        let fake = FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), QueueView::Idle);
        let r = app(fake.clone()).oneshot(move_request()).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(fake.last_mover(), Some(UserId::new(9)));
    }
```

Match `move_request()`'s session user. It signs in user 9 (`session(9)`); if it differs,
assert that user. Add `last_mover(&self) -> Option<UserId>` to `FakeBackend`, which stores
the `user` argument of its `move_track`.

Also in `remote.rs`'s test module, if it has a move test with an offline call, add
an audit channel and assert the recorded actor is `Actor::web(mover)`. Otherwise the
crack-web test above plus Task 4's primitive tests cover the path.

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test -p crack-core --lib lease` and `cargo test -p crack-web --lib dashboard_move_is_recorded`
Expected: compile FAILs (no `actor` argument, no `record`, no `last_mover`).

- [ ] **Step 3: Implement it in `lease.rs`**

```rust
pub struct QueueGuard {
    guild_id: GuildId,
    /// Who holds this guard; every change made under it is recorded as them.
    actor: crate::music::audit::Actor,
    /// A clone of `Data::audit_tx`.
    audit_tx: Option<tokio::sync::mpsc::Sender<crate::music::audit::AuditEvent>>,
    /// Dropping this releases the per-guild mutex. Never read.
    _exclusion: OwnedMutexGuard<()>,
}

impl QueueGuard {
    #[must_use]
    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    /// Who this guard acts for.
    #[must_use]
    pub fn actor(&self) -> &crate::music::audit::Actor {
        &self.actor
    }

    /// Record a change made under this guard. Call it only after the change
    /// succeeded. Never waits and never fails: see `audit::emit`.
    pub fn record(&self, voice: Option<songbird::id::ChannelId>, action: crate::music::audit::Action) {
        crate::music::audit::emit(
            self.audit_tx.as_ref(),
            crate::music::audit::AuditEvent {
                at: chrono::Utc::now(),
                guild_id: self.guild_id,
                voice_channel: voice.map(|c| serenity::all::ChannelId::new(c.0.get())),
                actor: self.actor.clone(),
                action,
            },
        );
    }
}
```

`lock_queue` gains `actor: crate::music::audit::Actor` as its last parameter and builds
`QueueGuard { guild_id, actor, audit_tx: self.audit_tx.clone(), _exclusion: lock.lock_owned().await }`.
Update its doc comment: the actor is who every change made under the guard is recorded as.
`QueueGuard` loses `Debug` if `Sender` isn't `Debug`. Keep `#[derive(Debug)]`: both
`Actor` and `Sender` implement it.

- [ ] **Step 4: Give every call site its actor** (the compiler lists them)

| Site | Actor |
|---|---|
| commands in `commands/music/*.rs`, and `ctx`-taking functions in `music/queue.rs` | `Actor::from_ctx(&ctx)` |
| `gp.rs` (all six, including the `Free` one at ~2299, which stops a discarded game's queue) | `Actor::bot(BotReason::Game)` |
| `track_end.rs` ~174 (autopause) | `Actor::bot(BotReason::Autopause)` |
| `track_end.rs` ~404 (`enqueue_resolved_autoplay`) | `Actor::bot(BotReason::Autoplay)` |
| `remote.rs` `move_by_id` | `Actor::web(mover)` |
| test code (`queue.rs`, `lease.rs`, `skip.rs`, `interface.rs` tests) | `Actor::bot(BotReason::Autopause)`, or a test-local `fn actor() -> Actor` returning it |

For `remote::move_by_id`, add `mover: UserId` after `guild_id`. In crack-web:
- `Backend::move_track` gains `user: UserId` first;
- `LiveBackend` passes it to `move_by_id`;
- `routes.rs` `move_track` passes the signed-in `user` it already logs;
- `FakeBackend::move_track` stores it for `last_mover`.

Delete the ordering-test module named under **Files** above.

- [ ] **Step 5: Run everything**

Run: `cargo test -p crack-core --lib && cargo test -p crack-web`
Expected: all pass, including the two new tests.

- [ ] **Step 6: Sabotage, one at a time, restoring each**

  1. In `QueueGuard::record`, pass `Actor::bot(BotReason::Game)` instead of `self.actor.clone()`.
     `a_guard_records_as_its_actor_into_the_audit_channel` must FAIL.
  2. In crack-web `routes.rs`, pass a fixed `UserId::new(1)` to `backend.move_track`.
     `a_dashboard_move_is_recorded_as_the_mover` must FAIL.

- [ ] **Step 7: Commit**

```bash
git add crack-core/src crack-web/src
git commit -m "feat(audit): the queue guard carries who is acting; every lock names them

Deletes queue_query_list_offset_ordering_tests (owner's ruling: no source
scans, and no lint can express it).

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

(`git add` of the two `src` directories is fine here; it stages only tracked-path
changes under them. Check `git status` first for strays.)

---

### Task 4: Every queue primitive records what it did

**Files:**
- Modify: `crack-core/src/music/queue.rs`
- Modify: `crack-core/src/commands/music/skip.rs` (move `force_skip_top_track` and its test
  into `queue.rs`, and re-import it where it's used: `skip.rs`, `voteskip.rs`, and any
  other caller the compiler names)

**Interfaces:**
- Consumes: `QueueGuard::record` and `audit::{track_ref, Action, AddAt, TrackRef}`.
- Produces: no new signatures. `force_skip_top_track` keeps its signature and moves to
  `crate::music::queue`.

What each primitive records (`voice` is `handler.current_channel()`):

| Primitive | Records | When nothing |
|---|---|---|
| `clear_from(from)` | `Clear { removed }`, the number drained | `removed == 0` |
| `drain_after_current(count)` | `Clear { removed }`, the number drained. It is `/skip N`'s drain; the skip itself is recorded by `force_skip_top_track` | `removed == 0` |
| `shuffle_behind_current` | `Shuffle { count }`, where count is the number of upcoming tracks shuffled | `count < 2` |
| `move_track(at, to)` | `Move { track, from: at, to }` | `at == to` |
| `move_track_by_id` | `Move { track, from: at - 1, to: to - 1 }` (upcoming-relative, like its return value) | `Err(_)`, or the same position |
| `remove_at(index)` | `Remove { track, index }` | no track at `index` |
| `stop_queue` | `Stop { removed }`, the queue length before | empty queue |
| `pause_queue` | `Pause` | `Err(_)` |
| `resume_queue` | `Resume` | `Err(_)` |
| `force_skip_top_track` | `Skip { track: Some(ref of current) }` | nothing playing |
| each enqueue entry point | `Add { tracks, at }`, **one record per call**, listing every handle it queued via `track_ref` | no track queued |

The enqueue entry points (every caller of the private `fn enqueue`, at lines ~49, 177,
290, 330, 754, 948 and 974) collect their handles and record once:
- `queue_resolved_track_back` and `enqueue_resolved_tracks_back` record `AddAt::Back`;
- `queue_track_ready_front` records `AddAt::Front`;
- `_queue_track_ready_back`, `enqueue_track_back` and `enqueue_input_back` record `AddAt::Back`;
- `queue_query_list_offset` records `AddAt::Index(offset)`.

For the async ones that lock `call` themselves, read `voice` from that `handler`.

- [ ] **Step 1: Write the failing tests** in `queue.rs`'s `mod test`

Change the `queue_of` helper to build `Data` with an audit channel and return the
receiver, drained of the setup adds:

```rust
    async fn queue_of(n: usize) -> (Data, Arc<Mutex<Call>>, Vec<uuid::Uuid>, tokio::sync::mpsc::Receiver<AuditEvent>) {
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let data = Data(Arc::new(DataInner { audit_tx: Some(tx), ..Default::default() }));
        // ... the existing enqueue loop, unchanged, with lock_queue(GUILD, PlaybackOwner::Free, actor()) ...
        while rx.try_recv().is_ok() {}
        (data, call, ids, rx)
    }

    fn actor() -> Actor {
        Actor::bot(BotReason::Autopause)
    }
```

Update the existing callers of `queue_of` to ignore the fourth value (`let (data, call, ids, _rx) = ...`).
Then add:

```rust
    fn next(rx: &mut tokio::sync::mpsc::Receiver<AuditEvent>) -> Action {
        rx.try_recv().expect("one record").action
    }
    fn t(i: usize) -> TrackRef {
        TrackRef { title: Some(format!("t{i}")), url: None }
    }

    #[tokio::test]
    async fn each_primitive_records_exactly_what_it_did() {
        let (data, call, ids, mut rx) = queue_of(5).await;
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free, actor()).await.unwrap();
        {
            let h = call.lock().await;
            move_track(&guard, &h, 3, 1);
            assert_eq!(next(&mut rx), Action::Move { track: t(3), from: 3, to: 1 });
            remove_at(&guard, &h, 1);
            assert_eq!(next(&mut rx), Action::Remove { track: t(3), index: 1 });
            shuffle_behind_current(&guard, &h);
            assert_eq!(next(&mut rx), Action::Shuffle { count: 3 });
            pause_queue(&guard, &h).ok();
            assert_eq!(next(&mut rx), Action::Pause);
            resume_queue(&guard, &h).ok();
            assert_eq!(next(&mut rx), Action::Resume);
            clear_from(&guard, &h, 2);
            assert_eq!(next(&mut rx), Action::Clear { removed: 2 });
            stop_queue(&guard, &h);
            assert_eq!(next(&mut rx), Action::Stop { removed: 2 });
        }
        let _ = ids;
        assert!(rx.try_recv().is_err(), "nothing extra");
    }

    #[tokio::test]
    async fn a_dashboard_move_records_upcoming_positions() {
        let (data, call, ids, mut rx) = queue_of(4).await;
        assert_eq!(move_in(&data, &call, ids[3], 0).await, Ok(0));
        assert_eq!(next(&mut rx), Action::Move { track: t(3), from: 2, to: 0 });
    }

    #[tokio::test]
    async fn a_batch_add_is_one_record_listing_every_track() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let data = Data(Arc::new(DataInner { audit_tx: Some(tx), ..Default::default() }));
        let call = offline_call();
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free, actor()).await.unwrap();
        // Use the batch entry point (`enqueue_resolved_tracks_back`) with three
        // resolved tracks titled t0..t2, built as its existing tests build them.
        // ...
        assert_eq!(next(&mut rx), Action::Add { tracks: vec![t(0), t(1), t(2)], at: AddAt::Back });
        assert!(rx.try_recv().is_err(), "one record, not three");
    }

    #[tokio::test]
    async fn no_ops_record_nothing() {
        let (data, call, _ids, mut rx) = queue_of(1).await;
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free, actor()).await.unwrap();
        {
            let h = call.lock().await;
            move_track(&guard, &h, 0, 0);
            clear_from(&guard, &h, 1);
            drain_after_current(&guard, &h, 3);
            shuffle_behind_current(&guard, &h);
            remove_at(&guard, &h, 7);
        }
        assert!(rx.try_recv().is_err());
        let empty = offline_call();
        let h = empty.lock().await;
        stop_queue(&guard, &h);
        force_skip_top_track(&guard, &h).await.unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn skipping_records_the_track_that_was_playing() {
        let (data, call, _ids, mut rx) = queue_of(2).await;
        let guard = data.lock_queue(GUILD, PlaybackOwner::Free, actor()).await.unwrap();
        let h = call.lock().await;
        force_skip_top_track(&guard, &h).await.unwrap();
        assert_eq!(next(&mut rx), Action::Skip { track: Some(t(0)) });
    }
```

For `a_batch_add_is_one_record_listing_every_track`, the implementer fills the elided
lines by calling `enqueue_resolved_tracks_back` the way any existing test or caller
builds its `ResolvedTrack` slice. If none can be built offline, use
`enqueue_input_back` three times through a new private helper **only if** the batch
function genuinely can't run offline. In that case, record the substitution in the report
and keep the "one record per call" assertion on whichever entry point does take
several tracks offline. The review focus is that a multi-track call yields exactly one record.

`pause_queue` and `resume_queue` on an offline call with a queued track return `Ok`.
If either returns `Err` offline, assert nothing is recorded for it instead, and say
so in the report.

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test -p crack-core --lib music::queue::test`
Expected: FAIL. No records arrive (the `expect("one record")` panics).

- [ ] **Step 3: Implement recording in each primitive, per the table**

The pattern for synchronous primitives. The count and the track are read inside
`modify_queue`, and the record is made after it returns:

```rust
pub fn move_track(guard: &QueueGuard, handler: &Call, at: usize, to: usize) {
    if at == to {
        return;
    }
    let track = handler.queue().modify_queue(|queue| {
        // The caller verifies both indices are in range before calling.
        let song = queue.remove(at).expect("index out of bounds");
        let r = crate::music::audit::track_ref(song.handle());
        queue.insert(to, song);
        r
    });
    guard.record(handler.current_channel(), Action::Move { track, from: at, to });
}
```

`queue.remove(at)` yields a songbird `Queued`. Use its `.handle()` (or its `Deref` to
`TrackHandle`, whichever songbird exposes) with `track_ref`.

For `clear_from` and `drain_after_current`, compute `removed` as the drained range's
length inside the closure. For `stop_queue`, read `handler.queue().len()` before
stopping. For `force_skip_top_track`, compute `track_ref(&current)` before `stop()`,
and record only if there was a current track.

Move `force_skip_top_track` (with its doc comment and its test
`skipping_an_already_empty_queue_is_a_no_op_not_a_panic`) from `skip.rs` into
`queue.rs`, and fix the imports the compiler names.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p crack-core --lib`
Expected: all pass.

- [ ] **Step 5: Sabotage, one at a time, restoring each**

  1. Remove the `at == to` early return in `move_track`. `no_ops_record_nothing` must FAIL.
  2. In the batch enqueue, record inside the per-track loop instead of once.
     `a_batch_add_is_one_record_listing_every_track` must FAIL.
  3. In `move_track_by_id`, record `from: at` (not `at - 1`).
     `a_dashboard_move_records_upcoming_positions` must FAIL.
  4. In `force_skip_top_track`, compute the `TrackRef` after `dequeue(0)`.
     `skipping_records_the_track_that_was_playing` must FAIL.

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/music/queue.rs crack-core/src/commands/music/skip.rs crack-core/src/commands/music/voteskip.rs
git commit -m "feat(audit): every queue primitive records what it did

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

(Add any other file the move of `force_skip_top_track` touched, by name.)

---

### Task 5: `music::disconnect`, the audited way out of voice

**Files:**
- Create: `crack-core/src/music/disconnect.rs`; Modify: `crack-core/src/music/mod.rs` (`pub mod disconnect;`)
- Modify:
  - `crack-core/src/commands/music/leave.rs` (~39);
  - `crack-core/src/handlers/idle.rs` (~71);
  - `crack-core/src/handlers/serenity.rs` (~328);
  - `crack-core/src/commands/music_utils.rs` (~220).

**Interfaces:**
- Consumes: `audit::{emit, Action, Actor, AuditEvent, BotReason}` and `Data.audit_tx`.
- Produces: `pub async fn disconnect(data: &Data, manager: &Songbird, guild_id: GuildId, actor: Actor) -> Result<(), songbird::error::JoinError>`.
  It returns exactly what `manager.remove(guild_id)` returned, so each site keeps its
  own error handling unchanged.

- [ ] **Step 1: Write the failing tests** in `disconnect.rs`

```rust
#[cfg(test)]
mod test {
    use super::*;
    use crate::music::audit::{Action, Actor, BotReason};
    use crate::{Data, DataInner};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(5);

    fn data_with_audit() -> (Data, tokio::sync::mpsc::Receiver<AuditEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        (Data(Arc::new(DataInner { audit_tx: Some(tx), ..Default::default() })), rx)
    }

    #[tokio::test]
    async fn disconnect_records_how_many_tracks_it_discarded() {
        let (data, mut rx) = data_with_audit();
        let manager = songbird::Songbird::serenity();
        // Register a Call without connecting, and queue two offline tracks on it
        // through `enqueue_input_back` (with a guard), as queue.rs's tests do.
        // ...
        let _ = disconnect(&data, &manager, G, Actor::bot(BotReason::IdleTimeout)).await;
        let e = rx.try_recv().expect("recorded");
        assert_eq!(e.action, Action::Leave { discarded: 2 });
        assert_eq!(e.actor, Actor::bot(BotReason::IdleTimeout));
    }

    #[tokio::test]
    async fn disconnecting_with_nothing_queued_records_nothing() {
        let (data, mut rx) = data_with_audit();
        let manager = songbird::Songbird::serenity();
        let _ = disconnect(&data, &manager, G, Actor::bot(BotReason::Kicked)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn disconnect_does_not_wait_for_the_lease() {
        let (data, _rx) = data_with_audit();
        let manager = songbird::Songbird::serenity();
        let _held = data
            .lock_queue(G, crate::music::PlaybackOwner::Free, Actor::bot(BotReason::Game))
            .await
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            disconnect(&data, &manager, G, Actor::bot(BotReason::Kicked)),
        )
        .await
        .expect("disconnect must not take the queue lease");
    }
}
```

To register a `Call` offline, use `manager.get_or_insert(G)`. It is not banned, and
it creates the `Call` without joining. If that call is unavailable on the
`Songbird::serenity()` manager, record why in the report and test `disconnect`'s
counting through the smallest seam that works, e.g. a private
`fn discarded(handler: &Call) -> usize` tested directly. Keep the no-lease test as written.

- [ ] **Step 2: Run them to watch them fail**

Run: `cargo test -p crack-core --lib music::disconnect`
Expected: a compile FAIL (no `disconnect`).

- [ ] **Step 3: Implement it**

```rust
//! Leaving voice discards the queue, and it does so without the queue lease:
//! `/leave` must work even during a `/gp` game. This is the one way out, so
//! the discard is recorded like any other queue change. See `music::audit`.

use crate::music::audit::{emit, Action, Actor, AuditEvent};
use crate::Data;
use serenity::all::{ChannelId, GuildId};
use songbird::Songbird;

/// Record what leaving will discard, then leave. Returns `manager.remove`'s
/// result unchanged, so each caller keeps its own handling.
pub async fn disconnect(
    data: &Data,
    manager: &Songbird,
    guild_id: GuildId,
    actor: Actor,
) -> Result<(), songbird::error::JoinError> {
    // Raw `get` on purpose: what is registered is what `remove` will discard,
    // connected or not.
    #[allow(clippy::disallowed_methods)]
    let call = manager.get(guild_id);
    if let Some(call) = call {
        let (discarded, voice) = {
            let h = call.lock().await;
            (h.queue().len(), h.current_channel())
        };
        if discarded > 0 {
            emit(
                data.audit_tx.as_ref(),
                AuditEvent {
                    at: chrono::Utc::now(),
                    guild_id,
                    voice_channel: voice.map(|c| ChannelId::new(c.0.get())),
                    actor,
                    action: Action::Leave { discarded },
                },
            );
        }
    }
    #[allow(clippy::disallowed_methods)] // the one audited caller; see clippy.toml
    manager.remove(guild_id).await
}
```

- [ ] **Step 4: Route the four sites through it**, keeping each site's existing handling of the result

| Site | Actor |
|---|---|
| `leave.rs` | `Actor::from_ctx(&ctx)` |
| `idle.rs` | `Actor::bot(BotReason::IdleTimeout)` |
| `serenity.rs` (bot disconnected or kicked) | `Actor::bot(BotReason::Kicked)` |
| `music_utils.rs` (failed-join cleanup) | `Actor::bot(BotReason::JoinCleanup)` |

Each site replaces `manager.remove(guild_id).await` with
`crate::music::disconnect::disconnect(&data, &manager, guild_id, actor).await`.
Get `data` the way that function already reaches it; every one of the four has
`Data` in scope or one call away. Leave the surrounding comments in place.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p crack-core --lib`
Expected: all pass.

- [ ] **Step 6: Sabotage, one at a time, restoring each**

  1. Take the lease inside `disconnect`
     (`let _g = data.lock_queue(guild_id, PlaybackOwner::Free, actor.clone()).await;`).
     `disconnect_does_not_wait_for_the_lease` must FAIL.
  2. Drop the `discarded > 0` check. `disconnecting_with_nothing_queued_records_nothing`
     must FAIL. (That holds if an unregistered guild yields no `Call`. If `get` returns
     `None` there, the sabotage passes vacuously. Then register an empty `Call` in that
     test first and re-run it.)

- [ ] **Step 7: Commit**

```bash
git add crack-core/src/music/disconnect.rs crack-core/src/music/mod.rs crack-core/src/commands/music/leave.rs \
  crack-core/src/handlers/idle.rs crack-core/src/handlers/serenity.rs crack-core/src/commands/music_utils.rs
git commit -m "feat(audit): leaving voice goes through one audited disconnect

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 6: `clippy.toml` closes every other door

**Files:**
- Modify: `clippy.toml`
- Modify: `crack-core/src/music/queue.rs`, plus any other file clippy then names, where an `#[allow]` with a reason is added

**Interfaces:** none.

- [ ] **Step 1: Add the bans** at the end of `disallowed-methods`, under a comment in
  the file's style:

```toml
    # 🪤 The queue audit log (docs/superpowers/specs/2026-09-30-queue-audit-log-design.md)
    # is recorded by the primitives in `music::queue` and by `music::disconnect`.
    # A change made anywhere else is a change nobody can account for, so these
    # close every other door, as the Songbird::get ban above does for joins.
    { path = "songbird::tracks::TrackQueue::skip", reason = "changes the queue without an audit record; use the primitives in music::queue" },
    { path = "songbird::tracks::TrackQueue::stop", reason = "changes the queue without an audit record; use music::queue::stop_queue" },
    { path = "songbird::tracks::TrackQueue::dequeue", reason = "changes the queue without an audit record; use the primitives in music::queue" },
    { path = "songbird::tracks::TrackQueue::modify_queue", reason = "changes the queue without an audit record; use the primitives in music::queue" },
    { path = "songbird::tracks::TrackQueue::pause", reason = "changes the queue without an audit record; use music::queue::pause_queue" },
    { path = "songbird::tracks::TrackQueue::resume", reason = "changes the queue without an audit record; use music::queue::resume_queue" },
    { path = "songbird::Songbird::remove", reason = "discards the queue without an audit record; use music::disconnect::disconnect" },
    { path = "songbird::Songbird::leave", reason = "discards the queue without an audit record; use music::disconnect::disconnect" },
    { path = "songbird::Call::leave", reason = "discards the queue without an audit record; use music::disconnect::disconnect" },
    { path = "songbird::Call::stop", reason = "stops the whole queue without an audit record; use music::queue::stop_queue" },
```

- [ ] **Step 2: Run clippy to see what it flags**

Run: `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings`
Expected: errors in `music/queue.rs`, which are the primitives, and possibly elsewhere.

- [ ] **Step 3: Resolve each hit**

- Inside `music::queue`'s primitives (and `disconnect.rs`, which already has its
  `#[allow]`s), put `#[allow(clippy::disallowed_methods)]` on the primitive function
  with a one-line comment: `// An audited primitive: records below. See clippy.toml.`
- **Anywhere else**, the hit is a real unaudited change. Route it through a
  primitive, or, if it only *reads* (none of the banned methods only reads), stop and
  report it. Do not blanket-`allow` outside `music::queue` and `music::disconnect`.
- Test code that calls a banned method directly (for example to set up a state) may
  carry a scoped `#[allow]` with a `// test setup` reason.

- [ ] **Step 4: Run clippy and all tests**

Run: `SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, and all pass.

- [ ] **Step 5: Sabotage.** Add `let _ = handler.queue().skip();` inside `/pause`'s
  command body in `commands/music/pause.rs`. Clippy must FAIL naming
  `songbird::tracks::TrackQueue::skip`. Remove the line.

- [ ] **Step 6: Commit**

```bash
git add clippy.toml crack-core/src
git commit -m "build(clippy): only the audited primitives may change a queue

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 7: Operator notes and the version

**Files:**
- Create: `docs/queue-audit.md`
- Modify: `Cargo.toml` (workspace `version` `0.15.1` → `0.16.0`), `Cargo.lock` (via `cargo update -w`)

- [ ] **Step 1: Write `docs/queue-audit.md`**

```markdown
# Queue audit log

Every change to a guild's queue is written to `queue_audit`: who (`actor_user_id`,
NULL for the bot), how (`source`: slash, prefix, web, bot; `command`), where
(`voice_channel_id`, `origin_channel_id`), when (`at`), and what (`action`, with
the details in `detail` as JSON). Design:
`docs/superpowers/specs/2026-09-30-queue-audit-log-design.md`.

Not recorded: a track ending on its own (see `play_log`); refused attempts; volume,
seek, repeat and the autoplay settings. Without a database, events go to the bot's
log at `info` as `queue audit (no database)`. A full writer channel drops an event
with a `warn`; commands never wait on the log.

⚠️ `/gp` rows (`command = 'gp'`) carry the round's song titles. Any reader shown to
players must hide a guild's game rows while its game is running.

## Example queries

    -- The last 20 changes in a guild
    SELECT at, actor_user_id, source, command, action, detail
    FROM queue_audit WHERE guild_id = $1 ORDER BY at DESC LIMIT 20;

    -- Who has been moving tracks from the web, this week
    SELECT actor_user_id, count(*) FROM queue_audit
    WHERE source = 'web' AND at > now() - interval '7 days'
    GROUP BY 1 ORDER BY 2 DESC;

    -- Everything the bot did on its own in a guild today
    SELECT at, command, action, detail FROM queue_audit
    WHERE guild_id = $1 AND source = 'bot' AND at > now() - interval '1 day'
    ORDER BY at;
```

- [ ] **Step 2: Bump the version**

Run: `sed -i 's/^version = "0.15.1"/version = "0.16.0"/' Cargo.toml && cargo update -w`
Then check that `git diff Cargo.lock` changes only the workspace crates' version lines.

- [ ] **Step 3: Full checks**

Run: `cargo fmt --all -- --check && SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean, and all pass.

- [ ] **Step 4: Commit**

```bash
git add docs/queue-audit.md Cargo.toml Cargo.lock
git commit -m "v0.16.0: the queue audit log

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```
