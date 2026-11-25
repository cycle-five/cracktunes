//! Health gauges: gateway state from events, and a 15 s sampler of voice calls.

use crack_metrics::{
    gateway_event, guild_playback_seconds, guild_voice_connected_seconds, set_gateway_connected,
    set_gateway_latency, set_guilds, set_queued_tracks, set_tracks_playing, set_voice_connections,
    GatewayEvent,
};
use poise::serenity_prelude::Context as SerenityContext;
use serenity::model::gateway::ConnectionStage;
use songbird::{tracks::PlayMode, Songbird};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const SAMPLE_EVERY: Duration = Duration::from_secs(15);

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

/// Pure: turns one tick's samples into metric calls. `tick` is the time since the previous tick.
pub fn record(samples: &[CallSample], tick: Duration) {
    set_voice_connections(samples.iter().filter(|s| s.connected).count());
    set_tracks_playing(samples.iter().filter(|s| s.playing).count());
    set_queued_tracks(samples.iter().map(|s| s.queued).sum());
    let seconds = tick.as_secs();
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

/// Only the `Connected` stage means the gateway is up.
pub fn is_connected(stage: ConnectionStage) -> bool {
    matches!(stage, ConnectionStage::Connected)
}

/// `Ready`: the gateway is up.
pub fn on_ready() {
    gateway_event(GatewayEvent::Ready);
    set_gateway_connected(true);
}

/// `Resume`: the session was resumed.
pub fn on_resumed() {
    gateway_event(GatewayEvent::Resumed);
    set_gateway_connected(true);
}

/// A shard changed stage; a move away from `Connected` is a reconnect.
pub fn on_stage_update(old: ConnectionStage, new: ConnectionStage) {
    set_gateway_connected(is_connected(new));
    if is_connected(old) && !is_connected(new) {
        gateway_event(GatewayEvent::Reconnect);
    }
}

/// Reads every voice call songbird manages.
pub async fn observe(songbird: &Songbird) -> Vec<CallSample> {
    let mut samples = Vec::new();
    for (guild_id, call) in songbird.iter() {
        // Hold the call lock only long enough to copy what we need.
        let (connected, queue) = {
            let call = call.lock().await;
            (call.current_connection().is_some(), call.queue().clone())
        };
        let playing = match queue.current() {
            Some(handle) => {
                match tokio::time::timeout(TRACK_STATE_TIMEOUT, handle.get_info()).await {
                    Ok(Ok(state)) => matches!(state.playing, PlayMode::Play),
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

/// Starts the sampler. Call once per process.
pub fn spawn(ctx: SerenityContext, songbird: Arc<Songbird>) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(SAMPLE_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = Instant::now();
        loop {
            every.tick().await;
            let now = Instant::now();
            let tick = now.duration_since(last);
            last = now;
            set_guilds(ctx.cache.guild_count());
            // The latency of the shard this context belongs to; None until a heartbeat is acked.
            let latency = ctx.runner_info.read().latency;
            if let Some(latency) = latency {
                set_gateway_latency(latency);
            }
            record(&observe(&songbird).await, tick);
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
        let ((), c) = capture(|| record(&samples, Duration::from_secs(15)));
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
        let ((), c) = capture(|| record(&[], Duration::from_secs(15)));
        assert_eq!(c.gauge(names::VOICE_CONNECTIONS, &[]), Some(0.0));
        assert_eq!(c.gauge(names::TRACKS_PLAYING, &[]), Some(0.0));
        assert_eq!(c.gauge(names::QUEUED_TRACKS, &[]), Some(0.0));
        assert_eq!(c.series().len(), 3);
    }

    #[test]
    fn a_sub_second_tick_adds_nothing() {
        let samples = [call(1, true, true, 1)];
        let ((), c) = capture(|| record(&samples, Duration::from_millis(400)));
        assert_eq!(c.series().len(), 3, "only the three gauges");
        assert_eq!(c.gauge(names::QUEUED_TRACKS, &[]), Some(1.0));
    }

    #[test]
    fn stage_to_connected_flag() {
        assert!(is_connected(ConnectionStage::Connected));
        for stage in [
            ConnectionStage::Disconnected,
            ConnectionStage::Handshake,
            ConnectionStage::Identifying,
            ConnectionStage::Resuming,
            ConnectionStage::Connecting,
        ] {
            assert!(!is_connected(stage), "{stage:?}");
        }
    }

    #[test]
    fn leaving_connected_counts_a_reconnect_and_clears_the_flag() {
        let ((), c) =
            capture(|| on_stage_update(ConnectionStage::Connected, ConnectionStage::Disconnected));
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(0.0));
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "reconnect")]),
            Some(1)
        );
    }

    #[test]
    fn moves_between_unconnected_stages_do_not_count_and_connecting_sets_the_flag() {
        let ((), c) = capture(|| {
            on_stage_update(ConnectionStage::Disconnected, ConnectionStage::Handshake);
            on_stage_update(ConnectionStage::Resuming, ConnectionStage::Connected);
        });
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(1.0));
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "reconnect")]),
            None
        );
    }

    #[test]
    fn ready_and_resume_count_their_event_and_set_connected() {
        let ((), c) = capture(|| {
            on_ready();
            on_resumed();
        });
        assert_eq!(c.gauge(names::GATEWAY_CONNECTED, &[]), Some(1.0));
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "ready")]),
            Some(1)
        );
        assert_eq!(
            c.counter(names::GATEWAY_EVENTS, &[("event", "resumed")]),
            Some(1)
        );
    }
}
