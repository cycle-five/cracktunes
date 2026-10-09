//! A per-key minimum gap between accepted events: the now-playing buttons'
//! press debounce.

use dashmap::{mapref::entry::Entry, DashMap};
use std::hash::Hash;
use std::time::{Duration, Instant};

/// Past this many keys, `allow` first drops the stamps whose window has
/// passed, so the map stays about as large as the set of people pressing
/// right now.
const PRUNE_ABOVE: usize = 1024;

/// Accepts at most one event per key per `window`, measured from the last
/// *accepted* one: a refused event does not extend the window. In memory
/// only; a restart forgets every stamp.
#[derive(Debug, Clone)]
pub struct Throttle<K: Eq + Hash> {
    window: Duration,
    last: DashMap<K, Instant>,
}

impl<K: Eq + Hash> Throttle<K> {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last: DashMap::new(),
        }
    }

    /// Whether an event for `key` at `now` is accepted, stamping it if so.
    /// One atomic step on the key's entry: two events arriving together
    /// cannot both get through.
    pub fn allow(&self, key: K, now: Instant) -> bool {
        if self.last.len() > PRUNE_ABOVE {
            self.last
                .retain(|_, at| now.saturating_duration_since(*at) < self.window);
        }
        match self.last.entry(key) {
            Entry::Occupied(mut stamp) => {
                if now.saturating_duration_since(*stamp.get()) >= self.window {
                    stamp.insert(now);
                    true
                } else {
                    false
                }
            },
            Entry::Vacant(slot) => {
                slot.insert(now);
                true
            },
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.last.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const W: Duration = Duration::from_secs(2);
    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn the_first_press_is_allowed() {
        let t = Throttle::new(W);
        assert!(t.allow(1, Instant::now()));
    }

    #[test]
    fn a_press_inside_the_window_is_refused_and_one_at_its_end_is_allowed() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0 + W - MS));
        assert!(t.allow(1, t0 + W));
    }

    /// Fixed from the last accepted press: a masher gets one through every
    /// window. A sliding window would refuse the press at `t0 + W`.
    #[test]
    fn a_refused_press_does_not_extend_the_window() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0 + W / 2));
        assert!(!t.allow(1, t0 + W - MS));
        assert!(t.allow(1, t0 + W));
    }

    #[test]
    fn two_presses_at_the_same_instant_let_one_through() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow(1, t0));
        assert!(!t.allow(1, t0));
    }

    #[test]
    fn keys_are_independent() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        assert!(t.allow((10, 1), t0));
        assert!(t.allow((10, 2), t0), "another user");
        assert!(t.allow((20, 1), t0), "another server");
        assert!(!t.allow((10, 1), t0));
    }

    /// Past `PRUNE_ABOVE` keys, a press first drops every stamp whose window
    /// has passed; stamps still inside their window stay and still refuse.
    #[test]
    fn a_full_map_drops_only_stale_stamps() {
        let t = Throttle::new(W);
        let t0 = Instant::now();
        for k in 0..=PRUNE_ABOVE {
            assert!(t.allow(k, t0));
        }
        // Inside the window nothing is stale: all kept, the old ones still refuse.
        assert!(t.allow(PRUNE_ABOVE + 1, t0 + MS));
        assert_eq!(t.len(), PRUNE_ABOVE + 2);
        assert!(!t.allow(0, t0 + MS));
        // At the window every stamp from t0 is stale and goes.
        assert!(t.allow(PRUNE_ABOVE + 2, t0 + W));
        assert_eq!(
            t.len(),
            2,
            "only the stamps from t0 + 1ms and t0 + W remain"
        );
    }
}
