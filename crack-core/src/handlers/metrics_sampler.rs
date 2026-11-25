//! Health gauges: gateway state from events, and a 15 s sampler of voice calls.

use crack_metrics::{
    gateway_event, gateway_event_received, guild_playback_seconds, guild_voice_connected_seconds,
    set_gateway_connected, set_gateway_latency, set_guilds, set_queued_tracks, set_tracks_playing,
    set_voice_connections, GatewayEvent,
};
use poise::serenity_prelude::Context as SerenityContext;
use songbird::{tracks::PlayMode, Songbird};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SAMPLE_EVERY: Duration = Duration::from_secs(15);

/// A gateway event this recent means the gateway is up.
pub const GATEWAY_FRESH_FOR: Duration = Duration::from_secs(120);

/// Unix seconds of the latest gateway event this process received; 0 before any.
static LAST_GATEWAY_EVENT: AtomicU64 = AtomicU64::new(0);
/// Whether a `Ready` has been seen yet, to tell a fresh session from the first.
static READY_SEEN: AtomicBool = AtomicBool::new(false);

type LatencyReader = Arc<dyn Fn() -> Option<Duration> + Send + Sync>;

/// Reads the latency of the shard runner that most recently said `Ready`. serenity
/// builds a new runner per restart, so this is replaced on every `Ready`.
static LATENCY_SLOT: Mutex<Option<LatencyReader>> = Mutex::new(None);

/// How long the sampler waits on one track's state before treating it as not playing.
const TRACK_STATE_TIMEOUT: Duration = Duration::from_secs(1);

/// What one voice call looks like to the sampler. Built from songbird by `observe`, by hand in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallSample {
    pub guild_id: u64,
    pub connected: bool,
    pub playing: bool,
    pub queued: usize,
}

/// Pure: turns one tick's samples into metric calls. `seconds` is the whole seconds to
/// credit for this tick (see [`SecondsAccumulator`]).
pub fn record(samples: &[CallSample], seconds: u64) {
    set_voice_connections(samples.iter().filter(|s| s.connected).count());
    set_tracks_playing(samples.iter().filter(|s| s.playing).count());
    set_queued_tracks(samples.iter().map(|s| s.queued).sum());
    if seconds == 0 {
        return;
    }
    for s in samples {
        if s.connected {
            guild_voice_connected_seconds(s.guild_id, seconds);
        }
        if s.playing {
            guild_playback_seconds(s.guild_id, seconds);
        }
    }
}

/// Carries the sub-second remainder from one tick to the next, so the seconds recorded
/// track wall time however far each wake-up drifts from the nominal interval.
#[derive(Debug, Default)]
pub struct SecondsAccumulator {
    carry: Duration,
}

impl SecondsAccumulator {
    /// Adds one tick; returns the whole seconds to record now.
    pub fn add(&mut self, tick: Duration) -> u64 {
        let total = self.carry + tick;
        let whole = total.as_secs();
        self.carry = total - Duration::from_secs(whole);
        whole
    }
}

/// Pure: is a gateway event at `last_event_unix` recent enough, as of `now_unix`, to
/// call the gateway connected? A `now` earlier than `last` (clock step) counts as fresh.
pub fn gateway_is_fresh(last_event_unix: u64, now_unix: u64) -> bool {
    last_event_unix != 0 && now_unix.saturating_sub(last_event_unix) <= GATEWAY_FRESH_FOR.as_secs()
}

/// Pure: sets `cracktunes_gateway_connected` from the last event's age.
pub fn record_gateway(last_event_unix: u64, now_unix: u64) {
    set_gateway_connected(gateway_is_fresh(last_event_unix, now_unix));
}

fn unix_seconds(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Called for EVERY gateway event: a few atomic operations, no lock, no allocation.
pub fn note_gateway_event(now: SystemTime) {
    LAST_GATEWAY_EVENT.store(unix_seconds(now), Ordering::Relaxed);
    gateway_event_received(now);
}

/// `Ready`: the gateway is up. A `Ready` after the first is a fresh session, not a resume.
pub fn on_ready() {
    record_ready(!READY_SEEN.swap(true, Ordering::Relaxed));
}

fn record_ready(first: bool) {
    gateway_event(GatewayEvent::Ready);
    if !first {
        gateway_event(GatewayEvent::Reconnect);
    }
    set_gateway_connected(true);
}

/// `Resume`: the session was resumed.
pub fn on_resumed() {
    gateway_event(GatewayEvent::Resumed);
    set_gateway_connected(true);
}

/// Remember where to read gateway latency from. Call on every `Ready`.
pub fn note_latency_source(ctx: &SerenityContext) {
    let info = ctx.runner_info.clone();
    set_latency_source(Arc::new(move || info.read().latency));
}

fn set_latency_source(reader: LatencyReader) {
    if let Ok(mut slot) = LATENCY_SLOT.lock() {
        *slot = Some(reader);
    }
}

/// The lock is held only to clone the reader, never while calling it.
fn current_latency() -> Option<Duration> {
    let reader = LATENCY_SLOT.lock().ok()?.clone()?;
    reader()
}

/// A paused, stopped, ended or errored track is not playing.
pub fn is_playing(mode: &PlayMode) -> bool {
    matches!(mode, PlayMode::Play)
}

/// Reads every voice call songbird manages.
pub async fn observe(songbird: &Songbird) -> Vec<CallSample> {
    let mut samples = Vec::new();
    // Collect first: `iter()` is a dashmap iterator that holds a shard read lock while
    // it lives, and joins/leaves need the write lock. Never keep it across an await.
    let calls: Vec<_> = songbird.iter().collect();
    for (guild_id, call) in calls {
        // Hold the call lock only long enough to copy what we need.
        let (connected, queue) = {
            let call = call.lock().await;
            (call.current_connection().is_some(), call.queue().clone())
        };
        let playing = match queue.current() {
            Some(handle) => {
                match tokio::time::timeout(TRACK_STATE_TIMEOUT, handle.get_info()).await {
                    Ok(Ok(state)) => is_playing(&state.playing),
                    _ => false,
                }
            },
            None => false,
        };
        samples.push(CallSample {
            guild_id: guild_id.get(),
            connected,
            playing,
            queued: queue.len(),
        });
    }
    samples
}

/// Starts the sampler. Call once per process. A supervising task logs if the loop dies.
pub fn spawn(ctx: SerenityContext, songbird: Arc<Songbird>) {
    let sampler = tokio::spawn(async move {
        let mut every = tokio::time::interval(SAMPLE_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        let mut seconds = SecondsAccumulator::default();
        loop {
            every.tick().await;
            let now = Instant::now();
            let whole = seconds.add(now.duration_since(last));
            last = now;
            set_guilds(ctx.cache.guild_count());
            // None until a heartbeat is acked.
            if let Some(latency) = current_latency() {
                set_gateway_latency(latency);
            }
            record_gateway(
                LAST_GATEWAY_EVENT.load(Ordering::Relaxed),
                unix_seconds(SystemTime::now()),
            );
            record(&observe(&songbird).await, whole);
        }
    });
    tokio::spawn(async move {
        match sampler.await {
            Ok(()) => tracing::error!("metrics sampler exited; metrics are no longer updated"),
            Err(e) => tracing::error!("metrics sampler died: {e}; metrics are no longer updated"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crack_metrics::names;
    use crack_metrics::testing::capture;

    fn call(guild_id: u64, connected: bool, playing: bool, queued: usize) -> CallSample {
        CallSample {
            guild_id,
            connected,
            playing,
            queued,
        }
    }

    #[test]
    fn record_sets_gauges_and_advances_time() {
        let samples = [
            call(1, true, true, 3),
            call(2, true, false, 0),
            call(3, false, false, 5),
        ];
        let ((), c) = capture(|| record(&samples, 15));
        assert_eq!(c.gauge(names::VOICE_CONNECTIONS, &[]), Some(2.0));
        assert_eq!(c.gauge(names::TRACKS_PLAYING, &[]), Some(1.0));
        assert_eq!(c.gauge(names::QUEUED_TRACKS, &[]), Some(8.0));
        let connected = |g| c.counter(names::GUILD_VOICE_CONNECTED_SECONDS, &[("guild_id", g)]);
        assert_eq!(connected("1"), Some(15));
        assert_eq!(connected("2"), Some(15));
        assert_eq!(connected("3"), None);
        let playing = |g| c.counter(names::GUILD_PLAYBACK_SECONDS, &[("guild_id", g)]);
        assert_eq!(playing("1"), Some(15));
        assert_eq!(playing("2"), None);
        assert_eq!(playing("3"), None);
    }

    #[test]
    fn record_with_no_calls_sets_zeroes() {
        let ((), c) = capture(|| record(&[], 15));
        assert_eq!(c.gauge(names::VOICE_CONNECTIONS, &[]), Some(0.0));
        assert_eq!(c.gauge(names::TRACKS_PLAYING, &[]), Some(0.0));
        assert_eq!(c.gauge(names::QUEUED_TRACKS, &[]), Some(0.0));
        assert_eq!(c.series().len(), 3);
    }

    #[test]
    fn a_sub_second_tick_adds_nothing() {
        let samples = [call(1, true, true, 1)];
        let ((), c) = capture(|| record(&samples, 0));
        assert_eq!(c.series().len(), 3, "only the three gauges");
        assert_eq!(c.gauge(names::QUEUED_TRACKS, &[]), Some(1.0));
    }

    #[test]
    fn only_play_counts_as_playing() {
        assert!(is_playing(&PlayMode::Play));
        assert!(!is_playing(&PlayMode::Pause));
        assert!(!is_playing(&PlayMode::Stop));
        assert!(!is_playing(&PlayMode::End));
        let err = symphonia::core::errors::Error::Unsupported("test");
        assert!(!is_playing(&PlayMode::Errored(
            songbird::tracks::PlayError::Seek(std::sync::Arc::new(err))
        )));
    }

    #[test]
    fn gateway_freshness_table() {
        let cases = [
            (0, 1_000, false, "never received"),
            (1_000, 1_000, true, "just now"),
            (1_000, 1_119, true, "119 s"),
            (1_000, 1_120, true, "120 s"),
            (1_000, 1_121, false, "121 s"),
            (1_000, 900, true, "clock stepped back"),
        ];
        for (last, now, want, why) in cases {
            assert_eq!(gateway_is_fresh(last, now), want, "{why}");
        }
    }

    #[test]
    fn a_stale_stamp_zeroes_the_gauge_and_a_fresh_one_sets_it() {
        let ((), c) = capture(|| record_gateway(1_000, 1_500));
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(0.0));
        let ((), c) = capture(|| record_gateway(1_000, 1_030));
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(1.0));
        let ((), c) = capture(|| record_gateway(0, 1_030));
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(0.0));
    }

    #[test]
    fn noting_an_event_stamps_the_atomic_and_the_gauge() {
        let at = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
        let ((), c) = capture(|| note_gateway_event(at));
        assert_eq!(LAST_GATEWAY_EVENT.load(Ordering::Relaxed), 1_700_000_123);
        assert_eq!(
            c.gauge(names::GATEWAY_LAST_EVENT, &[]),
            Some(1_700_000_123.0)
        );
    }

    #[test]
    fn the_first_ready_is_not_a_reconnect_a_later_one_is() {
        let count = |c: &crack_metrics::testing::Captured, e| {
            c.counter(names::GATEWAY_EVENTS, &[("event", e)])
        };
        let ((), c) = capture(|| record_ready(true));
        assert_eq!(count(&c, "ready"), Some(1));
        assert_eq!(count(&c, "reconnect"), None);
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(1.0));
        let ((), c) = capture(|| record_ready(false));
        assert_eq!(count(&c, "ready"), Some(1));
        assert_eq!(count(&c, "reconnect"), Some(1));
    }

    #[test]
    fn on_ready_tells_first_from_later_by_process_state() {
        let ((), c) = capture(|| {
            on_ready();
            on_ready();
        });
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "ready")]),
            Some(2)
        );
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "reconnect")]),
            Some(1)
        );
    }

    #[test]
    fn resume_counts_resumed_and_sets_connected() {
        let ((), c) = capture(on_resumed);
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(1.0));
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "resumed")]),
            Some(1)
        );
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "reconnect")]),
            None
        );
    }

    #[test]
    fn the_latency_slot_follows_the_latest_ready() {
        assert_eq!(current_latency(), None);
        set_latency_source(Arc::new(|| Some(Duration::from_millis(40))));
        assert_eq!(current_latency(), Some(Duration::from_millis(40)));
        set_latency_source(Arc::new(|| Some(Duration::from_millis(90))));
        assert_eq!(current_latency(), Some(Duration::from_millis(90)));
    }

    #[test]
    fn seconds_carry_between_ticks() {
        let mut a = SecondsAccumulator::default();
        assert_eq!(a.add(Duration::from_millis(14_999)), 14);
        assert_eq!(a.add(Duration::from_millis(15_001)), 16);
        let mut a = SecondsAccumulator::default();
        let got: Vec<u64> = (0..5).map(|_| a.add(Duration::from_millis(400))).collect();
        assert_eq!(got, [0, 0, 1, 0, 1]);
        assert_eq!(
            SecondsAccumulator::default().add(Duration::from_millis(400)),
            0
        );
    }
}
