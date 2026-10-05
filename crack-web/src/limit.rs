//! A per-user rate limit for dashboard controls, in memory.

use dashmap::DashMap;
use serenity::all::UserId;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Controls one user may make in one window, across all servers.
pub const CONTROLS_PER_WINDOW: usize = 5;
pub const CONTROL_WINDOW: Duration = Duration::from_secs(10);
/// Past this many users on the ledger, a hit first drops every user whose
/// newest hit has left the window.
const SWEEP_AT: usize = 1024;

pub struct RateLimit {
    window: Duration,
    max: usize,
    sweep_at: usize,
    hits: DashMap<UserId, VecDeque<Instant>>,
}

impl RateLimit {
    pub fn new(max: usize, window: Duration) -> Self {
        Self::with_sweep(max, window, SWEEP_AT)
    }

    /// As [`RateLimit::new`], sweeping idle users once more than `sweep_at`
    /// are on the ledger.
    pub fn with_sweep(max: usize, window: Duration, sweep_at: usize) -> Self {
        Self {
            window,
            max,
            sweep_at,
            hits: DashMap::new(),
        }
    }

    /// Count a hit at `now` and say whether it is within the limit. A refused
    /// hit is not counted.
    pub fn allow(&self, user: UserId, now: Instant) -> bool {
        // 🔑 Before this user's entry is taken: `retain` locks every shard,
        // and a held entry would deadlock it.
        if self.hits.len() > self.sweep_at {
            self.hits.retain(|_, d| {
                d.back()
                    .is_some_and(|&t| now.saturating_duration_since(t) <= self.window)
            });
        }
        let mut hits = self.hits.entry(user).or_default();
        while hits
            .front()
            .is_some_and(|&t| now.saturating_duration_since(t) > self.window)
        {
            hits.pop_front();
        }
        if hits.len() >= self.max {
            return false;
        }
        hits.push_back(now);
        true
    }

    /// Users on the ledger.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.hits.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_in_ten_seconds_then_no_and_the_window_slides() {
        let l = RateLimit::new(5, Duration::from_secs(10));
        let (u, t0) = (UserId::new(1), Instant::now());
        for i in 0..5 {
            assert!(l.allow(u, t0 + Duration::from_millis(i)), "hit {i}");
        }
        assert!(
            !l.allow(u, t0 + Duration::from_secs(1)),
            "6th within the window"
        );
        assert!(l.allow(UserId::new(2), t0), "per user");
        assert!(
            l.allow(u, t0 + Duration::from_secs(10) + Duration::from_millis(1)),
            "oldest slid out"
        );
    }

    #[test]
    fn idle_users_are_swept_once_the_ledger_is_large() {
        let window = Duration::from_secs(10);
        let l = RateLimit::with_sweep(5, window, 4);
        let t0 = Instant::now();
        for u in 1..=6 {
            assert!(l.allow(UserId::new(u), t0));
        }
        assert_eq!(l.len(), 6);
        let later = t0 + window + Duration::from_millis(1);
        assert!(l.allow(UserId::new(100), later), "the new user is counted");
        assert_eq!(l.len(), 1, "only the new user is left");
        // And counted: four more fill its window, a sixth is refused.
        for _ in 0..4 {
            assert!(l.allow(UserId::new(100), later));
        }
        assert!(!l.allow(UserId::new(100), later));
    }

    #[test]
    fn a_user_still_in_the_window_survives_a_sweep() {
        let window = Duration::from_secs(10);
        let l = RateLimit::with_sweep(1, window, 2);
        let t0 = Instant::now();
        for u in 1..=3 {
            assert!(l.allow(UserId::new(u), t0));
        }
        assert!(l.allow(UserId::new(9), t0 + window));
        assert_eq!(l.len(), 4, "a hit exactly one window old is still counted");
        assert!(!l.allow(UserId::new(1), t0 + window));
    }

    #[test]
    fn refused_hits_do_not_extend_the_wait() {
        let l = RateLimit::new(1, Duration::from_secs(10));
        let (u, t0) = (UserId::new(1), Instant::now());
        assert!(l.allow(u, t0));
        for s in 1..10 {
            assert!(!l.allow(u, t0 + Duration::from_secs(s)));
        }
        assert!(l.allow(u, t0 + Duration::from_secs(10) + Duration::from_millis(1)));
    }
}
