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
        d.claim_playback(G, PlaybackOwner::Game)
            .expect("free guild accepts a claim");
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
        d.claim_playback(G, PlaybackOwner::Game)
            .expect("same owner may re-claim");
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
        let guard = d
            .lock_queue(G, PlaybackOwner::Free)
            .await
            .expect("free guild locks");
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
        d.lock_queue(G, PlaybackOwner::Game)
            .await
            .expect("the owner may lock");
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
