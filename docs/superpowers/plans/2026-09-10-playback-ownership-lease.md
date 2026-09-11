# Playback Ownership Lease Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make queue mutation in a guild impossible without holding a token that
proves nobody else owns playback, closing #434 and #333 together.

**Architecture:** Two per-guild concepts on `Data`, deliberately separate because
their lifetimes differ by three orders of magnitude. *Ownership* (`PlaybackOwner`)
is long-lived and declarative, written only inside the five methods that already
move `gp_games` so the two cannot diverge. *Exclusion* (`QueueGuard`) is a
short-lived RAII guard over a per-guild mutex. Every queue mutation moves behind a
`music/queue.rs` helper that takes `&QueueGuard`, so a command physically cannot
mutate the queue without passing the ownership check.

**Tech Stack:** Rust, tokio (`Mutex`, `OwnedMutexGuard`), dashmap, songbird
(`Call`, `TrackQueue`, `TrackHandle`), poise/serenity.

**Spec:** `docs/superpowers/specs/2026-09-10-playback-ownership-lease-design.md`

## Global Constraints

- **Resolution stays outside the lease.** `resolve_track_many` shells out to
  `yt-dlp` and takes 8–15s cold. Resolve first, then take the guard for the
  enqueue and the reply. A guard held across resolution is a defect.
- **Lock order: playback lease first, join token second, never the reverse.**
  Carry a `debug_assert`, not only a comment.
- **Never hold a `dashmap` reference across an `.await`.** It deadlocks. Clone
  the `Arc` out and let the entry ref drop before awaiting — the pattern
  `JoinVCToken::acquire` (`crack-core/src/poise_ext.rs:586-594`) already uses.
- **The lease is process-local.** It is never written to Postgres.
- **No test may reach live YouTube.** Use `#[ignore = "hits live YouTube"]` if a
  test genuinely needs the network. See `docs/testing.md`.
- **`GP_BLOCKED_COMMANDS` is not removed.** It stays as the early, friendly
  refusal.
- Commit trailer on every commit:
  `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)`

### ⚠️ Raise lease refusals from command bodies, not from poise checks

`CrackedError::GameInProgress` reaches a user only when it is returned from a
command **body**. Returned from a poise **check** it becomes
`FrameworkError::CommandCheckFailed`, which `on_error`
(`crack-core/src/config.rs:32`) has **no arm for** — it falls through to poise's
builtin, which logs and never replies, and Discord times the interaction out
after three seconds with "The application did not respond". That is **#467**,
open and unfixed.

Every `lock_queue` call in this plan is inside a command body or an event
handler, so none of them hit this. **Do not move one into a check** to make it
fire earlier: it would become invisible. `GP_BLOCKED_COMMANDS` already runs in
`cmd_check_music` and has the same problem — that is #467's job, not this
plan's.

### ⚠️ Re-derive the site lists before you start

`GP_BLOCKED_COMMANDS` grew 11 → 19 in the four days between the issue and the
spec. The tables below were accurate at `0d5f4d0`. Run these first and reconcile:

```bash
grep -rn "\.enqueue(\|modify_queue\|\.dequeue(\|queue()\.\(skip\|stop\|pause\|resume\)()" \
     --include='*.rs' crack-core/src
grep -rn "gp_games\.\(insert\|remove\|entry\)" --include='*.rs' crack-core/src
```

If a site exists that is not in this plan, add it to the task that owns its file
and say so in the task report.

---

## File Structure

| file | responsibility |
|---|---|
| **Create** `crack-core/src/music/lease.rs` | `PlaybackOwner`, `QueueGuard`, and the `Data` impl block for claim/release/lock. New file rather than `lib.rs` (already 600+ lines of `Data`) or `gp.rs` (5,651 lines, and #461 proposes splitting it). |
| **Modify** `crack-core/src/music/mod.rs` | declare and re-export `lease` |
| **Modify** `crack-core/src/lib.rs` | two new `DataInner` fields + their `Default` |
| **Modify** `crack-core/src/commands/music/gp.rs` | 5 ownership sites, 4 mutation sites |
| **Modify** `crack-core/src/commands/music/gp_persist.rs` | route the raw `gp_games.remove()` through a method |
| **Modify** `crack-core/src/music/queue.rs` | helpers take `&QueueGuard`; `Inserted` return; fix check-then-act |
| **Modify** `crack-core/src/commands/music/{skip,shuffle,remove,clear}.rs` | call helpers instead of `handler.queue()` |
| **Modify** `crack-core/src/handlers/track_end.rs` | event-handler carve-out |
| **Modify** `crack-core/src/commands/music/doplay.rs` | reply from the insertion result |

---

### Task 1: The lease primitives

**Files:**
- Create: `crack-core/src/music/lease.rs`
- Modify: `crack-core/src/music/mod.rs`
- Modify: `crack-core/src/lib.rs` (`DataInner` fields + `Default`)
- Test: inline `#[cfg(test)] mod test` in `crack-core/src/music/lease.rs`

**Interfaces:**
- Consumes: `CrackedError::GameInProgress` (`crack-core/src/errors.rs:109`),
  `Data`/`DataInner` (`crack-core/src/lib.rs:367`).
- Produces:
  ```rust
  pub enum PlaybackOwner { Free, Game }          // Copy, Eq, Default = Free
  pub struct QueueGuard;                          // opaque; .guild_id() -> GuildId
  impl Data {
      pub fn playback_owner(&self, guild_id: GuildId) -> PlaybackOwner;
      pub fn claim_playback(&self, guild_id: GuildId, owner: PlaybackOwner) -> Result<(), CrackedError>;
      pub fn release_playback(&self, guild_id: GuildId);
      pub async fn lock_queue(&self, guild_id: GuildId, as_: PlaybackOwner) -> Result<QueueGuard, CrackedError>;
  }
  ```

- [ ] **Step 1: Add the two `DataInner` fields**

In `crack-core/src/lib.rs`, immediately after the `join_vc_tokens` field
(currently line 378) so the two per-guild locks sit together:

```rust
    /// Who owns playback per guild. Written ONLY by `claim_playback` /
    /// `release_playback`, which are called from inside the methods that move
    /// `gp_games` -- see `music/lease.rs`. Do not write it anywhere else.
    pub playback_owners: dashmap::DashMap<serenity::GuildId, crate::music::lease::PlaybackOwner>,
    /// Per-guild queue-mutation mutex. Held for milliseconds; see `lock_queue`.
    pub queue_locks: dashmap::DashMap<serenity::GuildId, Arc<tokio::sync::Mutex<()>>>,
```

And in the `Default for DataInner` impl, beside `join_vc_tokens: Default::default(),`
(currently line 595):

```rust
            playback_owners: Default::default(),
            queue_locks: Default::default(),
```

- [ ] **Step 2: Declare the module**

In `crack-core/src/music/mod.rs`, after `pub(crate) mod queue;`:

```rust
pub mod lease;

pub use lease::{PlaybackOwner, QueueGuard};
```

- [ ] **Step 3: Write the failing tests**

Create `crack-core/src/music/lease.rs` containing ONLY this test module for now
(the code comes in Step 5), plus `use` lines so it compiles once the types exist:

```rust
#[cfg(test)]
mod test {
    use super::*;
    use crate::{Data, DataInner};
    use std::sync::Arc;

    const G: GuildId = GuildId::new(1);
    const H: GuildId = GuildId::new(2);

    fn data() -> Data {
        Data(Arc::new(DataInner::default()))
    }

    #[test]
    fn a_guild_with_no_claim_is_free() {
        assert_eq!(data().playback_owner(G), PlaybackOwner::Free);
    }

    #[test]
    fn claim_then_release_round_trips() {
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).expect("free guild accepts a claim");
        assert_eq!(d.playback_owner(G), PlaybackOwner::Game);
        d.release_playback(G);
        assert_eq!(d.playback_owner(G), PlaybackOwner::Free);
    }

    #[test]
    fn releasing_a_guild_that_owns_nothing_is_a_no_op() {
        // The game-end paths are event-driven and can each arrive first, so a
        // double release is normal, not an error.
        let d = data();
        d.release_playback(G);
        d.release_playback(G);
        assert_eq!(d.playback_owner(G), PlaybackOwner::Free);
    }

    #[test]
    fn re_claiming_as_the_same_owner_succeeds() {
        // Defensive idempotency: `gp_start` only claims from an Entry::Vacant
        // branch, so this should be unreachable -- but failing here would turn a
        // harmless double-claim into a refused /gp start.
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        d.claim_playback(G, PlaybackOwner::Game).expect("same owner may re-claim");
    }

    #[test]
    fn a_claim_is_per_guild() {
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        assert_eq!(d.playback_owner(H), PlaybackOwner::Free);
    }

    #[tokio::test]
    async fn a_free_guild_hands_out_a_guard() {
        let d = data();
        let guard = d.lock_queue(G, PlaybackOwner::Free).await.expect("free guild locks");
        assert_eq!(guard.guild_id(), G);
    }

    #[tokio::test]
    async fn a_game_owned_guild_refuses_a_non_owner_immediately() {
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        let err = d.lock_queue(G, PlaybackOwner::Free).await.unwrap_err();
        assert!(matches!(err, CrackedError::GameInProgress));
    }

    #[tokio::test]
    async fn the_owner_can_still_lock_its_own_queue() {
        // /gp mutates its own queue at four sites; the lease must not lock the
        // owner out of the thing it owns.
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        d.lock_queue(G, PlaybackOwner::Game).await.expect("the owner may lock");
    }

    #[tokio::test]
    async fn the_refusal_does_not_wait_for_the_mutex() {
        // Fail-fast is the whole point: nobody waits out a half-hour game. If the
        // ownership check happened after the mutex, this would block forever.
        let d = data();
        d.claim_playback(G, PlaybackOwner::Game).unwrap();
        let _held = d.lock_queue(G, PlaybackOwner::Game).await.unwrap();
        let refused = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            d.lock_queue(G, PlaybackOwner::Free),
        )
        .await
        .expect("must refuse without waiting on the mutex");
        assert!(matches!(refused, Err(CrackedError::GameInProgress)));
    }

    #[tokio::test]
    async fn two_lockers_of_a_free_guild_serialise() {
        let d = data();
        let first = d.lock_queue(G, PlaybackOwner::Free).await.unwrap();
        let blocked = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            d.lock_queue(G, PlaybackOwner::Free),
        )
        .await;
        assert!(blocked.is_err(), "the second locker must wait");
        drop(first);
        d.lock_queue(G, PlaybackOwner::Free)
            .await
            .expect("the lock is released on drop");
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p crack-core --lib music::lease`
Expected: FAIL to compile — `PlaybackOwner`, `QueueGuard`, `playback_owner`,
`claim_playback`, `release_playback`, `lock_queue` are not defined.

- [ ] **Step 5: Write the implementation**

Prepend to `crack-core/src/music/lease.rs`, above the test module:

```rust
//! Per-guild playback ownership and queue exclusion.
//!
//! Two concepts, deliberately not merged, because their lifetimes differ by
//! three orders of magnitude:
//!
//! * **Ownership** ([`PlaybackOwner`]) -- long-lived and declarative. Who owns
//!   playback in this guild. Claimed by `/gp start`, released at game end,
//!   checked cheaply, and **fails fast**.
//! * **Exclusion** ([`QueueGuard`]) -- short-lived and operational. A per-guild
//!   mutex serialising queue mutation, held for milliseconds.
//!
//! # 🔑 Ownership is written in exactly five places
//!
//! [`Data::claim_playback`] and [`Data::release_playback`] are called from
//! *inside* the methods that move `gp_games`, never alongside them. An explicit
//! lease is a second source of truth, and a lease that outlives its game wedges
//! `/play` forever with no game left to end. Co-locating the two writes makes
//! divergence structurally impossible rather than merely unlikely. If you find
//! yourself calling `claim_playback` from a new place, you are adding the bug
//! this design exists to prevent.
//!
//! # 🪤 `QueueGuard` is NOT shaped like `JoinVCToken`
//!
//! #434 describes it as "in the shape of `JoinVCToken`", and the *intent* is the
//! same -- an unforgeable proof carried in the type system -- but the mechanism
//! differs and assuming otherwise will produce a bug:
//!
//! | | `JoinVCToken` | `QueueGuard` |
//! |---|---|---|
//! | `acquire` | not `async`, takes no lock | `async`, holds the lock on return |
//! | who locks | the consumer (`join_vc`) | the constructor |
//! | what it proves | you went through the right door | you hold exclusion *now* |
//!
//! # Lock ordering
//!
//! The lease sits alongside `join_vc_tokens`, not replacing it, so there are two
//! per-guild locks. **Playback lease first, join token second, never the
//! reverse.**

use crate::errors::CrackedError;
use crate::Data;
use dashmap::mapref::entry::Entry;
use poise::serenity_prelude::GuildId;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Who owns playback in a guild.
///
/// An enum rather than a bool because a third owner is foreseeable; adding one
/// now would be speculative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackOwner {
    /// Nobody owns playback; ordinary music commands may mutate the queue.
    #[default]
    Free,
    /// A `/gp` game owns playback for the duration of the game.
    Game,
}

/// Proof that the holder may mutate this guild's queue, and that nobody else is
/// doing so concurrently.
///
/// Obtained only from [`Data::lock_queue`]. The exclusion is released when this
/// is dropped, so do not hold one across a slow operation -- in particular never
/// across track resolution, which takes 8-15s cold.
#[derive(Debug)]
pub struct QueueGuard {
    guild_id: GuildId,
    /// Dropping this releases the per-guild mutex. Never read.
    _exclusion: OwnedMutexGuard<()>,
}

impl QueueGuard {
    /// The guild this guard permits mutation in.
    #[must_use]
    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }
}

impl Data {
    /// Who owns playback in this guild. A guild with no claim is [`PlaybackOwner::Free`].
    #[must_use]
    pub fn playback_owner(&self, guild_id: GuildId) -> PlaybackOwner {
        self.playback_owners
            .get(&guild_id)
            .map(|owner| *owner)
            .unwrap_or_default()
    }

    /// Take ownership of playback in a guild.
    ///
    /// # Errors
    ///
    /// [`CrackedError::GameInProgress`] if a *different* owner already holds it.
    /// Re-claiming as the current owner succeeds: the callers claim from inside
    /// an `Entry::Vacant` branch on `gp_games`, so a collision should be
    /// unreachable, and failing here would turn a harmless double-claim into a
    /// refused `/gp start`.
    pub fn claim_playback(
        &self,
        guild_id: GuildId,
        owner: PlaybackOwner,
    ) -> Result<(), CrackedError> {
        match self.playback_owners.entry(guild_id) {
            Entry::Vacant(slot) => {
                slot.insert(owner);
                Ok(())
            },
            Entry::Occupied(held) if *held.get() == owner => Ok(()),
            Entry::Occupied(_) => Err(CrackedError::GameInProgress),
        }
    }

    /// Give up ownership of playback in a guild.
    ///
    /// Infallible and idempotent. Releasing a guild that owns nothing is a
    /// no-op, not an error: the game-end paths are event-driven and each can
    /// arrive first.
    pub fn release_playback(&self, guild_id: GuildId) {
        self.playback_owners.remove(&guild_id);
    }

    /// Acquire exclusive permission to mutate this guild's queue.
    ///
    /// Game-versus-command fails fast; command-versus-command waits, and the
    /// wait is short because resolution happens outside the guard.
    ///
    /// # Errors
    ///
    /// [`CrackedError::GameInProgress`] -- immediately, without touching the
    /// mutex -- when a game owns playback and `as_` is not that owner.
    pub async fn lock_queue(
        &self,
        guild_id: GuildId,
        as_: PlaybackOwner,
    ) -> Result<QueueGuard, CrackedError> {
        // Ownership is checked BEFORE the mutex, deliberately. A caller refused
        // on ownership must not first wait out whoever holds the mutex.
        let owner = self.playback_owner(guild_id);
        if owner != PlaybackOwner::Free && owner != as_ {
            return Err(CrackedError::GameInProgress);
        }

        // 🪤 The `.clone()` matters: a dashmap reference held across the await
        // below deadlocks the shard. Cloning the Arc lets the entry ref drop at
        // the end of this statement. Same shape as `JoinVCToken::acquire`.
        let lock = self
            .queue_locks
            .entry(guild_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        Ok(QueueGuard {
            guild_id,
            _exclusion: lock.lock_owned().await,
        })
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p crack-core --lib music::lease`
Expected: PASS, 10 tests.

- [ ] **Step 7: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/music/lease.rs crack-core/src/music/mod.rs crack-core/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(playback): a per-guild ownership lease and queue-exclusion guard

Ownership is long-lived and fails fast; exclusion is a short RAII guard over a
per-guild mutex. Ownership is checked BEFORE the mutex so a refused caller does
not first wait out whoever holds it.

Nothing calls these yet; the call sites arrive over the following commits.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 2: Co-locate claim/release with the five `gp_games` sites

This is the load-bearing task. After it, the lease and the games map cannot
disagree.

**Files:**
- Modify: `crack-core/src/commands/music/gp.rs` (`gp_start` ~:1003,
  `gp_remove_if_parked` ~:1582, `gp_remove` ~:1596, `gp_restore` ~:1611)
- Modify: `crack-core/src/commands/music/gp_persist.rs` (~:619)
- Test: inline in `crack-core/src/commands/music/gp.rs` test module

**Interfaces:**
- Consumes: `Data::claim_playback`, `Data::release_playback`,
  `Data::playback_owner`, `PlaybackOwner` from Task 1.
- Produces: the invariant *"`gp_games.contains_key(g)` ⟺ `playback_owner(g) == Game`"*,
  which every later task may rely on.

- [ ] **Step 1: Write the failing invariant test**

Add to the existing `#[cfg(test)] mod test` in `crack-core/src/commands/music/gp.rs`.

**Three fixtures are needed. Reuse the module's own if they exist under other
names; otherwise write them:**

- `recording() -> (Data, mpsc::UnboundedReceiver<GpPersist>)` — a `Data` whose
  persistence writes land in a channel rather than Postgres. This one **already
  exists** in `crack-core/src/commands/music/gp_persist.rs`'s test module; copy
  it or lift it into a shared test helper.
- `a_game(guild_id: GuildId) -> GpGame` — a minimal game, enough for
  `gp_restore` to accept. Build it the way the existing `gp.rs` tests build one.
- `start_a_game(data: &Data, guild_id: GuildId)` — calls `data.gp_start(..)` with
  minimal arguments and asserts it succeeded, so the tests below read as
  lifecycle rather than setup.

`G` and `A` are the module's existing `GuildId`/`UserId` constants
(`GuildId::new(1)`, `UserId::new(100)` in the `gp_persist.rs` tests).

```rust
    /// The lease and the games map must agree after EVERY game-lifecycle method.
    /// This is the regression guard for the drift the co-location design exists
    /// to prevent: a lease that outlives its game wedges /play forever, with no
    /// game left to end.
    fn assert_lease_agrees(data: &Data, guild_id: GuildId) {
        let has_game = data.gp_games.contains_key(&guild_id);
        let owned = data.playback_owner(guild_id) == PlaybackOwner::Game;
        assert_eq!(
            has_game, owned,
            "gp_games says {has_game} but the lease says {owned} for {guild_id}"
        );
    }

    #[test]
    fn gp_start_claims_playback() {
        let (data, _rx) = recording();
        assert_lease_agrees(&data, G);
        start_a_game(&data, G);
        assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
        assert_lease_agrees(&data, G);
    }

    #[test]
    fn gp_remove_releases_playback() {
        let (data, _rx) = recording();
        start_a_game(&data, G);
        data.gp_remove(G);
        assert_eq!(data.playback_owner(G), PlaybackOwner::Free);
        assert_lease_agrees(&data, G);
    }

    #[test]
    fn gp_remove_if_parked_releases_only_when_it_removes() {
        let (data, _rx) = recording();
        start_a_game(&data, G);

        // Not parked: nothing is removed, so nothing is released.
        assert!(!data.gp_remove_if_parked(G));
        assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
        assert_lease_agrees(&data, G);

        data.gp_park_for_end(G, A, true).expect("park");
        assert!(data.gp_remove_if_parked(G));
        assert_eq!(data.playback_owner(G), PlaybackOwner::Free);
        assert_lease_agrees(&data, G);
    }

    #[test]
    fn gp_restore_claims_playback() {
        let (data, _rx) = recording();
        let game = a_game(G);
        assert!(data.gp_restore(G, game));
        assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
        assert_lease_agrees(&data, G);
    }

    #[test]
    fn a_refused_restore_does_not_claim() {
        // `gp_restore` returns false when /gp start got there first. The lease
        // is already held by that game; the refused restore must not touch it.
        let (data, _rx) = recording();
        start_a_game(&data, G);
        assert!(!data.gp_restore(G, a_game(G)));
        assert_lease_agrees(&data, G);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-core --lib gp::test -- lease`
Expected: FAIL — `playback_owner` returns `Free` after `gp_start`.

- [ ] **Step 3: Claim inside the two insert paths**

In `gp.rs`, `Data::gp_start` — the `Entry::Vacant` branch around line 1003. Claim
**inside** the branch that inserts, so a refused start claims nothing:

```rust
        let dashmap::mapref::entry::Entry::Vacant(slot) = self.gp_games.entry(guild_id) else {
            return Err(CrackedError::GameInProgress);
        };
        // 🔑 Claimed here, inside the branch that inserts, so the lease and the
        // map move together. See music/lease.rs on why this must not be called
        // from anywhere else.
        self.claim_playback(guild_id, PlaybackOwner::Game)?;
        slot.insert(game);
```

In `Data::gp_restore` around line 1611, in the `Vacant` arm only:

```rust
        match self.gp_games.entry(guild_id) {
            dashmap::mapref::entry::Entry::Vacant(slot) => {
                // Reclaimed before anything is restored into the guild -- the
                // arbitration #431 needed, so a resumed game and a restored
                // queue cannot both take the voice channel.
                if self.claim_playback(guild_id, PlaybackOwner::Game).is_err() {
                    return false;
                }
                slot.insert(game);
                true
            },
```

- [ ] **Step 4: Release inside the two remove paths**

In `Data::gp_remove_if_parked` (~:1582), inside the `if let Some(..)`:

```rust
            if let Some((_, game)) = self.gp_games.remove(&guild_id) {
                self.release_playback(guild_id);
                self.gp_mark_finished(&game, GpOutcome::Ended);
            }
```

In `Data::gp_remove` (~:1596), immediately after the successful remove:

```rust
        let (_, game) = self.gp_games.remove(&guild_id)?;
        self.release_playback(guild_id);
```

- [ ] **Step 5: Route the raw remove through the method**

`gp_persist.rs` around line 619 currently calls `data.gp_games.remove(&guild_id)`
directly on the rejoin-failure path, bypassing `Data::gp_remove` — a defect in
its own right, and with the lease it would strand ownership. Replace:

```rust
            data.gp_games.remove(&guild_id);
```

with:

```rust
            // Through the method, not the map: `gp_remove` is the one place a
            // game ends, and it is what releases the playback lease. A raw
            // remove here would leave the guild owned by a game that no longer
            // exists, and /play refused forever.
            data.gp_remove(guild_id);
```

`gp_remove` also writes a `GpOutcome`; the surrounding code then calls the
`gp_mark_finished` *free function* with `GpOutcome::Lost`. Keep that call — the
free function is the authoritative database write for this path — and note in a
comment that `gp_remove`'s own bookkeeping is superseded here.

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test -p crack-core --lib gp`
Expected: PASS, including the five new lease tests and every pre-existing `gp` test.

- [ ] **Step 7: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/commands/music/gp.rs crack-core/src/commands/music/gp_persist.rs
git commit -m "$(cat <<'EOF'
feat(gp): claim and release the playback lease where the games map moves

Claim/release live INSIDE gp_start, gp_restore, gp_remove and
gp_remove_if_parked rather than alongside them, so the lease and gp_games cannot
diverge. An invariant test asserts they agree after every one.

Also routes the rejoin-failure path in gp_resume_guild through Data::gp_remove
instead of removing from the map directly. That was already a defect -- it
bypassed the one place a game ends -- and with a lease it would strand ownership
and refuse /play forever.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 3: Phase C — report from the insertion result

Closes #333 and shrinks the critical section the later tasks have to hold.

**Files:**
- Modify: `crack-core/src/music/queue.rs` (`enqueue_resolved_tracks_back` :112-134)
- Modify: `crack-core/src/commands/music/doplay.rs` (reply construction)
- Test: inline in `crack-core/src/music/queue.rs`

**Interfaces:**
- Consumes: nothing from Tasks 1-2.
- Produces:
  ```rust
  pub struct Inserted {
      pub handles: Vec<TrackHandle>,  // what THIS call added, in order
      pub first_position: usize,      // where the first landed
      pub queue_len: usize,           // queue length after this call
  }
  pub async fn enqueue_resolved_tracks_back(
      call: &Arc<Mutex<Call>>, tracks: Vec<ResolvedTrack<'static>>, http_client: reqwest::Client,
  ) -> Result<Inserted, CrackedError>;
  ```

- [ ] **Step 1: Write the failing test**

Add to `crack-core/src/music/queue.rs`:

```rust
#[cfg(test)]
mod insertion_tests {
    use super::*;

    #[test]
    fn inserted_describes_only_this_call() {
        // #333: the bug was returning the whole queue after enqueueing, so two
        // concurrent /play calls each reported both sets of songs. `Inserted`
        // cannot express that -- it carries only what this call added.
        let inserted = Inserted {
            handles: Vec::new(),
            first_position: 3,
            queue_len: 5,
        };
        assert_eq!(inserted.count(), 0);
        assert_eq!(inserted.first_position, 3);
        assert_eq!(inserted.queue_len, 5);
    }

    #[test]
    fn count_is_the_handles_this_call_added() {
        let inserted = Inserted { handles: Vec::new(), first_position: 0, queue_len: 0 };
        assert_eq!(inserted.count(), inserted.handles.len());
    }
}
```

> A test that enqueues for real needs a songbird `Call`, which needs a voice
> connection. Do not fake one. The behavioural guarantee here is structural — the
> return type cannot express "the whole queue" — and that is what these assert.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p crack-core --lib music::queue::insertion_tests`
Expected: FAIL to compile — `Inserted` is not defined.

- [ ] **Step 3: Add `Inserted` and change the return**

In `crack-core/src/music/queue.rs`:

```rust
/// What one enqueue call put into the queue.
///
/// 🔑 Deliberately NOT the whole queue. `enqueue_resolved_tracks_back` used to
/// return `handler.queue().current_queue()` *after* enqueueing, so two
/// concurrent `/play` calls both read the post-both state and both replies
/// listed both sets of songs -- #333, reported as "one of the songs got queued
/// twice". A reply built from what *this* call inserted cannot be corrupted by
/// a concurrent one.
#[derive(Debug, Clone)]
pub struct Inserted {
    /// The handles this call added, in the order they were added.
    pub handles: Vec<TrackHandle>,
    /// Where the first of them landed in the queue.
    pub first_position: usize,
    /// Queue length after this call. For "added N, now M in queue" replies.
    pub queue_len: usize,
}

impl Inserted {
    /// How many tracks this call added.
    #[must_use]
    pub fn count(&self) -> usize {
        self.handles.len()
    }
}
```

Rewrite `enqueue_resolved_tracks_back` (currently :112-134):

```rust
pub async fn enqueue_resolved_tracks_back(
    call: &Arc<Mutex<Call>>,
    tracks: Vec<ResolvedTrack<'static>>,
    http_client: reqwest::Client,
) -> Result<Inserted, CrackedError> {
    let mut handler = call.lock().await;
    let first_position = handler.queue().len();
    let mut handles = Vec::with_capacity(tracks.len());
    for resolved in &tracks {
        match build_track(resolved, &http_client) {
            Ok(track) => handles.push(handler.enqueue(track).await),
            Err(e) => tracing::warn!("Failed to enqueue {}: {e}", resolved.get_url()),
        }
    }
    let queue_len = handler.queue().len();
    Ok(Inserted { handles, first_position, queue_len })
}
```

Note the empty-`tracks` early return is gone: the loop handles it, and the old
early return called `current_queue()` — the very thing being removed.

- [ ] **Step 4: Update the callers**

Build with `cargo check -p crack-core` and fix each caller the compiler names.
Callers that want a full queue snapshot for a message may still call
`call.lock().await.queue().current_queue()` explicitly — the point is that they
do so knowingly rather than receiving it from an enqueue.

In `doplay.rs`, build the reply from the `Inserted` rather than a re-read.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p crack-core --lib` and `cargo check -p crack-core --all-targets`
Expected: PASS, no compile errors.

- [ ] **Step 6: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/music/queue.rs crack-core/src/commands/music/doplay.rs
git commit -m "$(cat <<'EOF'
fix(queue): report what this call inserted, not the queue afterwards (#333)

enqueue_resolved_tracks_back returned handler.queue().current_queue() AFTER
enqueueing, so two concurrent /play calls both read the post-both state and both
replies listed both sets of songs -- the reported "one of the songs got queued
twice".

It now returns `Inserted`, which carries only the handles this call added. The
type cannot express the old bug.

This also shrinks the critical section the lease has to hold: a reply derived
from the insertion result needs exclusion only over the enqueue, not over the
enqueue and a re-read.

Closes #333

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 4: The funnel — `queue.rs` helpers require a `&QueueGuard`

**Files:**
- Modify: `crack-core/src/music/queue.rs` (all 7 mutation sites)

**Interfaces:**
- Consumes: `QueueGuard` (Task 1), `Inserted` (Task 3).
- Produces: every mutating helper in `music/queue.rs` takes `guard: &QueueGuard`
  as its first parameter. Later tasks call these instead of `handler.queue()`.
  Tasks 5 and 6 additionally need these **new** helpers, so define them here:

  ```rust
  /// Drop everything from `from` onward, stopping each track. Used by `/clear`.
  pub fn clear_from(guard: &QueueGuard, handler: &Call, from: usize);

  /// Drop `count` tracks after the currently-playing one. Used by `/skip`.
  pub fn drain_after_current(guard: &QueueGuard, handler: &Call, count: usize);

  /// Reorder the queue behind the currently-playing track. Used by `/shuffle`.
  pub fn shuffle_behind_current(guard: &QueueGuard, handler: &Call);

  /// Remove one track by queue index. Used by `/remove`.
  pub fn remove_at(guard: &QueueGuard, handler: &Call, index: usize);

  /// Stop the queue outright. Used by `/gp` when a game is discarded.
  pub async fn stop_queue(guard: &QueueGuard, call: &Arc<Mutex<Call>>);

  /// Pause the queue. Used by the track-end handler's autopause.
  pub async fn pause_queue(guard: &QueueGuard, call: &Arc<Mutex<Call>>);
  ```

  Each takes the guard as its first parameter and ignores it — holding the
  reference is the proof. Give each a `let _ = guard;` or name it `_guard` so
  clippy does not flag it, and a doc line saying the caller must hold exclusion.

- [ ] **Step 1: Add the guard parameter to every mutating helper**

For each of `queue_resolved_track_back`, `queue_track_ready_front`,
`_queue_track_ready_back`, `queue_track_front`, `queue_track_back`,
`_append_queue`, `enqueue_resolved_tracks_back` and
`queue_query_list_offset`, add a first parameter:

```rust
    guard: &QueueGuard,
```

and a doc line:

```rust
/// Requires a [`QueueGuard`]: the caller must hold playback exclusion for this
/// guild. That is what makes forgetting a `GP_BLOCKED_COMMANDS` entry a worse
/// error message rather than a corrupted `/gp` round.
```

Assert the guard is for the right guild wherever a `guild_id` is in scope:

```rust
    debug_assert_eq!(guard.guild_id(), guild_id, "QueueGuard is for another guild");
```

- [ ] **Step 2: Verify resolution stays outside**

In `queue_track_back` and `queue_query_list_offset`, confirm
`resolve_track`/`resolve_track_many` is called **before** the guard is used, and
that no guard is held across it. If the current shape holds a lock across
resolution, restructure so resolution completes first.

This is a Global Constraint. A guard held across an 8–15s resolution makes a
second `/play` wait 8–15s, which is barely an improvement on the bug.

- [ ] **Step 3: Build**

Run: `cargo check -p crack-core --all-targets`
Expected: errors at every caller — that is the funnel working. Do not fix callers
here; Tasks 5 and 6 own them.

- [ ] **Step 4: Commit (compiles only after Task 6)**

Because this task deliberately breaks callers, commit it together with Task 5 if
your workflow requires each commit to build. Otherwise commit with
`--no-verify` and note it in the task report.

---

### Task 5: Migrate the ordinary command call sites

**Files:**
- Modify: `crack-core/src/commands/music/skip.rs` (:39, :118, :119)
- Modify: `crack-core/src/commands/music/shuffle.rs` (:42, :71)
- Modify: `crack-core/src/commands/music/remove.rs` (:72)
- Modify: `crack-core/src/commands/music/clear.rs` (:42)

**Interfaces:**
- Consumes: `Data::lock_queue`, `PlaybackOwner` (Task 1); the guard-taking
  helpers (Task 4).
- Produces: nothing new.

- [ ] **Step 1: Apply this pattern at each site**

These commands run for ordinary users, so they lock `as_ PlaybackOwner::Free` —
which is exactly what a game refuses.

Before (`clear.rs:42`):

```rust
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();
    verify(queue.len() > 1, CrackedError::QueueEmpty)?;
    handler.queue().modify_queue(|v| { /* ... */ });
```

After:

```rust
    // Ordinary music commands mutate as `Free`; a guild a game owns refuses
    // here, which is the same refusal GP_BLOCKED_COMMANDS gives earlier and
    // more kindly. This one cannot be forgotten.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let handler = call.lock().await;
    let queue = handler.queue().current_queue();
    verify(queue.len() > 1, CrackedError::QueueEmpty)?;
    clear_from(&guard, &handler, 1);
```

Where a site does not have a natural helper yet, add one to `music/queue.rs`
taking `&QueueGuard` and call it. Do not leave a bare `handler.queue()` mutation
in a command file.

- [ ] **Step 2: Build and test**

Run: `cargo check -p crack-core --all-targets && cargo test -p crack-core --lib`
Expected: PASS.

- [ ] **Step 3: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/music/queue.rs crack-core/src/commands/music/{skip,shuffle,remove,clear}.rs
git commit -m "$(cat <<'EOF'
refactor(queue): route command queue mutation through guard-taking helpers

Mutating helpers in music/queue.rs now take a &QueueGuard, and skip, shuffle,
remove and clear acquire one instead of reaching for handler.queue() directly.

A command can no longer mutate the queue without passing the ownership check, so
forgetting a GP_BLOCKED_COMMANDS entry becomes a worse error message rather than
a corrupted /gp round.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 6: The two carve-outs — `/gp` itself and the event handler

The sharp edge. A bug where `/gp` fails to take its own guard is **worse** than
the bug being closed, because it is intermittent rather than deterministic.
Review this task harder than Task 5.

**Files:**
- Modify: `crack-core/src/commands/music/gp.rs` (:2253, :2473, :3003, :3417)
- Modify: `crack-core/src/handlers/track_end.rs` (:138)

**Interfaces:**
- Consumes: `Data::lock_queue`, `PlaybackOwner`, `QueueGuard` (Task 1); helpers
  (Task 4).
- Produces: nothing new.

- [ ] **Step 1: `/gp` locks as the owner**

All four `gp.rs` sites are the owner mutating its own queue, so they pass
`PlaybackOwner::Game`. **No plumbing is needed** — `GpPlayback`
(`gp.rs:2155-2160`) already carries `data: Arc<Data>`, `guild_id` and `call`.

`gp_abort` (~:2253) becomes:

```rust
async fn gp_abort(pb: &GpPlayback, text_channel: GenericChannelId, reason: &str) {
    if pb.data.gp_remove(pb.guild_id).is_none() {
        return;
    }
    tracing::warn!("gp: {reason} in {}, game discarded", pb.guild_id);
    // 🪤 ORDER MATTERS. `gp_remove` above released the lease, so this locks a
    // guild that is now Free -- which is correct and is why `Free` is passed,
    // not `Game`. Locking as `Game` here would refuse.
    match pb.data.lock_queue(pb.guild_id, PlaybackOwner::Free).await {
        Ok(guard) => stop_queue(&guard, &pb.call).await,
        Err(e) => tracing::warn!("gp: could not lock the queue to abort in {}: {e}", pb.guild_id),
    }
```

Apply the same reasoning at each of the other three: **lock as `Game` while the
game still owns playback, as `Free` after it has been released.** State which in
a comment at every site.

- [ ] **Step 2: The event handler**

`track_end.rs:138` pauses from an event handler with no command and no user.
`TrackEndHandler` (`:36-42`) already carries `data`, `guild_id` and `call`, and
`:130` already calls `self.data.gp_remove_if_parked(..)` three lines above.

```rust
        if autopause {
            tracing::trace!("Pausing");
            // Autopause is not a user action, but it mutates the queue, so it
            // takes the guard like everything else. A game that owns playback
            // refuses -- correctly: the game controls its own pausing, and the
            // early return above has already handled the active-game case.
            match self.data.lock_queue(self.guild_id, PlaybackOwner::Free).await {
                Ok(guard) => pause_queue(&guard, &self.call).await,
                Err(e) => tracing::trace!("autopause skipped in {}: {e}", self.guild_id),
            }
        }
```

- [ ] **Step 3: Build and test**

Run: `cargo check -p crack-core --all-targets && cargo test -p crack-core --lib`
Expected: PASS.

- [ ] **Step 4: Confirm no bare mutation remains**

Run:

```bash
grep -rn "\.enqueue(\|modify_queue\|\.dequeue(\|queue()\.\(skip\|stop\|pause\|resume\)()" \
     --include='*.rs' crack-core/src | grep -v "music/queue.rs"
```

Expected: **no output**. Every hit outside `music/queue.rs` is a mutation that
escaped the funnel. If one is genuinely legitimate, say why in the task report
rather than silencing the grep.

- [ ] **Step 5: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/commands/music/gp.rs crack-core/src/handlers/track_end.rs
git commit -m "$(cat <<'EOF'
refactor(gp): take the playback guard at the game's own mutation sites

/gp mutates the queue at four sites and the track-end handler pauses at one.
Both already carry Arc<Data>, guild_id and call, so no plumbing was needed.

Each site states in a comment whether it locks as Game (the game still owns
playback) or as Free (ownership already released) -- getting that backwards
refuses rather than corrupts, but refuses silently, so it is written down.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 7: Close the check-then-act in `queue_query_list_offset`

**Files:**
- Modify: `crack-core/src/music/queue.rs` (:463-511)
- Test: inline in `crack-core/src/music/queue.rs`

**Interfaces:**
- Consumes: `QueueGuard` (Task 1), guard-taking helpers (Task 4).
- Produces: nothing new.

- [ ] **Step 1: Restructure**

Currently the function reads `queue().len()`, drops the lock, validates `offset`
against that number, then crosses `resolve_track_many` (8–15s cold) before
acting. Resolve first, then take the guard, then read *and* act under it:

```rust
    // Resolution is slow (8-15s cold) and touches no shared state, so it happens
    // BEFORE exclusion is taken -- a guard held across it would make a second
    // /play wait it out. See the Global Constraints in the plan.
    let tracks = ctx.data().ct_client.resolve_track_many(queries).await?;

    // From here the queue length is read and acted on under one guard, so the
    // number cannot go stale between the check and the insert.
    let guard = ctx.data().lock_queue(guild_id, PlaybackOwner::Free).await?;
    let mut handler = call.lock().await;
    let queue_size = handler.queue().len();
    if queue_size <= 1 {
        drop(handler);
        return queue_vec_query_type(&guard, ctx, call, queries, Mode::End).await;
    }
    verify(
        offset > 0 && offset <= queue_size + 1,
        CrackedError::NotInRange("index", offset as isize, 1, queue_size as isize),
    )?;
```

⚠️ The early `queue_size <= 1` branch currently happens *before* resolution.
Moving the length read after resolution means resolving even in that case. If
that is unacceptable, keep a pre-resolution read as a fast path but **re-read and
re-validate under the guard** — the pre-read must never be the number acted on.

- [ ] **Step 2: Add the regression comment and test**

```rust
    #[test]
    fn offset_validation_reads_under_the_same_guard_it_acts_under() {
        // Documents the invariant this function got wrong: the queue length was
        // read, the lock dropped, `resolve_track_many` awaited for 8-15s, and
        // then the stale number acted on. Any refactor that reintroduces a read
        // outside the guard reintroduces the race.
        let src = include_str!("queue.rs");
        let body = src
            .split("pub async fn queue_query_list_offset")
            .nth(1)
            .expect("queue_query_list_offset must exist");
        let body = &body[..body.find("\n}\n").expect("function end")];
        let guard_at = body.find("lock_queue").expect("must take a QueueGuard");
        let resolve_at = body.find("resolve_track_many").expect("must resolve");
        assert!(
            resolve_at < guard_at,
            "resolution must happen BEFORE the guard is taken, never under it"
        );
    }
```

- [ ] **Step 3: Run to verify it passes**

Run: `cargo test -p crack-core --lib music::queue`
Expected: PASS.

- [ ] **Step 4: Lint and commit**

```bash
cargo fmt --all
cargo clippy -p crack-core --all-targets
git add crack-core/src/music/queue.rs
git commit -m "$(cat <<'EOF'
fix(queue): validate the insert offset under the guard that acts on it

queue_query_list_offset read queue().len(), dropped the lock, awaited
resolve_track_many for 8-15s, then acted on the number it read before. The
length is now read and acted on under one QueueGuard.

Resolution deliberately stays outside the guard: it touches no shared state, and
holding exclusion across it would make a second /play wait 8-15s.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

### Task 8: Lock ordering, and write the rules down

**Files:**
- Modify: `crack-core/src/poise_ext.rs` (`JoinVCToken::acquire` ~:586)
- Modify: `crack-core/src/music/lease.rs` (doc)
- Test: inline in `crack-core/src/music/lease.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: nothing new.

- [ ] **Step 1: Write the failing test**

```rust
    #[tokio::test]
    async fn the_sanctioned_lock_order_completes() {
        // Lease first, join token second. Two per-guild locks taken in opposite
        // orders is a textbook deadlock; this asserts the sanctioned order is
        // not itself blocking.
        let d = data();
        let guard = d.lock_queue(G, PlaybackOwner::Free).await.unwrap();
        let token = crate::poise_ext::JoinVCToken::acquire(&d, G);
        drop(token);
        drop(guard);
    }
```

- [ ] **Step 2: Add the ordering assert**

In `poise_ext.rs`, in `JoinVCToken::acquire`:

```rust
    pub fn acquire(data: &Data, guild_id: serenity::GuildId) -> Self {
        // 🪤 LOCK ORDER: playback lease first, join token second, NEVER the
        // reverse. Two per-guild locks taken in opposite orders deadlock the
        // shard for that guild. If you are taking a join token you must not
        // subsequently take the playback lease.
        debug_assert!(
            !data.queue_locks.get(&guild_id).is_some_and(|l| l.try_lock().is_err())
                || data.playback_owner(guild_id) != PlaybackOwner::Free,
            "join token acquired while this task may be about to take the playback lease"
        );
```

> If that predicate proves unworkable in practice — it is a heuristic, not a
> proof — replace it with a plain comment plus the doc section, and say so in the
> task report. Do **not** ship an assert that fires spuriously; a `debug_assert`
> that cries wolf gets deleted, and the rule goes with it.

- [ ] **Step 3: Document the rule where both locks are visible**

Extend the `# Lock ordering` section of `music/lease.rs`'s module doc with the
concrete rule, the deadlock it prevents, and a pointer to `join_vc_tokens`.

- [ ] **Step 4: Run the whole suite**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all --check
SQLX_OFFLINE=true cargo check -p crack-core
```

Expected: PASS. `SQLX_OFFLINE` is checked because CI's `Docker` job is skipped on
pull requests and is the only place a bad offline build would otherwise surface.

- [ ] **Step 5: Bump the version**

This is a feature: minor bump, `0.8.0` → `0.9.0`, in **all nine** member
`Cargo.toml` files. A partial bump compiles clean (#424b), so verify:

```bash
grep -H '^version = ' */Cargo.toml | sort
grep -rn '"0\.8\.0"' --include='Cargo.toml' .   # expect no output
```

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add -A
git commit -m "$(cat <<'EOF'
docs(playback): write down the lease/join-token lock order, and bump to 0.9.0

Two per-guild locks taken in opposite orders deadlock the guild's shard. The
order is playback lease first, join token second, never the reverse -- now
carried by a debug_assert and the lease module's docs rather than by memory.

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)
EOF
)"
```

---

## After the tasks

Open the PR against `master` from `feat/playback-ownership-lease`. The body
should explain the two concepts and why they are not merged, cite #434 and
#333, and state plainly what is **not** covered: `leave`, `summon` and
`summonchannel` never touch the queue, so the funnel closes the bug class for
queue mutation (16 of 19 blocked commands) and leaves it intact for voice state
(3 of 19).

Then follow the repo cadence: CI green → review → merge → sync master → tag
`-s v0.9.0` → push the tag. **Do not run `gh release create`** — cargo-dist's
`Release` workflow creates the release from the tag, and running it by hand
races the workflow and produces a release with no assets.

⚠️ Before merging, re-check whether **#461** (splitting `gp.rs`, 5,651 lines,
assigned to ChristianMorton) has started. This plan touches four sites in that
file; a rebase across a split of that size is expensive, and it is cheaper to
coordinate than to resolve.
