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

pub struct RateLimit {
    window: Duration,
    max: usize,
    hits: DashMap<UserId, VecDeque<Instant>>,
}

impl RateLimit {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            window,
            max,
            hits: DashMap::new(),
        }
    }

    /// Count a hit at `now` and say whether it is within the limit. A refused
    /// hit is not counted.
    pub fn allow(&self, user: UserId, now: Instant) -> bool {
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
