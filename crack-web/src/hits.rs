//! Each member's last search results, per server, for a short while: an add
//! names a hit by id, so the browser never hands the server a URL.

use crack_core::music::search::SavedTrack;
use dashmap::DashMap;
use serenity::all::{GuildId, UserId};
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
    entries: DashMap<(UserId, GuildId), (Instant, HashMap<Uuid, SavedTrack>)>,
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

    /// Replace `user`'s hits in `guild` with `tracks`; their ids, in order.
    pub fn store(
        &self,
        user: UserId,
        guild: GuildId,
        tracks: Vec<SavedTrack>,
        now: Instant,
    ) -> Vec<Uuid> {
        // 🔑 Before this entry is taken: `retain` locks every shard.
        if self.entries.len() > self.sweep_at {
            self.entries
                .retain(|_, (at, _)| now.saturating_duration_since(*at) < self.ttl);
        }
        let mut ids = Vec::with_capacity(tracks.len());
        let mut map = HashMap::with_capacity(tracks.len());
        for t in tracks {
            let id = Uuid::new_v4();
            ids.push(id);
            map.insert(id, t);
        }
        self.entries.insert((user, guild), (now, map));
        ids
    }

    /// The hit `id` from `user`'s last search in `guild`, if it is still fresh.
    pub fn get(&self, user: UserId, guild: GuildId, id: Uuid, now: Instant) -> Option<SavedTrack> {
        let entry = self.entries.get(&(user, guild))?;
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

    fn t(n: u8) -> SavedTrack {
        SavedTrack {
            url: format!("https://y/{n}"),
            ..Default::default()
        }
    }
    const U: UserId = UserId::new(1);
    const G: GuildId = GuildId::new(5);

    #[test]
    fn a_hit_resolves_for_its_user_and_guild_only() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let ids = memo.store(U, G, vec![t(1), t(2)], now);
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(memo.get(U, G, ids[1], now), Some(t(2)));
        assert_eq!(
            memo.get(UserId::new(2), G, ids[1], now),
            None,
            "another user"
        );
        assert_eq!(
            memo.get(U, GuildId::new(6), ids[1], now),
            None,
            "another guild"
        );
        assert_eq!(
            memo.get(U, G, ids[1], now),
            Some(t(2)),
            "a lookup does not consume it"
        );
    }

    #[test]
    fn a_new_search_replaces_the_old_hits_and_they_expire() {
        let memo = HitMemo::new(HIT_TTL);
        let now = Instant::now();
        let old = memo.store(U, G, vec![t(1)], now);
        let new = memo.store(U, G, vec![t(3)], now);
        assert_eq!(memo.get(U, G, old[0], now), None);
        assert_eq!(memo.get(U, G, new[0], now), Some(t(3)));
        assert_eq!(memo.get(U, G, new[0], now + HIT_TTL), None);
    }

    #[test]
    fn past_the_sweep_size_expired_entries_are_dropped() {
        let memo = HitMemo::with_sweep(HIT_TTL, 2);
        let now = Instant::now();
        for u in 1..=3 {
            memo.store(UserId::new(u), G, vec![t(1)], now);
        }
        memo.store(UserId::new(9), G, vec![t(1)], now + HIT_TTL);
        assert_eq!(memo.len(), 1);
    }
}
