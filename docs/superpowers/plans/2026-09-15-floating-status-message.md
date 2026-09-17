# Floating Status Message Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the track-end "Now playing" post with one per-guild status message that edits in place while it is the newest message in its channel and moves to the bottom otherwise, with a saved `ephemeral_replies` guild setting for the status-related command replies.

**Architecture:** A new `crack-core/src/messaging/status.rs` owns all status-message I/O. Pure functions decide placement (`placement`) and channel (`target_channel`); an executor (`apply`) talks to Discord through a `StatusTransport` trait so every branch is unit-tested against a fake. Per-guild state lives in `DataInner::status_slots` behind one `tokio::sync::Mutex` per guild. Triggers (track end, `/play`, `/skip`, `/nowplaying`, stop/leave/idle/kicked) call `show_now_playing` / `show_finished`.

**Tech Stack:** Rust 2021, serenity (git 37b9f43, `next` API: `GenericChannelId`, `MessageId`, cache `BaseGuildChannel::last_message_id`), poise, songbird (git 3fe7289), sqlx 0.8 with offline `.sqlx`, tokio, dashmap, async-trait via `serenity::async_trait`.

**Spec:** `docs/superpowers/specs/2026-09-15-floating-status-message-design.md` — read it before Task 1. Where this plan and the spec disagree, the spec wins; record the ruling.

## Global Constraints

- Repo: `/home/lothrop/projects/cracktunes`, branch `feat/floating-status-message` (from master `989ee18a`, v0.12.1).
- Gate before every commit that touches Rust: `cargo fmt --all -- --check`, `SQLX_OFFLINE=true cargo clippy --workspace --all-targets --locked -- -D warnings`, `SQLX_OFFLINE=true cargo test --workspace --locked` (the lock refresh in Task 7 is the one exception to `--locked`).
- Every new test is written **red first** (run it and see it fail for the stated reason — a compile error naming the missing item counts), then made green, then **sabotaged**: break the code it guards, watch it fail, restore the file byte-for-byte (compare `sha256sum` before/after).
- Commit messages end with exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other `Co-Authored-By` line. Stage explicit paths; never `git add -A` or `git add .`.
- Typed serde only: no `serde_json::json!` / `serde_json::Value` for data we own.
- `clippy.toml` bans songbird's `Track` constructors, queue/play methods, `TrackHandle::data`, `add_global_event`, `TrackHandle::add_event`, and rusty_ytdl's `stream` methods. Do not add `#[allow(clippy::disallowed_methods)]` in this work; read track metadata with `crate::utils::get_track_handle_metadata`.
- 🔑 **Lock order:** never hold a songbird `Call` lock (`call.lock().await` guard) while calling `show_now_playing` / `show_finished` — they take the Call lock and then the guild's status-slot lock themselves.
- 🔑 **DashMap refs never cross an `.await`:** clone the `Arc` out of the entry in the same statement (see `crack-core/src/music/lease.rs::lock_queue`).
- The Bash tool runs zsh. Use `bash <<'EOF' ... EOF` for loops, `PIPESTATUS` or `set --`. Give stdin-reading commands `</dev/null`.
- Never build container images locally. `docker run` of the already-present `postgres:16-alpine` image is allowed (Task 4) with `--pull never`. Never use `docker-compose-postgres.yml`, and never touch the `runecast-*` or `chaotic-nick-names-db-1` containers.
- Feature → all ten members bump `0.12.1 → 0.13.0` (Task 7).

---

### Task 1: Status core — placement, channel choice, executor

**Files:**
- Create: `crack-core/src/messaging/status.rs`
- Modify: `crack-core/src/messaging/mod.rs`
- Modify: `crack-core/src/http_utils.rs` (add `is_unknown_message`)
- Modify: `crack-core/src/commands/utility/clean.rs` (use the shared helper)
- Modify: `crack-core/src/lib.rs` (`DataInner::status_slots` field + `Default`)

**Interfaces:**
- Produces (all in `crate::messaging::status`):
  - `pub enum Phase { Playing, Finished }` (Debug, Clone, Copy, PartialEq, Eq)
  - `pub struct StatusMessage { pub channel: GenericChannelId, pub id: MessageId, pub phase: Phase }` (Debug, Clone, Copy, PartialEq, Eq)
  - `pub struct StatusSlot { pub message: Option<StatusMessage>, pub last_command_channel: Option<GenericChannelId> }` (Debug, Default)
  - `pub enum Placement { Edit, Replace, Send }`
  - `pub fn placement(current: Option<&StatusMessage>, target: GenericChannelId, channel_last: Option<MessageId>) -> Placement`
  - `pub fn target_channel(music: Option<GenericChannelId>, last_command: Option<GenericChannelId>, tracked: Option<GenericChannelId>) -> Option<GenericChannelId>`
  - `pub enum TransportError { UnknownMessage, Other(String) }` + `impl From<serenity::Error> for TransportError`
  - `#[async_trait] pub trait StatusTransport: Send + Sync` with `send`, `edit`, `delete`, `last_message_id` (signatures in Step 3)
  - `pub async fn apply(transport: &dyn StatusTransport, slot: &mut StatusSlot, guild: GuildId, target: GenericChannelId, embed: CreateEmbed<'static>, phase: Phase) -> Option<StatusMessage>`
  - `impl crate::Data { pub fn status_slot(&self, guild: GuildId) -> Arc<tokio::sync::Mutex<StatusSlot>> }`
  - `crate::http_utils::is_unknown_message(err: &serenity::Error) -> bool`

- [ ] **Step 1: Move `is_unknown_message` to `http_utils`**

In `crack-core/src/http_utils.rs`, add after the imports:

```rust
/// Whether Discord refused a request because the message is already gone
/// (JSON error 10008, Unknown Message) -- deleted by hand, say.
///
/// Shared by `/clean` and the status message, which both treat an
/// already-deleted message as settled rather than as a failure.
pub fn is_unknown_message(err: &serenity::Error) -> bool {
    matches!(
        err,
        serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response))
            if response.error.code == serenity::http::JsonErrorCode::UnknownMessage
    )
}
```

In `crack-core/src/commands/utility/clean.rs`: delete the local `fn is_unknown_message` (with its doc comment) and the now-unused `use serenity::http::{HttpError, JsonErrorCode};`, and add `use crate::http_utils::is_unknown_message;` to the imports. The two existing clean tests (`a_message_discord_no_longer_knows_counts_as_already_gone`, `a_refused_delete_is_not_taken_for_an_already_gone_message`) keep passing through `use super::*`.

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib commands::utility::clean`
Expected: every clean test passes, including the two `is_unknown_message` tests named above.

- [ ] **Step 2: Write the failing tests**

Create `crack-core/src/messaging/status.rs` containing only the module doc and this test module (the items it names do not exist yet):

```rust
//! The floating status message: one per guild, edited in place while it is
//! still the newest message in its channel and moved to the bottom otherwise.
//! Spec: docs/superpowers/specs/2026-09-15-floating-status-message-design.md

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const GUILD: GuildId = GuildId::new(1);

    fn ch(id: u64) -> GenericChannelId {
        GenericChannelId::new(id)
    }

    fn tracked(channel: u64, id: u64, phase: Phase) -> StatusMessage {
        StatusMessage { channel: ch(channel), id: MessageId::new(id), phase }
    }

    fn embed() -> CreateEmbed<'static> {
        CreateEmbed::new().title("status")
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Op {
        Send(u64),
        Edit(u64, u64),
        Delete(u64, u64),
    }

    /// A stand-in Discord: records every call, answers from what the test set.
    #[derive(Default)]
    struct Fake {
        last: std::sync::Mutex<Option<MessageId>>,
        edit_error: std::sync::Mutex<Option<TransportError>>,
        delete_error: std::sync::Mutex<Option<TransportError>>,
        send_error: std::sync::Mutex<Option<TransportError>>,
        ops: std::sync::Mutex<Vec<Op>>,
        sent: AtomicU64,
    }

    impl Fake {
        fn with_last(self, last: u64) -> Self {
            *self.last.lock().unwrap() = Some(MessageId::new(last));
            self
        }
        fn ops(&self) -> Vec<Op> {
            self.ops.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl StatusTransport for Fake {
        async fn send(
            &self,
            channel: GenericChannelId,
            _embed: CreateEmbed<'static>,
        ) -> Result<MessageId, TransportError> {
            self.ops.lock().unwrap().push(Op::Send(channel.get()));
            if let Some(err) = self.send_error.lock().unwrap().clone() {
                return Err(err);
            }
            Ok(MessageId::new(1000 + self.sent.fetch_add(1, Ordering::SeqCst)))
        }

        async fn edit(
            &self,
            channel: GenericChannelId,
            id: MessageId,
            _embed: CreateEmbed<'static>,
        ) -> Result<(), TransportError> {
            self.ops.lock().unwrap().push(Op::Edit(channel.get(), id.get()));
            match self.edit_error.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }

        async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError> {
            self.ops.lock().unwrap().push(Op::Delete(channel.get(), id.get()));
            match self.delete_error.lock().unwrap().clone() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }

        fn last_message_id(&self, _guild: GuildId, _channel: GenericChannelId) -> Option<MessageId> {
            *self.last.lock().unwrap()
        }
    }

    // ---- placement ----

    #[test]
    fn nothing_tracked_is_sent() {
        assert_eq!(placement(None, ch(5), Some(MessageId::new(9))), Placement::Send);
    }

    #[test]
    fn a_status_in_another_channel_is_moved() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(6), Some(MessageId::new(100))), Placement::Replace);
    }

    #[test]
    fn a_message_posted_since_moves_the_status() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(5), Some(MessageId::new(101))), Placement::Replace);
    }

    #[test]
    fn a_quiet_channel_edits_in_place() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(5), Some(MessageId::new(100))), Placement::Edit);
    }

    /// Our own send has not echoed back through the gateway yet, so the cache
    /// still holds an older id. That is not "someone posted".
    #[test]
    fn our_own_send_not_yet_echoed_still_edits() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(5), Some(MessageId::new(99))), Placement::Edit);
    }

    #[test]
    fn an_uncached_channel_moves_rather_than_guesses() {
        let current = tracked(5, 100, Phase::Playing);
        assert_eq!(placement(Some(&current), ch(5), None), Placement::Replace);
    }

    // ---- target_channel ----

    #[test]
    fn the_music_channel_wins() {
        assert_eq!(target_channel(Some(ch(7)), Some(ch(5)), Some(ch(6))), Some(ch(7)));
    }

    #[test]
    fn then_the_last_command_channel() {
        assert_eq!(target_channel(None, Some(ch(5)), Some(ch(6))), Some(ch(5)));
    }

    #[test]
    fn then_wherever_the_status_already_is() {
        assert_eq!(target_channel(None, None, Some(ch(6))), Some(ch(6)));
    }

    #[test]
    fn with_nowhere_known_there_is_no_channel() {
        assert_eq!(target_channel(None, None, None), None);
    }

    // ---- apply ----

    #[tokio::test]
    async fn the_first_update_sends_and_tracks_the_message() {
        let fake = Fake::default();
        let mut slot = StatusSlot::default();

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
        assert_eq!(slot.message, shown);
    }

    #[tokio::test]
    async fn a_quiet_channel_is_edited_in_place() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100)]);
        assert_eq!(shown, Some(tracked(5, 100, Phase::Playing)));
    }

    #[tokio::test]
    async fn chat_since_the_status_moves_it_to_the_bottom() {
        let fake = Fake::default().with_last(101);
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_status_deleted_by_hand_is_sent_again() {
        let fake = Fake::default().with_last(100);
        *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_failed_delete_still_sends_the_new_status() {
        let fake = Fake::default().with_last(101);
        *fake.delete_error.lock().unwrap() = Some(TransportError::Other("Missing Permissions".into()));
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(5)]);
        assert_eq!(shown, Some(tracked(5, 1000, Phase::Playing)));
    }

    #[tokio::test]
    async fn a_failed_send_forgets_the_message() {
        let fake = Fake::default();
        *fake.send_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        let mut slot = StatusSlot::default();

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(shown, None);
        assert_eq!(slot.message, None);
    }

    #[tokio::test]
    async fn a_failed_edit_forgets_the_message() {
        let fake = Fake::default().with_last(100);
        *fake.edit_error.lock().unwrap() = Some(TransportError::Other("Missing Permissions".into()));
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Edit(5, 100)]);
        assert_eq!(shown, None);
        assert_eq!(slot.message, None);
    }

    #[tokio::test]
    async fn finished_stays_tracked_and_playing_continues_it() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let finished = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Finished).await;
        let playing = apply(&fake, &mut slot, GUILD, ch(5), embed(), Phase::Playing).await;

        assert_eq!(finished, Some(tracked(5, 100, Phase::Finished)));
        assert_eq!(playing, Some(tracked(5, 100, Phase::Playing)));
        assert_eq!(fake.ops(), vec![Op::Edit(5, 100), Op::Edit(5, 100)]);
    }

    #[tokio::test]
    async fn moving_to_another_channel_deletes_the_old_status() {
        let fake = Fake::default().with_last(100);
        let mut slot = StatusSlot { message: Some(tracked(5, 100, Phase::Playing)), ..Default::default() };

        let shown = apply(&fake, &mut slot, GUILD, ch(6), embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Delete(5, 100), Op::Send(6)]);
        assert_eq!(shown, Some(tracked(6, 1000, Phase::Playing)));
    }

    // ---- per-guild slot ----

    #[tokio::test]
    async fn every_update_for_a_guild_shares_one_slot() {
        let data = crate::Data::default();

        data.status_slot(GUILD).lock().await.last_command_channel = Some(ch(5));

        assert_eq!(data.status_slot(GUILD).lock().await.last_command_channel, Some(ch(5)));
        assert_eq!(data.status_slot(GuildId::new(2)).lock().await.last_command_channel, None);
    }
}
```

Add `pub mod status;` to `crack-core/src/messaging/mod.rs`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib messaging::status 2>&1 | head -30`
Expected: compile errors naming the missing items (`placement`, `StatusSlot`, `StatusTransport`, ...).

- [ ] **Step 4: Implement**

Put this above the test module in `status.rs`:

```rust
use crate::http_utils::is_unknown_message;
use serenity::all::{CreateEmbed, GenericChannelId, GuildId, MessageId};
use serenity::async_trait;
use std::sync::Arc;

/// Whether the status says something is playing or that playback finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Finished,
}

/// The status message on screen for a guild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusMessage {
    pub channel: GenericChannelId,
    pub id: MessageId,
    pub phase: Phase,
}

/// Everything the status message needs to remember per guild.
#[derive(Debug, Default)]
pub struct StatusSlot {
    /// What is on screen now, if anything.
    pub message: Option<StatusMessage>,
    /// Where the guild's most recent music command was run.
    pub last_command_channel: Option<GenericChannelId>,
}

/// How to bring the status up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Still the newest message in its channel: change it in place.
    Edit,
    /// Something was posted since, or it is in another channel: delete it and
    /// send a new one.
    Replace,
    /// Nothing is tracked: send one.
    Send,
}

/// 🔑 Discord ids grow with time, so the status is still at the bottom exactly
/// when the channel's last message is not newer than it. A cached id *older*
/// than ours is our own send not yet echoed back through the gateway, which is
/// still "nothing posted since". An unknown last message (channel not cached)
/// moves rather than guesses.
pub fn placement(
    current: Option<&StatusMessage>,
    target: GenericChannelId,
    channel_last: Option<MessageId>,
) -> Placement {
    let Some(current) = current else {
        return Placement::Send;
    };
    if current.channel != target {
        return Placement::Replace;
    }
    match channel_last {
        Some(last) if last <= current.id => Placement::Edit,
        _ => Placement::Replace,
    }
}

/// The guild's music channel, else the channel of its last music command,
/// else wherever the status already is. None means post nothing.
pub fn target_channel(
    music: Option<GenericChannelId>,
    last_command: Option<GenericChannelId>,
    tracked: Option<GenericChannelId>,
) -> Option<GenericChannelId> {
    music.or(last_command).or(tracked)
}

/// Why Discord refused a status request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The message is gone -- deleted by hand or by `/clean`.
    UnknownMessage,
    /// Anything else, as text for the log.
    Other(String),
}

impl From<serenity::Error> for TransportError {
    fn from(err: serenity::Error) -> Self {
        if is_unknown_message(&err) {
            Self::UnknownMessage
        } else {
            Self::Other(err.to_string())
        }
    }
}

/// The Discord calls the status makes, behind a seam so every branch of
/// [`apply`] is testable without Discord.
#[async_trait]
pub trait StatusTransport: Send + Sync {
    async fn send(
        &self,
        channel: GenericChannelId,
        embed: CreateEmbed<'static>,
    ) -> Result<MessageId, TransportError>;
    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        embed: CreateEmbed<'static>,
    ) -> Result<(), TransportError>;
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>;
    /// The newest message id the gateway has reported for `channel`, if the
    /// channel is cached.
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId>;
}

/// Bring the status in `slot` up to date in `target`, and return what is on
/// screen afterwards.
pub async fn apply(
    transport: &dyn StatusTransport,
    slot: &mut StatusSlot,
    guild: GuildId,
    target: GenericChannelId,
    embed: CreateEmbed<'static>,
    phase: Phase,
) -> Option<StatusMessage> {
    let current = slot.message;
    let last = transport.last_message_id(guild, target);
    match (placement(current.as_ref(), target, last), current) {
        (Placement::Edit, Some(current)) => {
            match transport.edit(current.channel, current.id, embed.clone()).await {
                Ok(()) => {
                    let shown = StatusMessage { phase, ..current };
                    slot.message = Some(shown);
                    return Some(shown);
                },
                // Deleted by hand or by `/clean`: a fresh one is sent below.
                Err(TransportError::UnknownMessage) => {},
                Err(TransportError::Other(err)) => {
                    tracing::warn!(
                        "status: could not edit {} in {}: {err}",
                        current.id,
                        current.channel
                    );
                    slot.message = None;
                    return None;
                },
            }
        },
        (Placement::Replace, Some(current)) => {
            match transport.delete(current.channel, current.id).await {
                Ok(()) | Err(TransportError::UnknownMessage) => {},
                Err(TransportError::Other(err)) => tracing::warn!(
                    "status: could not delete {} in {}: {err}",
                    current.id,
                    current.channel
                ),
            }
        },
        _ => {},
    }
    match transport.send(target, embed).await {
        Ok(id) => {
            let shown = StatusMessage { channel: target, id, phase };
            slot.message = Some(shown);
            Some(shown)
        },
        Err(err) => {
            tracing::warn!("status: could not send to {target}: {err:?}");
            slot.message = None;
            None
        },
    }
}

impl crate::Data {
    /// The guild's status slot, created on first use.
    ///
    /// 🪤 The `.clone()` matters: a dashmap reference held across the caller's
    /// `.lock().await` deadlocks the shard (see `lease.rs::lock_queue`).
    pub fn status_slot(&self, guild: GuildId) -> Arc<tokio::sync::Mutex<StatusSlot>> {
        self.status_slots
            .entry(guild)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(StatusSlot::default())))
            .clone()
    }
}
```

In `crack-core/src/lib.rs`, in `pub struct DataInner`, directly after the `queue_locks` field:

```rust
    /// Per-guild floating status message; see `messaging::status`.
    pub status_slots:
        dashmap::DashMap<serenity::GuildId, Arc<tokio::sync::Mutex<crate::messaging::status::StatusSlot>>>,
```

and in `impl Default for DataInner`, directly after `queue_locks: Default::default(),`:

```rust
            status_slots: Default::default(),
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib messaging::status`
Expected: 20 passed.

- [ ] **Step 6: Sabotage**

For each mutation: record `sha256sum crack-core/src/messaging/status.rs`, apply it, run `SQLX_OFFLINE=true cargo test -p crack-core --lib messaging::status`, confirm the named test fails, restore the file and confirm the checksum matches.

| mutation in `status.rs` | must fail |
|---|---|
| `Some(last) if last <= current.id` → `Some(last) if last == current.id` | `our_own_send_not_yet_echoed_still_edits` |
| `if current.channel != target {` → `if false {` | `a_status_in_another_channel_is_moved`, `moving_to_another_channel_deletes_the_old_status` |
| `_ => Placement::Replace,` → `_ => Placement::Edit,` | `a_message_posted_since_moves_the_status`, `an_uncached_channel_moves_rather_than_guesses` |
| `music.or(last_command).or(tracked)` → `last_command.or(music).or(tracked)` | `the_music_channel_wins` |
| `Err(TransportError::UnknownMessage) => {},` (edit arm) → `Err(TransportError::UnknownMessage) => return None,` | `a_status_deleted_by_hand_is_sent_again` |
| in the edit `Other` arm delete `slot.message = None;` | `a_failed_edit_forgets_the_message` |
| in the send `Err` arm delete `slot.message = None;` | `a_failed_send_forgets_the_message` |
| `StatusMessage { phase, ..current }` → `current` | `finished_stays_tracked_and_playing_continues_it` |

- [ ] **Step 7: Gate and commit**

Run the three gate commands (Global Constraints). Then:

```bash
git add crack-core/src/messaging/status.rs crack-core/src/messaging/mod.rs crack-core/src/http_utils.rs crack-core/src/commands/utility/clean.rs crack-core/src/lib.rs
git commit -F - <<'EOF'
status: decide where the floating status message goes, and move it

The pure core of the floating status message. `placement` compares the
status message's id with the channel's last message id: not newer means
edit in place, newer or unknown means delete and resend. `apply` carries
that out through a `StatusTransport` seam, so an already-deleted message,
a failed delete, a failed edit and a failed send are each tested against
a stand-in Discord. `is_unknown_message` moves to `http_utils`, shared
with `/clean`.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 2: Status entry points, reply rules, and the command-channel hook

**Files:**
- Modify: `crack-core/src/messaging/status.rs`
- Modify: `crack-core/src/messaging/messages.rs` (constants after `QUEUE_NOW_PLAYING`, line 149)
- Modify: `crack-core/src/commands/permissions.rs` (`cmd_check_music`)

**Interfaces:**
- Consumes (Task 1): `Phase`, `StatusMessage`, `StatusSlot`, `StatusTransport`, `TransportError`, `apply`, `target_channel`, `Data::status_slot`.
- Produces (in `crate::messaging::status`):
  - `pub struct DiscordTransport { pub http: Arc<Http>, pub cache: Arc<Cache> }` implementing `StatusTransport`
  - `pub async fn note_command_channel(data: &Data, guild: GuildId, channel: GenericChannelId)`
  - `pub async fn update(data: &Data, transport: &dyn StatusTransport, guild: GuildId, embed: CreateEmbed<'static>, phase: Phase) -> Option<StatusMessage>`
  - `pub async fn show_now_playing(data: &Data, http: Arc<Http>, cache: Arc<Cache>, guild: GuildId, call: &Arc<Mutex<Call>>) -> Option<StatusMessage>`
  - `pub async fn show_finished(data: &Data, http: Arc<Http>, cache: Arc<Cache>, guild: GuildId) -> Option<StatusMessage>`
  - `pub fn finished_embed() -> CreateEmbed<'static>`
  - `pub fn now_playing_pointer(title: &str, link: Option<MessageLink>) -> String`
  - `pub fn reply_privately(setting: bool, is_prefix: bool) -> bool`
  - `pub fn pointer_goes_first(private: bool, music: Option<GenericChannelId>, command_channel: GenericChannelId) -> bool`
  - `crate::messaging::messages::{NOW_PLAYING_POINTER, STATUS_FINISHED_TITLE, STATUS_FINISHED_DESCRIPTION}`

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `status.rs`:

```rust
    // ---- update: channel resolution under the slot lock ----

    #[tokio::test]
    async fn with_nowhere_to_post_nothing_is_posted() {
        let data = crate::Data::default();
        let fake = Fake::default();

        assert_eq!(update(&data, &fake, GUILD, embed(), Phase::Playing).await, None);
        assert!(fake.ops().is_empty());
    }

    #[tokio::test]
    async fn the_music_channel_beats_the_last_command_channel() {
        let data = crate::Data::default();
        let mut settings = crate::guild::settings::GuildSettings::new(GUILD, None, None);
        settings.set_music_channel(7);
        data.guild_settings_map.write().await.insert(GUILD, settings);
        note_command_channel(&data, GUILD, ch(5)).await;
        let fake = Fake::default();

        update(&data, &fake, GUILD, embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Send(7)]);
    }

    #[tokio::test]
    async fn without_a_music_channel_the_last_command_channel_is_used() {
        let data = crate::Data::default();
        note_command_channel(&data, GUILD, ch(5)).await;
        let fake = Fake::default();

        update(&data, &fake, GUILD, embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Send(5)]);
    }

    #[tokio::test]
    async fn a_command_in_another_channel_moves_the_status_there() {
        let data = crate::Data::default();
        note_command_channel(&data, GUILD, ch(5)).await;
        let fake = Fake::default().with_last(1000);
        update(&data, &fake, GUILD, embed(), Phase::Playing).await;

        note_command_channel(&data, GUILD, ch(6)).await;
        let shown = update(&data, &fake, GUILD, embed(), Phase::Playing).await;

        assert_eq!(fake.ops(), vec![Op::Send(5), Op::Delete(5, 1000), Op::Send(6)]);
        assert_eq!(shown, Some(tracked(6, 1001, Phase::Playing)));
    }

    // ---- reply rules ----

    #[test]
    fn replies_are_private_only_for_slash_commands_with_the_setting_on() {
        assert!(reply_privately(true, false));
        assert!(!reply_privately(true, true));
        assert!(!reply_privately(false, false));
        assert!(!reply_privately(false, true));
    }

    /// A visible `/nowplaying` reply goes first only when the status will land
    /// directly below it -- in the command's own channel. Otherwise the status
    /// is updated first so the reply can link to it.
    #[test]
    fn a_visible_pointer_goes_first_only_when_the_status_lands_below_it() {
        assert!(pointer_goes_first(false, None, ch(5)));
        assert!(pointer_goes_first(false, Some(ch(5)), ch(5)));
        assert!(!pointer_goes_first(false, Some(ch(7)), ch(5)));
        assert!(!pointer_goes_first(true, None, ch(5)));
    }

    #[test]
    fn the_pointer_links_to_the_status_when_it_can() {
        let link = MessageId::new(100).link(ch(5), Some(GUILD));

        assert_eq!(
            now_playing_pointer("Hit That", Some(link)),
            "🔊 Now playing: **Hit That** — https://discord.com/channels/1/5/100"
        );
        assert_eq!(now_playing_pointer("Hit That", None), "🔊 Now playing: **Hit That** ↓");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib messaging::status 2>&1 | head -30`
Expected: compile errors naming `update`, `note_command_channel`, `reply_privately`, `pointer_goes_first`, `now_playing_pointer`.

- [ ] **Step 3: Implement**

In `crack-core/src/messaging/messages.rs`, directly after `pub const QUEUE_NOW_PLAYING: &str = "🔊 Now playing";`:

```rust
pub const NOW_PLAYING_POINTER: &str = "🔊 Now playing:";
pub const STATUS_FINISHED_TITLE: &str = "⏹️ Finished";
pub const STATUS_FINISHED_DESCRIPTION: &str = "Nothing is playing. Use /play to start again.";
```

In `status.rs`, extend the imports:

```rust
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::interface::create_now_playing_embed;
use crate::messaging::messages::{
    NOW_PLAYING_POINTER, STATUS_FINISHED_DESCRIPTION, STATUS_FINISHED_TITLE,
};
use crate::Data;
use serenity::all::{Cache, Http, MessageLink};
use serenity::builder::{CreateMessage, EditMessage};
use songbird::Call;
use tokio::sync::Mutex;
```

and add after `impl crate::Data { ... }`:

```rust
/// The real Discord behind [`StatusTransport`].
pub struct DiscordTransport {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
}

#[async_trait]
impl StatusTransport for DiscordTransport {
    async fn send(
        &self,
        channel: GenericChannelId,
        embed: CreateEmbed<'static>,
    ) -> Result<MessageId, TransportError> {
        Ok(channel
            .send_message(&self.http, CreateMessage::new().embed(embed))
            .await?
            .id)
    }

    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        embed: CreateEmbed<'static>,
    ) -> Result<(), TransportError> {
        channel
            .edit_message(&self.http, id, EditMessage::new().embed(embed))
            .await?;
        Ok(())
    }

    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError> {
        Ok(channel.delete_message(&self.http, id, None).await?)
    }

    /// serenity sets `last_message_id` on every message-create for guild
    /// channels and threads (`cache/event.rs`). None when the guild or channel
    /// is not cached, which `placement` treats as "moved".
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId> {
        let guild = self.cache.guild(guild)?;
        let (channel_id, thread_id) = channel.split();
        match guild.channels.get(&channel_id) {
            Some(guild_channel) => guild_channel.base.last_message_id,
            None => guild
                .threads
                .get(&thread_id)
                .and_then(|thread| thread.base.last_message_id),
        }
    }
}

/// Remember where the guild's latest music command was run.
pub async fn note_command_channel(data: &Data, guild: GuildId, channel: GenericChannelId) {
    data.status_slot(guild).lock().await.last_command_channel = Some(channel);
}

/// Resolve the target channel and apply the update, under the guild's slot
/// lock so a `/skip` cannot race a track end.
pub async fn update(
    data: &Data,
    transport: &dyn StatusTransport,
    guild: GuildId,
    embed: CreateEmbed<'static>,
    phase: Phase,
) -> Option<StatusMessage> {
    let music = data.get_music_channel(guild).await;
    let slot = data.status_slot(guild);
    let mut slot = slot.lock().await;
    let tracked = slot.message.map(|status| status.channel);
    let target = target_channel(music, slot.last_command_channel, tracked)?;
    apply(transport, &mut slot, guild, target, embed, phase).await
}

/// Show what is playing now.
///
/// 🔑 Lock order: the Call lock is taken and released here, before the slot
/// lock in [`update`]. Callers must not hold a Call lock themselves.
pub async fn show_now_playing(
    data: &Data,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild: GuildId,
    call: &Arc<Mutex<Call>>,
) -> Option<StatusMessage> {
    // A `/gp` round is guessing the song: the status would give it away.
    if data.gp_is_active(guild) {
        return None;
    }
    let track = call.lock().await.queue().current()?;
    let embed: CreateEmbed<'static> = create_now_playing_embed(track).await;
    update(data, &DiscordTransport { http, cache }, guild, embed, Phase::Playing).await
}

/// Show that playback finished. The message stays tracked, so the next
/// now-playing moment continues it.
pub async fn show_finished(
    data: &Data,
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild: GuildId,
) -> Option<StatusMessage> {
    if data.gp_is_active(guild) {
        return None;
    }
    update(data, &DiscordTransport { http, cache }, guild, finished_embed(), Phase::Finished).await
}

/// The "Finished" status.
pub fn finished_embed() -> CreateEmbed<'static> {
    CreateEmbed::new()
        .title(STATUS_FINISHED_TITLE)
        .description(STATUS_FINISHED_DESCRIPTION)
}

/// `/nowplaying`'s one-line reply: a jump link when the status exists
/// elsewhere, or an arrow when it is about to land directly below.
pub fn now_playing_pointer(title: &str, link: Option<MessageLink>) -> String {
    match link {
        Some(link) => format!("{NOW_PLAYING_POINTER} **{title}** — {link}"),
        None => format!("{NOW_PLAYING_POINTER} **{title}** ↓"),
    }
}

/// Whether a status-related command replies ephemerally: the guild's
/// `ephemeral_replies` setting, and only for slash commands -- a prefix
/// command's reply cannot be ephemeral.
pub fn reply_privately(setting: bool, is_prefix: bool) -> bool {
    setting && !is_prefix
}

/// Whether `/nowplaying` replies before updating the status. Only a visible
/// reply in the channel the status will land in goes first; everywhere else
/// the status is updated first so the reply can link to it.
pub fn pointer_goes_first(
    private: bool,
    music: Option<GenericChannelId>,
    command_channel: GenericChannelId,
) -> bool {
    !private && music.is_none_or(|music| music == command_channel)
}
```

In `crack-core/src/commands/permissions.rs`, replace the whole `cmd_check_music` function with (everything above `let allowed` is the existing body, unchanged):

```rust
/// Public function to check if the user is authorized to use the music commands.
pub async fn cmd_check_music(ctx: Context<'_>) -> Result<bool, Error> {
    if ctx.author().bot() {
        return Ok(false);
    };

    // While a guilty pleasure game owns playback, queue-mutating commands would
    // corrupt the round order. Matched on the qualified name so the game's own
    // `gp skip` is not caught by the top-level `skip`.
    if let Some(guild_id) = ctx.guild_id() {
        if ctx.data().gp_is_active(guild_id)
            && GP_BLOCKED_COMMANDS.contains(&&*ctx.command().qualified_name)
        {
            return Err(CrackedError::GameInProgress.into());
        }
    }

    let channel_id: GenericChannelId = ctx.channel_id();
    let member = ctx.author_member().await;

    let allowed = cmd_check_music_internal(member, channel_id, ctx).await?;
    // The floating status message follows the conversation: remember where
    // this guild's latest music command was run.
    if allowed {
        if let Some(guild_id) = ctx.guild_id() {
            crate::messaging::status::note_command_channel(&ctx.data(), guild_id, channel_id)
                .await;
        }
    }
    Ok(allowed)
}
```

Diff the result against `git show HEAD:crack-core/src/commands/permissions.rs` before committing: the only change must be the `let allowed ... Ok(allowed)` tail replacing the final `cmd_check_music_internal(member, channel_id, ctx).await` expression.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib messaging::status`
Expected: 27 passed. If `the_pointer_links_to_the_status_when_it_can` fails only on the URL shape, print `MessageId::new(100).link(ch(5), Some(GUILD))` in the test, confirm serenity's `MessageLink` Display format, and correct the expected string (not the code).

- [ ] **Step 5: Sabotage**

| mutation | must fail |
|---|---|
| in `update`, `target_channel(music, slot.last_command_channel, tracked)` → `target_channel(None, slot.last_command_channel, tracked)` | `the_music_channel_beats_the_last_command_channel` |
| in `note_command_channel`, `= Some(channel)` → `= None` | `without_a_music_channel_the_last_command_channel_is_used` |
| `setting && !is_prefix` → `setting` | `replies_are_private_only_for_slash_commands_with_the_setting_on` |
| `!private && music.is_none_or(...)` → `music.is_none_or(...)` | `a_visible_pointer_goes_first_only_when_the_status_lands_below_it` |
| `**{title}** ↓` → `**{title}**` | `the_pointer_links_to_the_status_when_it_can` |

Restore each by checksum.

- [ ] **Step 6: Gate and commit**

```bash
git add crack-core/src/messaging/status.rs crack-core/src/messaging/messages.rs crack-core/src/commands/permissions.rs
git commit -F - <<'EOF'
status: show now playing or finished, in the right channel

`show_now_playing` and `show_finished` are the status message's entry
points: they resolve the guild's music channel, else the channel of its
last music command, and apply the update under the guild's slot lock.
`cmd_check_music` records that command channel. `reply_privately` and
`pointer_goes_first` are the reply rules the commands will use; the
`/gp` guard keeps the status from naming a song being guessed.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 3: Track end and every way playback ends

**Files:**
- Modify: `crack-core/src/handlers/track_end.rs`
- Modify: `crack-core/src/commands/music/stop.rs`
- Modify: `crack-core/src/commands/music/leave.rs`
- Modify: `crack-core/src/handlers/idle.rs`
- Modify: `crack-core/src/handlers/serenity.rs` (`on_voice_state_update`)

**Interfaces:**
- Consumes (Task 2): `status::show_now_playing`, `status::show_finished`.
- Produces: `pub enum TrackEndStatus { NowPlaying, Finished, Autoplay, Nothing }` and `pub fn track_end_status(gp_active: bool, next_exists: bool, autoplay: bool) -> TrackEndStatus` in `crate::handlers::track_end`.

- [ ] **Step 1: Write the failing test**

Find the test module at the bottom of `crack-core/src/handlers/track_end.rs` (`grep -n 'cfg(test)' crack-core/src/handlers/track_end.rs`) and append inside it:

```rust
    /// Every track end decides what the status shows. Until v0.13.0 anything
    /// but autoplay returned before "Now playing" was ever posted.
    #[test]
    fn a_track_ending_decides_what_the_status_shows() {
        use super::{track_end_status, TrackEndStatus::*};

        assert_eq!(track_end_status(true, true, true), Nothing);
        assert_eq!(track_end_status(true, false, false), Nothing);
        assert_eq!(track_end_status(false, true, false), NowPlaying);
        assert_eq!(track_end_status(false, true, true), NowPlaying);
        assert_eq!(track_end_status(false, false, true), Autoplay);
        assert_eq!(track_end_status(false, false, false), Finished);
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib a_track_ending_decides 2>&1 | head -20`
Expected: compile error, `track_end_status` / `TrackEndStatus` not found.

- [ ] **Step 3: Implement the decision**

In `track_end.rs`, directly after `fn get_track_states_union(...) { ... }`:

```rust
/// What the status message shows after a track ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackEndStatus {
    /// Another track is up: show it.
    NowPlaying,
    /// Nothing is next and autoplay is off.
    Finished,
    /// Nothing is next and autoplay is on: ask for a recommendation.
    Autoplay,
    /// A `/gp` game owns playback.
    Nothing,
}

/// Decide [`TrackEndStatus`] from the three facts that matter.
pub fn track_end_status(gp_active: bool, next_exists: bool, autoplay: bool) -> TrackEndStatus {
    if gp_active {
        TrackEndStatus::Nothing
    } else if next_exists {
        TrackEndStatus::NowPlaying
    } else if autoplay {
        TrackEndStatus::Autoplay
    } else {
        TrackEndStatus::Finished
    }
}
```

- [ ] **Step 4: Rewire `TrackEndHandler::act`**

In `act`, replace everything from `let music_channel = self.data.get_music_channel(self.guild_id).await;` down to (and including) the final `None` before the closing braces of `act` with:

```rust
        let music_channel = self.data.get_music_channel(self.guild_id).await;
        let mut autoplay = autoplay;

        if autoplay {
            if let EventContext::Track(x) = event_ctx {
                // `debug!` rather than `trace!`: this one is genuinely useful when
                // debugging playback, being the whole track-state slice. It is
                // still not an error.
                tracing::debug!("TrackEvent: {:?}", x);
                if get_track_states_union(x).errored {
                    self.data.set_autoplay(self.guild_id, false).await;
                    tracing::warn!("autoplay disabled for {}: track errored", self.guild_id);
                    // Speaks up only when a music channel is configured.
                    if let Some(c) = music_channel {
                        send_plain(c, self.http.clone(), AUTOPLAY_STOPPED).await;
                    }
                    // Carry on without it: the status still says what plays
                    // next, or that playback finished.
                    autoplay = false;
                }
            }
        }

        let next_exists = self.call.lock().await.queue().current().is_some();
        match track_end_status(self.data.gp_is_active(self.guild_id), next_exists, autoplay) {
            TrackEndStatus::Nothing => return None,
            TrackEndStatus::NowPlaying => {
                self.show_now_playing().await;
                return None;
            },
            TrackEndStatus::Finished => {
                self.show_finished().await;
                return None;
            },
            TrackEndStatus::Autoplay => {},
        }

        // The track that just ended seeds the next recommendation. No database:
        // neither YouTube's Mix nor Deezer needs one, so autoplay works without one.
        let ended: Option<TrackHandle> = match event_ctx {
            EventContext::Track(tracks) => tracks.first().map(|(_, handle)| (*handle).clone()),
            _ => None,
        };

        // Where "autoplay off" is announced: the music channel, else the voice
        // channel's chat. The status message resolves its own channel.
        let fallback = self
            .call
            .lock()
            .await
            .current_channel()
            .map(|c| GenericChannelId::new(c.get()));
        let Some(channel) = music_channel.or(fallback) else {
            // Not connected any more: nowhere to announce, nothing to play into.
            return None;
        };

        // 🔴 This replaces a Spotify path that could never run: it needed client
        // credentials production does not have, and Spotify stopped issuing new
        // Web API apps around 2025-12.
        let Some(next) = self.next_autoplay_track(ended).await else {
            // Turning a feature the user switched ON back OFF is not something
            // to do silently: from the channel's point of view the music would
            // simply stop.
            self.data.set_autoplay(self.guild_id, false).await;
            tracing::warn!("autoplay disabled for {}: no recommendation", self.guild_id);
            announce_autoplay_off(channel, self.http.clone(), self.data.musicreco.is_some()).await;
            self.show_finished().await;
            return None;
        };
        tracing::debug!(
            "autoplay in {}: `{} - {}` from {}",
            self.guild_id,
            next.artist,
            next.title,
            next.source
        );
        let query = autoplay::to_query(&next);

        match queue_query(&self.data, self.guild_id, query, self.call.clone()).await {
            Ok(_) => {
                self.show_now_playing().await;
            },
            Err(e) => {
                self.data.set_autoplay(self.guild_id, false).await;
                tracing::warn!("autoplay disabled for {}: {}", self.guild_id, e);
                announce_autoplay_off(channel, self.http.clone(), self.data.musicreco.is_some())
                    .await;
                self.show_finished().await;
            },
        }
        None
```

Add to `impl TrackEndHandler` (next to `next_autoplay_track`):

```rust
    /// The status says what is playing now.
    async fn show_now_playing(&self) {
        crate::messaging::status::show_now_playing(
            &self.data,
            self.http.clone(),
            self.cache.clone(),
            self.guild_id,
            &self.call,
        )
        .await;
    }

    /// The status says playback finished.
    async fn show_finished(&self) {
        crate::messaging::status::show_finished(
            &self.data,
            self.http.clone(),
            self.cache.clone(),
            self.guild_id,
        )
        .await;
    }
```

Remove `send_now_playing` from the `messaging::interface::{...}` import in `track_end.rs` (it is still used by `commands/music/grab.rs`).

- [ ] **Step 5: The other ways playback ends**

`crack-core/src/commands/music/stop.rs` — in `stop_internal`, directly after `send_reply(&ctx, CrackedMessage::Stop, true).await?;`:

```rust
    // Idempotent with the track end `stop_queue` fires: both land on Finished.
    let serenity_ctx = ctx.serenity_context();
    crate::messaging::status::show_finished(
        &ctx.data(),
        serenity_ctx.http.clone(),
        serenity_ctx.cache.clone(),
        guild_id,
    )
    .await;
```

`crack-core/src/commands/music/leave.rs` — in `leave_internal`, change the match so the success case is remembered, and show Finished after the reply:

```rust
    let mut left = false;
    let crack_msg = match manager.remove(guild_id).await {
        Ok(()) => {
            tracing::info!("Driver successfully removed.");
            left = true;
            CrackedMessage::Leaving
        },
        Err(err) => {
            tracing::error!("Driver could not be removed: {}", err);
            match err {
                JoinError::NoCall => CrackedMessage::CrackedError(CrackedError::NotConnected),
                _ => return Err(err.into()),
            }
        },
    };

    let _ = send_reply(&ctx, crack_msg, true).await?;
    if left {
        let serenity_ctx = ctx.serenity_context();
        crate::messaging::status::show_finished(
            &ctx.data(),
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
        )
        .await;
    }
    Ok(())
```

`crack-core/src/handlers/idle.rs` — in the `Ok(_) =>` arm of `match manager.remove(self.guild_id).await`, before the idle alert is sent:

```rust
                Ok(_) => {
                    crate::messaging::status::show_finished(
                        &data,
                        self.serenity_ctx.http.clone(),
                        self.serenity_ctx.cache.clone(),
                        self.guild_id,
                    )
                    .await;
                    match self
                        .channel_id
                        .say(&self.serenity_ctx.http, IDLE_ALERT)
```

(the rest of that arm unchanged).

`crack-core/src/handlers/serenity.rs` — in `on_voice_state_update`, directly after the `if let Err(e) = manager.remove(guild_id).await { ... }` block and **before** `if self.data.gp_remove(guild_id).is_some()` (so a game still owns playback when the guard checks):

```rust
        // Kicked or disconnected: the status says playback finished.
        crate::messaging::status::show_finished(
            &self.data,
            ctx.http.clone(),
            ctx.cache.clone(),
            guild_id,
        )
        .await;
```

- [ ] **Step 6: Run the test and the crate**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib a_track_ending_decides`
Expected: PASS. Then `SQLX_OFFLINE=true cargo clippy -p crack-core --all-targets --locked -- -D warnings` clean.

- [ ] **Step 7: Sabotage**

| mutation in `track_end.rs` | must fail |
|---|---|
| `if gp_active {` → `if false {` | `a_track_ending_decides_what_the_status_shows` |
| `else if next_exists {` → `else if next_exists && autoplay {` | same |
| `TrackEndStatus::Autoplay` (in `track_end_status`) → `TrackEndStatus::Finished` | same |

Restore each by checksum. The handler wiring itself is exercised on TuneTitan (Task 7).

- [ ] **Step 8: Gate and commit**

```bash
git add crack-core/src/handlers/track_end.rs crack-core/src/commands/music/stop.rs crack-core/src/commands/music/leave.rs crack-core/src/handlers/idle.rs crack-core/src/handlers/serenity.rs
git commit -F - <<'EOF'
status: every track change updates it, and every ending finishes it

A track end now updates the status whether or not autoplay is on:
`TrackEndHandler::act` returned before its only "Now playing" post
unless autoplay was on, so a playlist changed songs in silence.
`track_end_status` makes that decision testable. `/stop`, `/leave`, the
idle disconnect and being kicked set the status to Finished.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 4: The `ephemeral_replies` guild setting

**Files:**
- Create: `migrations/20260915120000_ephemeral_replies.sql`
- Create: `crack-core/test_migrations/20260915120000_ephemeral_replies.sql` (identical)
- Modify: `crack-core/src/guild/settings.rs` (field, `PartialEq`, `new`, `From<GuildSettingsRead>`, `toggle_ephemeral_replies`, tests)
- Modify: `crack-core/src/db/guild.rs` (`GuildSettingsRead` field, `write_settings` upsert, db test)
- Modify: `crack-core/src/guild/operations.rs` (`get_ephemeral_replies` + test)
- Modify: `crack-core/src/messaging/messages.rs` (`EPHEMERAL_REPLIES_ON`, `EPHEMERAL_REPLIES_OFF`)
- Create: `crack-core/src/commands/settings/toggle/toggle_ephemeral.rs`
- Modify: `crack-core/src/commands/settings/toggle/mod.rs`
- Modify: `.sqlx/` (regenerated)

**Interfaces:**
- Produces: `GuildSettings::ephemeral_replies: bool`; `GuildSettings::toggle_ephemeral_replies(&mut self) -> &mut Self`; `GuildSettingsRead::ephemeral_replies: bool`; `GuildSettingsOperations::get_ephemeral_replies(&self, guild_id: GuildId) -> impl Future<Output = bool>`; command `toggle_ephemeral` (`/settings toggle ephemeral`).

- [ ] **Step 1: Write the failing unit tests**

In `crack-core/src/guild/settings.rs`, inside `mod test`:

```rust
    #[test]
    fn ephemeral_replies_are_off_by_default() {
        assert!(!GuildSettings::new(GuildId::new(123), None, None).ephemeral_replies);
    }

    #[test]
    fn toggling_ephemeral_replies_flips_them() {
        let mut settings = GuildSettings::new(GuildId::new(123), None, None);

        settings.toggle_ephemeral_replies();
        assert!(settings.ephemeral_replies);
        settings.toggle_ephemeral_replies();
        assert!(!settings.ephemeral_replies);
    }

    #[test]
    fn a_database_row_carries_ephemeral_replies() {
        let row = crate::db::GuildSettingsRead {
            guild_id: 123,
            guild_name: "guild".to_string(),
            prefix: "r!".to_string(),
            premium: false,
            autopause: false,
            allow_all_domains: true,
            allowed_domains: vec![],
            banned_domains: vec![],
            ignored_channels: vec![],
            old_volume: 1.0,
            volume: 1.0,
            self_deafen: true,
            timeout_seconds: Some(360),
            additional_prefixes: vec![],
            ephemeral_replies: true,
        };

        assert!(GuildSettings::from(row).ephemeral_replies);
    }

    #[test]
    fn settings_differing_only_in_ephemeral_replies_are_not_equal() {
        let visible = GuildSettings::new(GuildId::new(123), None, None);
        let mut private = visible.clone();
        private.ephemeral_replies = true;

        assert_ne!(visible, private);
    }
```

In `crack-core/src/guild/operations.rs`, inside its test module (next to `test_get_music_channel`):

```rust
    #[tokio::test]
    async fn ephemeral_replies_follow_the_guild_setting() {
        let data = crate::Data::default();
        let guild_id = GuildId::new(123);
        assert!(!data.get_ephemeral_replies(guild_id).await, "no settings means visible");

        let mut settings = GuildSettings::new(guild_id, None, None);
        settings.ephemeral_replies = true;
        data.guild_settings_map.write().await.insert(guild_id, settings);

        assert!(data.get_ephemeral_replies(guild_id).await);
    }
```

(Match that module's existing imports; add `use crate::guild::settings::GuildSettings;` / `GuildId` there if not already in scope.)

- [ ] **Step 2: Run them to verify they fail**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib ephemeral_replies 2>&1 | head -30`
Expected: compile errors, no field/method `ephemeral_replies` / `toggle_ephemeral_replies` / `get_ephemeral_replies`.

- [ ] **Step 3: Migration, struct fields, conversion, getter**

`migrations/20260915120000_ephemeral_replies.sql` and `crack-core/test_migrations/20260915120000_ephemeral_replies.sql`, identical:

```sql
-- Whether /play, /skip and /nowplaying reply ephemerally in this guild. Off
-- keeps visible replies, the behaviour before v0.13.0. See
-- docs/superpowers/specs/2026-09-15-floating-status-message-design.md.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS ephemeral_replies BOOLEAN NOT NULL DEFAULT FALSE;
```

`crack-core/src/db/guild.rs`, `GuildSettingsRead`: add as the last field, after `pub additional_prefixes: Vec<String>,`:

```rust
    pub ephemeral_replies: bool,
```

`crack-core/src/guild/settings.rs`:
- In `pub struct GuildSettings`, after `pub reply_with_embed: bool,`:
  ```rust
      /// Whether /play, /skip and /nowplaying reply ephemerally.
      #[serde(default = "default_false")]
      pub ephemeral_replies: bool,
  ```
- In `impl PartialEq for GuildSettings`, after `&& self.reply_with_embed == other.reply_with_embed`:
  ```rust
              && self.ephemeral_replies == other.ephemeral_replies
  ```
- In `GuildSettings::new`, after `reply_with_embed: true,`:
  ```rust
              ephemeral_replies: false,
  ```
- In `impl From<GuildSettingsRead> for GuildSettings`, after `settings.autopause = settings_db.autopause;`:
  ```rust
          settings.ephemeral_replies = settings_db.ephemeral_replies;
  ```
- After `pub fn toggle_autopause(&mut self) -> &mut Self { ... }`:
  ```rust
      /// Toggle private (ephemeral) replies for the status-related commands.
      pub fn toggle_ephemeral_replies(&mut self) -> &mut Self {
          self.ephemeral_replies = !self.ephemeral_replies;
          self
      }
  ```

`crack-core/src/guild/operations.rs`:
- In `trait GuildSettingsOperations`, after the `set_reply_with_embed` declaration:
  ```rust
      fn get_ephemeral_replies(&self, guild_id: GuildId) -> impl Future<Output = bool>;
  ```
- In `impl GuildSettingsOperations for Data`, after `set_reply_with_embed`:
  ```rust
      /// Whether /play, /skip and /nowplaying reply ephemerally in this guild.
      async fn get_ephemeral_replies(&self, guild_id: GuildId) -> bool {
          self.guild_settings_map
              .read()
              .await
              .get(&guild_id)
              .is_some_and(|settings| settings.ephemeral_replies)
      }
  ```

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib ephemeral_replies`
Expected: this still fails to compile **only** inside `db/guild.rs` (sqlx offline data no longer matches `GuildSettingsRead`). That is expected; Step 5 regenerates it.

- [ ] **Step 4: Write the upsert and the failing database round-trip test**

In `GuildEntity::write_settings`, replace the `sqlx::query!` for `guild_settings` with:

```rust
        sqlx::query!(
            r#"
            INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause, allow_all_domains, allowed_domains, banned_domains, ignored_channels, old_volume, volume, self_deafen, timeout_seconds, additional_prefixes, ephemeral_replies)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::FLOAT, $11::FLOAT, $12, $13, $14, $15)
            ON CONFLICT (guild_id)
            DO UPDATE SET guild_name = $2, prefix = $3, premium = $4, autopause = $5, allow_all_domains = $6, allowed_domains = $7, banned_domains = $8, ignored_channels = $9, old_volume = $10::FLOAT, volume = $11::FLOAT, self_deafen = $12, timeout_seconds = $13, additional_prefixes = $14, ephemeral_replies = $15
            "#,
            settings.guild_id.get() as i64,
            to_write,
            settings.prefix,
            settings.premium,
            settings.autopause,
            settings.allow_all_domains,
            &settings.allowed_domains.clone().into_iter().collect::<Vec<String>>(),
            &settings.banned_domains.clone().into_iter().collect::<Vec<String>>(),
            ignored_channels.as_slice(),
            settings.old_volume as i64,
            settings.volume as i64,
            settings.self_deafen,
            settings.timeout as i32,
            &settings.additional_prefixes,
            settings.ephemeral_replies,
        )
        .execute(pool)
        .await?;
```

Append to the bottom of `crack-core/src/db/guild.rs`:

```rust
#[cfg(test)]
mod ephemeral_replies_db_tests {
    use super::*;
    use std::str::FromStr;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    /// The setting only matters if it survives a restart.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn ephemeral_replies_survive_a_save_and_load(pool: PgPool) -> Result<(), SerenityError> {
        let name = FixedString::from_str("status test").expect("a short name");
        let (_guild, mut settings) =
            GuildEntity::get_or_create(&pool, 424242, name, "r!".to_string()).await?;
        assert!(!settings.ephemeral_replies);

        settings.ephemeral_replies = true;
        GuildEntity::write_settings(&pool, &settings).await?;

        let reloaded = GuildEntity::new_guild(424242, "status test".to_string())
            .get_settings(&pool)
            .await?;
        assert!(reloaded.ephemeral_replies);
        Ok(())
    }
}
```

- [ ] **Step 5: Regenerate `.sqlx` against a throwaway Postgres, and run the round trip red then green**

```bash
bash <<'EOF'
set -euo pipefail
cd /home/lothrop/projects/cracktunes
port=55432
if ss -ltn | grep -q ":$port "; then echo "port $port is busy; pick another and use it below"; exit 1; fi
docker image inspect postgres:16-alpine >/dev/null
docker run --rm -d --pull never --name cracktunes-sqlx-prepare \
  -e POSTGRES_PASSWORD=prepare -p 127.0.0.1:$port:5432 postgres:16-alpine
for i in $(seq 1 60); do
  docker exec cracktunes-sqlx-prepare pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
export DATABASE_URL="postgres://postgres:prepare@127.0.0.1:$port/postgres"
cargo sqlx migrate run --source migrations/
cargo sqlx prepare --workspace -- --tests --all
EOF
```

Then, with the same container still running:

1. **Red:** temporarily change `settings.ephemeral_replies,` (the `$15` argument) to `false,` in `write_settings`, run
   `DATABASE_URL=postgres://postgres:prepare@127.0.0.1:55432/postgres cargo test -p crack-core --lib --features db-tests ephemeral_replies_survive`
   and confirm it FAILS at `assert!(reloaded.ephemeral_replies)`. Restore by checksum.
2. **Green:** run the same command; it passes.
3. `SQLX_OFFLINE=true cargo check --workspace --all-targets` — clean.
4. `docker stop cracktunes-sqlx-prepare` (the `--rm` removes it). Confirm `docker ps -a --filter name=cracktunes-sqlx-prepare` shows nothing.

`git status --porcelain .sqlx` should show the changed `write_settings` query and the `GuildSettingsRead`-returning queries (`SELECT *` / `RETURNING *`) updated. No other `.sqlx` entry should change; if one does, investigate before committing.

- [ ] **Step 6: The toggle command**

`crack-core/src/messaging/messages.rs`, after `STATUS_FINISHED_DESCRIPTION`:

```rust
pub const EPHEMERAL_REPLIES_ON: &str = "🙈 /play, /skip and /nowplaying now reply privately.";
pub const EPHEMERAL_REPLIES_OFF: &str = "👀 /play, /skip and /nowplaying now reply in the channel.";
```

Create `crack-core/src/commands/settings/toggle/toggle_ephemeral.rs`:

```rust
use crate::http_utils::CacheHttpExt;
use crate::messaging::messages::{EPHEMERAL_REPLIES_OFF, EPHEMERAL_REPLIES_ON};
use crate::{errors::CrackedError, guild::settings::GuildSettings, Context, Data, Error};
use serenity::all::GuildId;
use serenity::small_fixed_array::FixedString;
use sqlx::PgPool;
use std::sync::Arc;

/// Toggle whether /play, /skip and /nowplaying reply privately.
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    rename = "ephemeral",
    required_permissions = "ADMINISTRATOR"
)]
#[cfg(not(tarpaulin_include))]
pub async fn toggle_ephemeral(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild_name = ctx.guild_name_from_guild_id(guild_id).await?;
    let res = toggle_ephemeral_internal(
        ctx.data(),
        ctx.data()
            .database_pool
            .clone()
            .ok_or(CrackedError::NoDatabasePool)?,
        guild_id,
        Some(guild_name),
        ctx.data().bot_settings.get_prefix(),
    )
    .await?;

    let reply = if res.ephemeral_replies {
        EPHEMERAL_REPLIES_ON
    } else {
        EPHEMERAL_REPLIES_OFF
    };
    ctx.say(reply).await?;
    Ok(())
}

/// Toggle ephemeral replies for a guild and save it.
#[cfg(not(tarpaulin_include))]
pub async fn toggle_ephemeral_internal(
    data: Arc<Data>,
    pool: PgPool,
    guild_id: GuildId,
    guild_name: Option<FixedString>,
    prefix: String,
) -> Result<GuildSettings, CrackedError> {
    // 🔑 Before mutating: make sure what is in memory came from Postgres. A
    // guild whose boot load failed holds defaults, and `save()` below is a
    // full-row upsert that would write them over its stored row.
    data.ensure_settings_loaded(guild_id).await?;

    let res = data
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.toggle_ephemeral_replies();
        })
        .or_insert_with(|| {
            GuildSettings::new(guild_id, Some(&prefix), guild_name)
                .toggle_ephemeral_replies()
                .clone()
        })
        .clone();
    res.save(&pool).await?;
    Ok(res)
}
```

`crack-core/src/commands/settings/toggle/mod.rs`: add `pub mod toggle_ephemeral;` and `pub use toggle_ephemeral::*;`; change `subcommands("selfdeafen", "toggle_autopause")` to `subcommands("selfdeafen", "toggle_autopause", "toggle_ephemeral")` and `vec![selfdeafen(), toggle_autopause()]` to `vec![selfdeafen(), toggle_autopause(), toggle_ephemeral()]`.

- [ ] **Step 7: Run the unit tests and sabotage**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib ephemeral_replies`
Expected: 5 passed (the db test is ignored without the feature).

| mutation | must fail |
|---|---|
| `settings.ephemeral_replies = settings_db.ephemeral_replies;` → delete the line | `a_database_row_carries_ephemeral_replies` |
| `&& self.ephemeral_replies == other.ephemeral_replies` → delete the line | `settings_differing_only_in_ephemeral_replies_are_not_equal` |
| `self.ephemeral_replies = !self.ephemeral_replies;` → `self.ephemeral_replies = true;` | `toggling_ephemeral_replies_flips_them` |
| `.is_some_and(|settings| settings.ephemeral_replies)` → `.is_some()` | `ephemeral_replies_follow_the_guild_setting` |
| `ephemeral_replies: false,` (in `new`) → `ephemeral_replies: true,` | `ephemeral_replies_are_off_by_default` |

(The upsert's `$15` was sabotaged in Step 5.) Restore each by checksum.

- [ ] **Step 8: Gate and commit**

```bash
git add migrations/20260915120000_ephemeral_replies.sql crack-core/test_migrations/20260915120000_ephemeral_replies.sql crack-core/src/guild/settings.rs crack-core/src/db/guild.rs crack-core/src/guild/operations.rs crack-core/src/messaging/messages.rs crack-core/src/commands/settings/toggle/toggle_ephemeral.rs crack-core/src/commands/settings/toggle/mod.rs .sqlx
git commit -F - <<'EOF'
settings: ephemeral_replies, saved per guild

A guild setting for whether /play, /skip and /nowplaying reply
ephemerally, off by default. It is a `guild_settings` column, carried
through the full-row upsert and every `GuildSettingsRead` load, and
`/settings toggle ephemeral` flips it. `.sqlx` is regenerated against a
throwaway Postgres; a database test proves the value survives a save and
load.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 5: Command replies — `/play`, `/skip`, `/nowplaying`

**Files:**
- Modify: `crack-core/src/utils.rs` (`send_embed_response_poise_as`)
- Modify: `crack-core/src/messaging/interface.rs` (`send_search_message_as`)
- Modify: `crack-core/src/commands/music/doplay.rs` (`play_internal`)
- Modify: `crack-core/src/commands/music/skip.rs` (`skip`, `create_skip_response`)
- Modify: `crack-core/src/commands/music/voteskip.rs` (call site)
- Modify: `crack-core/src/commands/music/nowplaying.rs`

**Interfaces:**
- Consumes: `status::{reply_privately, pointer_goes_first, now_playing_pointer, show_now_playing}` (Task 2); `GuildSettingsOperations::get_ephemeral_replies` (Task 4).
- Produces: `crate::utils::send_embed_response_poise_as(ctx: CrackContext<'ctx>, embed: CreateEmbed<'ctx>, ephemeral: bool) -> Result<ReplyHandle<'ctx>, CrackedError>`; `crate::messaging::interface::send_search_message_as(ctx: &'ctx CrackContext<'_>, ephemeral: bool) -> CrackedResult<ReplyHandle<'ctx>>`; `create_skip_response(ctx, handler, tracks_to_skip, private: bool)`.

The rules these commands follow are the pure functions tested in Task 2. This task is wiring whose only full test is a live bot (Task 7); keep each change exactly as written.

- [ ] **Step 1: Ephemeral-capable reply helpers**

`crack-core/src/utils.rs` — replace `send_embed_response_poise` with:

```rust
/// Sends a reply response with an embed.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise<'ctx>(
    ctx: CrackContext<'ctx>,
    embed: CreateEmbed<'ctx>,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    send_embed_response_poise_as(ctx, embed, false).await
}

/// [`send_embed_response_poise`], ephemeral when `ephemeral` is set.
#[cfg(not(tarpaulin_include))]
pub async fn send_embed_response_poise_as<'ctx>(
    ctx: CrackContext<'ctx>,
    embed: CreateEmbed<'ctx>,
    ephemeral: bool,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    let params = SendMessageParams::default()
        .with_ephemeral(ephemeral)
        .with_embed(Some(embed))
        .with_reply(true);

    ctx.send_message_owned(params).await
}
```

`crack-core/src/messaging/interface.rs` — replace `send_search_message` with:

```rust
pub async fn send_search_message<'ctx>(
    ctx: &'ctx CrackContext<'_>,
) -> CrackedResult<ReplyHandle<'ctx>> {
    send_search_message_as(ctx, false).await
}

/// The "searching…" reply, ephemeral when `ephemeral` is set.
pub async fn send_search_message_as<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    ephemeral: bool,
) -> CrackedResult<ReplyHandle<'ctx>> {
    let embed = CreateEmbed::default().description(format!("{}", CrackedMessage::Search));
    let msg = crate::utils::send_embed_response_poise_as(*ctx, embed, ephemeral).await?;
    Ok(msg)
}
```

- [ ] **Step 2: `/play`**

In `crack-core/src/commands/music/doplay.rs`, add `guild::operations::GuildSettingsOperations,` to the `use crate::{ ... }` block. In `play_internal`, replace

```rust
    let search_msg = msg_int::send_search_message(&ctx).await?;
```

with

```rust
    // `ephemeral_replies` decides whether this reply -- and the edit that turns
    // it into the result -- is seen by its author alone.
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let private = crate::messaging::status::reply_privately(
        ctx.data().get_ephemeral_replies(guild_id).await,
        is_prefix,
    );
    let search_msg = msg_int::send_search_message_as(&ctx, private).await?;
```

Then, directly after the `if let Some(short) = shortfall { ... }` block (before the `// [Manage Messages]` comment):

```rust
    // A `/play` that started a song is a now-playing moment. The status follows
    // the reply, so a visible reply ends up directly above it.
    if queue.len() == 1 {
        let serenity_ctx = ctx.serenity_context();
        crate::messaging::status::show_now_playing(
            &ctx.data(),
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
    }
```

- [ ] **Step 3: `/skip` (and voteskip's call)**

In `crack-core/src/commands/music/skip.rs`:
- Imports: add `crate::guild::operations::GuildSettingsOperations`, `crate::http_utils::SendMessageParams`, and `serenity::all::{Colour, CreateEmbed}` (alongside the existing `serenity::all::Message`).
- In `skip`, directly after `let to_skip = num_tracks.unwrap_or(1) as usize;`:
  ```rust
      let private = crate::messaging::status::reply_privately(
          ctx.data().get_ephemeral_replies(guild_id).await,
          ctx.is_prefix(),
      );
  ```
- Replace the tail `create_skip_response(ctx, &handler, tracks_to_skip).await?;\n    Ok(())` with:
  ```rust
      create_skip_response(ctx, &handler, tracks_to_skip, private).await?;
      let still_playing = handler.queue().current().is_some();
      // 🔑 Released before the status update, which takes the Call lock itself.
      drop(handler);
      if still_playing {
          let serenity_ctx = ctx.serenity_context();
          crate::messaging::status::show_now_playing(
              &ctx.data(),
              serenity_ctx.http.clone(),
              serenity_ctx.cache.clone(),
              guild_id,
              &call,
          )
          .await;
      }
      Ok(())
  ```
- `create_skip_response`: add the parameter `private: bool` after `tracks_to_skip: usize`, and replace

  ```rust
      ctx.send_reply(send_msg, true)
          .await?
          .into_message()
          .await
          .map_err(|e| e.into())
  ```

  with

  ```rust
      // `send_reply(send_msg, true)`, plus the guild's ephemeral choice.
      let color = Colour::from(&send_msg);
      let embed: Option<CreateEmbed> = <Option<CreateEmbed>>::from(&send_msg);
      let params = SendMessageParams::new(send_msg)
          .with_color(color)
          .with_as_embed(true)
          .with_embed(embed)
          .with_reply(true)
          .with_ephemeral(private);
      ctx.send_message(params)
          .await?
          .into_message()
          .await
          .map_err(|e| e.into())
  ```

In `crack-core/src/commands/music/voteskip.rs`, change `create_skip_response(ctx, &handler, 1).await` to `create_skip_response(ctx, &handler, 1, false).await` (a vote skip keeps its visible reply; the track end it causes updates the status).

- [ ] **Step 4: `/nowplaying`**

Replace the contents of `crack-core/src/commands/music/nowplaying.rs` below the `nowplaying` command (keep the `#[poise::command(...)] pub async fn nowplaying` wrapper unchanged) and its imports with:

```rust
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::status::{
    now_playing_pointer, pointer_goes_first, reply_privately, show_now_playing,
};
use crate::poise_ext::{ContextExt, PoiseContextExt};
use crate::utils::get_track_handle_metadata;
use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    Context, Error,
};
use poise::CreateReply;
```

```rust
/// Get the currently playing track. Internal function.
///
/// The status message shows the track; this replies with a one-line pointer
/// to it (spec: `/nowplaying` ordering).
pub async fn nowplaying_internal(ctx: Context<'_>) -> Result<(), Error> {
    let call = ctx.get_call().await?;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // 🔑 The Call lock is released at the end of this statement: the status
    // update below takes it.
    let track = call
        .lock()
        .await
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;
    let title = get_track_handle_metadata(&track)
        .await
        .ok()
        .and_then(|meta| meta.title)
        .unwrap_or_default();

    let data = ctx.data();
    let private = reply_privately(data.get_ephemeral_replies(guild_id).await, ctx.is_prefix());
    let music_channel = data.get_music_channel(guild_id).await;
    let serenity_ctx = ctx.serenity_context();

    if pointer_goes_first(private, music_channel, ctx.channel_id()) {
        ctx.send(
            CreateReply::default()
                .content(now_playing_pointer(&title, None))
                .ephemeral(private),
        )
        .await?;
        show_now_playing(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
    } else {
        let shown = show_now_playing(
            &data,
            serenity_ctx.http.clone(),
            serenity_ctx.cache.clone(),
            guild_id,
            &call,
        )
        .await;
        let link = shown.map(|status| status.id.link(status.channel, Some(guild_id)));
        ctx.send(
            CreateReply::default()
                .content(now_playing_pointer(&title, link))
                .ephemeral(private),
        )
        .await?;
    }
    Ok(())
}
```

- [ ] **Step 5: Build and gate**

Run the three gate commands. Expected: clean. (If `send_search_message_as`'s lifetimes do not compile, match the exact lifetime shape of the original `send_search_message`, which compiled with `send_embed_response_poise(*ctx, embed)`.)

- [ ] **Step 6: Commit**

```bash
git add crack-core/src/utils.rs crack-core/src/messaging/interface.rs crack-core/src/commands/music/doplay.rs crack-core/src/commands/music/skip.rs crack-core/src/commands/music/voteskip.rs crack-core/src/commands/music/nowplaying.rs
git commit -F - <<'EOF'
status: /play, /skip and /nowplaying reply around the status message

`/play` and `/skip` reply ephemerally when the guild's
`ephemeral_replies` is on, then update the status. `/nowplaying` replies
with a one-line pointer: first, then the status below it, when a visible
reply shares the status's channel; otherwise after the status, with a
jump link to it.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 6: A playlist `/play` says what it did

**Files:**
- Modify: `crack-core/src/commands/music/doplay.rs`

**Interfaces:**
- Produces: `fn playlist_queued_embed<'a>() -> CreateEmbed<'a>` (private to `doplay.rs`).

- [ ] **Step 1: Extract the embed unchanged, and write the failing test**

In `doplay.rs`, directly above `pub async fn build_play_embed`:

```rust
/// The reply to a playlist `/play`: what [`CrackedMessage::PlaylistQueued`]
/// says, not its variant name.
fn playlist_queued_embed<'a>() -> CreateEmbed<'a> {
    CreateEmbed::default().description(format!("{:?}", CrackedMessage::PlaylistQueued))
}
```

In `build_play_embed`, in the `(QueryType::PlaylistLink(_) | QueryType::KeywordList(_), y)` arm, replace

```rust
                    CreateEmbed::default()
                        .description(format!("{:?}", CrackedMessage::PlaylistQueued))
```

with `playlist_queued_embed()`.

Append to the end of `doplay.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::messages::PLAY_PLAYLIST;

    /// 🪤 Seen on production v0.12.1: a playlist `/play` replied with the literal
    /// text "PlaylistQueued", `{:?}` of the message instead of its text.
    #[test]
    fn a_queued_playlist_is_announced_in_words() {
        let json = serde_json::to_string(&playlist_queued_embed()).expect("an embed serializes");

        assert!(json.contains(PLAY_PLAYLIST), "{json}");
        assert!(!json.contains("PlaylistQueued"), "{json}");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `SQLX_OFFLINE=true cargo test -p crack-core --lib a_queued_playlist_is_announced_in_words`
Expected: FAIL — the JSON holds `"description":"PlaylistQueued"`.

- [ ] **Step 3: Fix**

```rust
fn playlist_queued_embed<'a>() -> CreateEmbed<'a> {
    CreateEmbed::default().description(CrackedMessage::PlaylistQueued.to_string())
}
```

- [ ] **Step 4: Run it to verify it passes; sabotage**

Run the same test: PASS. Sabotage: revert the body to the `format!("{:?}", ...)` form, confirm FAIL, restore by checksum.

- [ ] **Step 5: Gate and commit**

```bash
git add crack-core/src/commands/music/doplay.rs
git commit -F - <<'EOF'
play: a playlist reply says "Added playlist to queue", not PlaylistQueued

`build_play_embed` formatted `CrackedMessage::PlaylistQueued` with `{:?}`
since 2024-12-09, so every playlist `/play` replied with the variant
name. It uses the message's text now.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

---

### Task 7: Version, full gate, and a branch image on TuneTitan

**Files:**
- Modify: the ten member manifests: `crack-bf/Cargo.toml crack-cli/Cargo.toml crack-core/Cargo.toml crack-gpt/Cargo.toml crack-musicreco/Cargo.toml crack-osint/Cargo.toml crack-sleevenote/Cargo.toml crack-testing/Cargo.toml crack-types/Cargo.toml crack-voting/Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Bump 0.12.1 → 0.13.0**

In each of the ten manifests change the package line `version = "0.12.1"` to `version = "0.13.0"` (only that exact line; other `version =` lines are dependencies). Then refresh the lock without `--locked`:

```bash
SQLX_OFFLINE=true cargo check --workspace
git diff Cargo.lock | grep '^[-+]' | grep -v '^[-+]version = "0.1[23]\.[01]"$' | grep -v '^+++\|^---'
```

Expected: the second command prints nothing (only our ten version lines changed).

- [ ] **Step 2: Full gate**

```bash
cargo fmt --all -- --check
SQLX_OFFLINE=true cargo clippy --workspace --all-targets --locked -- -D warnings
SQLX_OFFLINE=true cargo test --workspace --locked
```

Expected: all clean; record the crack-core pass count.

- [ ] **Step 3: Commit the bump**

```bash
git add crack-bf/Cargo.toml crack-cli/Cargo.toml crack-core/Cargo.toml crack-gpt/Cargo.toml crack-musicreco/Cargo.toml crack-osint/Cargo.toml crack-sleevenote/Cargo.toml crack-testing/Cargo.toml crack-types/Cargo.toml crack-voting/Cargo.toml Cargo.lock
git commit -F - <<'EOF'
v0.13.0: the floating status message

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
```

- [ ] **Step 4: Branch image on TuneTitan (the only test of the gateway-driven parts)**

```bash
git push -u origin feat/floating-status-message
gh workflow run docker.yml --repo cycle-five/cracktunes --ref feat/floating-status-message </dev/null
```

Find the run (`gh run list --repo cycle-five/cracktunes --workflow docker.yml --branch feat/floating-status-message --limit 1`), confirm its `headSha` is the branch head, and wait for it in the background (`gh run watch <id> --exit-status`). The tag is the sanitized branch name, `feat-floating-status-message`.

🔑 **TuneTitan migrates through its `migrate` one-shot** (homelab branch `tunetitan-migrate-oneshot`, added 2026-09-15; the same service `bots/` has). The branch image loads guild settings with `SELECT *` into a struct that now has `ephemeral_replies`, so the column must exist before the bot starts — the one-shot runs first and the bot waits for it to exit 0. The docker.yml dispatch publishes `cracktunes-migrate:feat-floating-status-message` beside the bot image.

🪤 **Override BOTH images.** Setting only `TUNETITAN_CRACKTUNES_IMAGE` runs v0.12.1's migrations (no new column) under the branch's queries, and the migrate container still exits 0; `verify` catches it by comparing the two revision labels. 🪤 **Pull both first:** the pull policy is `missing`, so a branch tag already on the host is not refreshed.

The column is added last with a default, so TuneTitan can still be rolled back to v0.12.1 (its compiled queries read only the columns they know).

```bash
ssh root@192.168.1.116 'for i in cracktunes cracktunes-migrate; do docker pull -q ghcr.io/cycle-five/$i:feat-floating-status-message && docker image inspect ghcr.io/cycle-five/$i:feat-floating-status-message --format "$i {{index .Config.Labels \"org.opencontainers.image.revision\"}}"; done' </dev/null
cd ~/projects/homelab && TUNETITAN_CRACKTUNES_IMAGE=ghcr.io/cycle-five/cracktunes:feat-floating-status-message TUNETITAN_CRACKTUNES_MIGRATE_IMAGE=ghcr.io/cycle-five/cracktunes-migrate:feat-floating-status-message ./homelab.sh up tunetitan </dev/null
cd ~/projects/homelab && ./homelab.sh verify tunetitan </dev/null
ssh root@192.168.1.116 'docker logs tunetitan-migrate-1 2>&1 | tail -3' </dev/null
```

Run these from a homelab checkout that has the migrate service (master once the homelab PR merges). A checkout without it has no `TUNETITAN_CRACKTUNES_MIGRATE_IMAGE` and would start the branch bot against an unmigrated database.

Confirm both revision labels are the branch head, `verify` passes (it now reports `23/23 migrations`), the migrate log shows `Applied 20260915120000/migrate ephemeral replies`, the bot log shows `Loaded settings for guild` lines, and there is no `ERROR`/`panicked` (`ssh root@192.168.1.116 docker logs tunetitan-cracktunes-1`).

Deploying the branch image to TuneTitan is the established pre-merge test; deploying to production (`bots`) is **not** part of this plan.

- [ ] **Step 5: Hand the owner this test list and stop**

1. Autoplay **off**, a playlist playing: at each song change the status updates; with no chat in between it is **edited** (same message), with chat in between it **moves** to the bottom.
2. `/nowplaying` with `ephemeral_replies` off: a visible "Now playing: … ↓" line and the status right below it.
3. `/settings toggle ephemeral`, then `/nowplaying`, `/skip`, `/play`: replies are only visible to the author; the status edits in place.
4. `/clean` while playing: the status is deleted with the rest; the next song change sends a fresh one.
5. `/stop`: the status turns into "⏹️ Finished"; a later `/play` in the same channel continues that message.
6. Run a music command in a second channel (no music channel set): the status moves there.
7. A `/gp` round: no status updates during it.
8. A playlist `/play` replies "📃 Added playlist to queue!".

After the owner's OK: PR → CI → merge → `git tag -s v0.13.0` → push tag → homelab pins → TuneTitan then `bots`.
