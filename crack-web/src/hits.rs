//! Each member's last search results, per server, for a short while: an add
//! names a hit by id, so the browser never hands the server a URL.

use crack_core::music::search::SavedTrack;
use crack_model::{CommunityRef, UserRef};
use dashmap::DashMap;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// How long a search's hits can be added.
pub const HIT_TTL: Duration = Duration::from_secs(10 * 60);
/// Past this many entries, a store first drops the expired ones.
const SWEEP_AT: usize = 1024;

pub struct HitMemo {
    ttl: Duration,
    sweep_at: usize,
    entries: DashMap<(UserRef, CommunityRef), (Instant, HashMap<Uuid, SavedTrack>)>,
}

impl HitMemo {
    pub fn new(ttl: Duration) -> Self {
        Self::with_sweep(ttl, SWEEP_AT)
    }

    pub fn with_sweep(ttl: Duration, sweep_at: usize) -> Self {
        Self {
            ttl,
            sweep_at,
            entries: DashMap::new(),
        }
    }

    /// Replace `user`'s hits in `guild` with `hits`, each under the id the
    /// caller minted for it (so an id and the view built with it cannot drift).
    pub fn store(
        &self,
        user: &UserRef,
        guild: &CommunityRef,
        hits: Vec<(Uuid, SavedTrack)>,
        now: Instant,
    ) {
        // 🔑 Before this entry is taken: `retain` locks every shard.
        if self.entries.len() > self.sweep_at {
            self.entries
                .retain(|_, (at, _)| now.saturating_duration_since(*at) < self.ttl);
        }
        self.entries.insert(
            (user.clone(), guild.clone()),
            (now, hits.into_iter().collect()),
        );
    }

    /// The hit `id` from `user`'s last search in `guild`, if it is still fresh.
    pub fn get(
        &self,
        user: &UserRef,
        guild: &CommunityRef,
        id: Uuid,
        now: Instant,
    ) -> Option<SavedTrack> {
        let entry = self.entries.get(&(user.clone(), guild.clone()))?;
        let (at, hits) = &*entry;
        if now.saturating_duration_since(*at) >= self.ttl {
            return None;
        }
        hits.get(&id).cloned()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_support::{guild, user};

    fn t(n: u8) -> SavedTrack {
        SavedTrack {
            url: format!("https://y/{n}"),
            ..Default::default()
        }
    }
    /// `tracks` under fresh ids, as `answer_search` mints them.
    fn minted(tracks: Vec<SavedTrack>) -> Vec<(Uuid, SavedTrack)> {
        tracks.into_iter().map(|t| (Uuid::new_v4(), t)).collect()
    }
    fn u() -> UserRef {
        user(1)
    }
    fn g() -> CommunityRef {
        guild(5)
    }

    #[test]
    fn a_hit_resolves_for_its_user_and_guild_only() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let pairs = minted(vec![t(1), t(2)]);
        let ids: Vec<Uuid> = pairs.iter().map(|(id, _)| *id).collect();
        memo.store(&u(), &g(), pairs, now);
        assert_eq!(memo.get(&u(), &g(), ids[1], now), Some(t(2)));
        assert_eq!(memo.get(&user(2), &g(), ids[1], now), None, "another user");
        assert_eq!(
            memo.get(&u(), &guild(6), ids[1], now),
            None,
            "another guild"
        );
        assert_eq!(
            memo.get(&u(), &g(), ids[1], now),
            Some(t(2)),
            "a lookup does not consume it"
        );
    }

    #[test]
    fn a_new_search_replaces_the_old_hits_and_they_expire() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let (old, new) = (minted(vec![t(1)]), minted(vec![t(3)]));
        let (old, new) = ([old[0].0], [new[0].0]);
        memo.store(&u(), &g(), vec![(old[0], t(1))], now);
        memo.store(&u(), &g(), vec![(new[0], t(3))], now);
        assert_eq!(memo.get(&u(), &g(), old[0], now), None);
        assert_eq!(memo.get(&u(), &g(), new[0], now), Some(t(3)));
        assert_eq!(memo.get(&u(), &g(), new[0], now + HIT_TTL), None);
    }

    #[test]
    fn past_the_sweep_size_expired_entries_are_dropped() {
        let memo = HitMemo::with_sweep(HIT_TTL, 2);
        let now = Instant::now();
        for u in 1..=3 {
            memo.store(&user(u), &g(), minted(vec![t(1)]), now);
        }
        memo.store(&user(9), &g(), minted(vec![t(1)]), now + HIT_TTL);
        assert_eq!(memo.len(), 1);
    }
}
