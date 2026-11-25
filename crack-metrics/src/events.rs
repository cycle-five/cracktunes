//! The event functions the rest of the bot calls. Each returns `()`: a metrics
//! failure never reaches a command. With no recorder installed they do nothing.

use crate::labels::{
    CommandKind, EndOutcome, FailureReason, GatewayEvent, ProxyOutcome, RequestOutcome,
    ResolveOutcome, SearchFallbackOutcome, SearchFallbackReason, Source, SuggestionLabelOutcome,
};
use crate::names;
use metrics::{counter, gauge, histogram};
use std::time::Duration;

pub fn command_started(command: &str, kind: CommandKind, guild_id: Option<u64>) {
    counter!(names::COMMANDS, "command" => command.to_owned(), "kind" => kind.as_str())
        .increment(1);
    if let Some(guild_id) = guild_id {
        counter!(names::GUILD_COMMANDS, "guild_id" => guild_id.to_string()).increment(1);
    }
}

pub fn command_finished(command: &str, elapsed: Duration) {
    histogram!(names::COMMAND_DURATION, "command" => command.to_owned())
        .record(elapsed.as_secs_f64());
}

pub fn command_failed(command: &str) {
    counter!(names::COMMAND_ERRORS, "command" => command.to_owned()).increment(1);
}

pub fn autocomplete_finished(elapsed: Duration) {
    histogram!(names::AUTOCOMPLETE_DURATION).record(elapsed.as_secs_f64());
}

pub fn gateway_event(event: GatewayEvent) {
    counter!(names::GATEWAY_EVENTS, "event" => event.as_str()).increment(1);
}

pub fn set_gateway_connected(connected: bool) {
    gauge!(names::GATEWAY_CONNECTED).set(if connected { 1.0 } else { 0.0 });
}

pub fn set_gateway_latency(latency: Duration) {
    gauge!(names::GATEWAY_LATENCY).set(latency.as_secs_f64());
}

pub fn set_guilds(count: usize) {
    gauge!(names::GUILDS).set(count as f64);
}

pub fn set_guild_info(guild_id: u64, name: &str) {
    gauge!(names::GUILD_INFO, "guild_id" => guild_id.to_string(), "name" => name.to_owned())
        .set(1.0);
}

pub fn set_voice_connections(count: usize) {
    gauge!(names::VOICE_CONNECTIONS).set(count as f64);
}

pub fn set_tracks_playing(count: usize) {
    gauge!(names::TRACKS_PLAYING).set(count as f64);
}

pub fn set_queued_tracks(count: usize) {
    gauge!(names::QUEUED_TRACKS).set(count as f64);
}

pub fn track_resolved(source: Source, outcome: ResolveOutcome, elapsed: Duration) {
    counter!(names::TRACK_RESOLVES, "source" => source.as_str(), "outcome" => outcome.as_str())
        .increment(1);
    histogram!(names::TRACK_RESOLVE_DURATION, "source" => source.as_str())
        .record(elapsed.as_secs_f64());
}

pub fn track_failed(source: Source, reason: FailureReason) {
    counter!(names::TRACK_FAILURES, "source" => source.as_str(), "reason" => reason.as_str())
        .increment(1);
}

pub fn track_started(source: Source, guild_id: u64) {
    counter!(names::TRACKS_STARTED, "source" => source.as_str()).increment(1);
    counter!(names::GUILD_TRACKS_STARTED, "guild_id" => guild_id.to_string()).increment(1);
}

pub fn track_ended(outcome: EndOutcome) {
    counter!(names::TRACKS_ENDED, "outcome" => outcome.as_str()).increment(1);
}

pub fn guild_playback_seconds(guild_id: u64, seconds: u64) {
    counter!(names::GUILD_PLAYBACK_SECONDS, "guild_id" => guild_id.to_string()).increment(seconds);
}

pub fn guild_voice_connected_seconds(guild_id: u64, seconds: u64) {
    counter!(names::GUILD_VOICE_CONNECTED_SECONDS, "guild_id" => guild_id.to_string())
        .increment(seconds);
}

/// Counts once per gated track, and adds the number of residential sessions tried.
pub fn proxy_fallback(outcome: ProxyOutcome, attempts: u64) {
    counter!(names::PROXY_FALLBACK, "outcome" => outcome.as_str()).increment(1);
    counter!(names::PROXY_FALLBACK_ATTEMPTS).increment(attempts);
}

pub fn search_fallback(reason: SearchFallbackReason, outcome: SearchFallbackOutcome) {
    counter!(names::SEARCH_FALLBACK, "reason" => reason.as_str(), "outcome" => outcome.as_str())
        .increment(1);
}

pub fn sleevenote_request(outcome: RequestOutcome, elapsed: Duration) {
    counter!(names::SLEEVENOTE_REQUESTS, "outcome" => outcome.as_str()).increment(1);
    histogram!(names::SLEEVENOTE_REQUEST_DURATION).record(elapsed.as_secs_f64());
}

pub fn suggestion_label(outcome: SuggestionLabelOutcome) {
    counter!(names::SUGGESTION_LABEL, "outcome" => outcome.as_str()).increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
    use metrics_util::CompositeKey;
    use std::collections::BTreeSet;

    type Row = (
        CompositeKey,
        Option<metrics::Unit>,
        Option<metrics::SharedString>,
        DebugValue,
    );

    fn record(f: impl FnOnce()) -> Vec<Row> {
        let recorder = DebuggingRecorder::new();
        let snapshotter: Snapshotter = recorder.snapshotter();
        metrics::with_local_recorder(&recorder, f);
        snapshotter.snapshot().into_vec()
    }

    fn labels(row: &Row) -> Vec<(String, String)> {
        row.0
            .key()
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect()
    }

    /// The counter named `name` whose labels are exactly `want`.
    fn counter_value(rows: &[Row], name: &str, want: &[(&str, &str)]) -> Option<u64> {
        let want: Vec<(String, String)> = want
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        rows.iter().find_map(|r| match &r.3 {
            DebugValue::Counter(v) if r.0.key().name() == name && labels(r) == want => Some(*v),
            _ => None,
        })
    }

    fn series_named(rows: &[Row], name: &str) -> usize {
        rows.iter().filter(|r| r.0.key().name() == name).count()
    }

    #[test]
    fn command_started_counts_by_name_and_by_guild() {
        let rows = record(|| {
            command_started("play", CommandKind::Slash, Some(42));
            command_started("ping", CommandKind::Prefix, None);
        });
        assert_eq!(
            counter_value(
                &rows,
                names::COMMANDS,
                &[("command", "play"), ("kind", "slash")]
            ),
            Some(1)
        );
        assert_eq!(
            counter_value(&rows, names::GUILD_COMMANDS, &[("guild_id", "42")]),
            Some(1)
        );
        assert_eq!(
            counter_value(
                &rows,
                names::COMMANDS,
                &[("command", "ping"), ("kind", "prefix")]
            ),
            Some(1)
        );
        assert_eq!(
            series_named(&rows, names::GUILD_COMMANDS),
            1,
            "no guild series for the guildless command"
        );
    }

    #[test]
    fn proxy_fallback_counts_once_and_adds_attempts() {
        let rows = record(|| proxy_fallback(ProxyOutcome::Recovered, 2));
        assert_eq!(
            counter_value(&rows, names::PROXY_FALLBACK, &[("outcome", "recovered")]),
            Some(1)
        );
        assert_eq!(
            counter_value(&rows, names::PROXY_FALLBACK_ATTEMPTS, &[]),
            Some(2)
        );
    }

    #[test]
    fn track_started_counts_by_source_and_by_guild() {
        let rows = record(|| track_started(Source::Spotify, 42));
        assert_eq!(
            counter_value(&rows, names::TRACKS_STARTED, &[("source", "spotify")]),
            Some(1)
        );
        assert_eq!(
            counter_value(&rows, names::GUILD_TRACKS_STARTED, &[("guild_id", "42")]),
            Some(1)
        );
    }

    #[test]
    fn gauges_and_histograms_record_their_values() {
        let rows = record(|| {
            set_gateway_connected(true);
            set_guilds(155);
            command_finished("play", Duration::from_millis(1500));
            guild_playback_seconds(42, 30);
            track_resolved(Source::Youtube, ResolveOutcome::Ok, Duration::from_secs(2));
        });
        let gauge = |name: &str| {
            rows.iter().find_map(|r| match &r.3 {
                DebugValue::Gauge(v) if r.0.key().name() == name => Some(v.into_inner()),
                _ => None,
            })
        };
        assert_eq!(gauge(names::GATEWAY_CONNECTED), Some(1.0));
        assert_eq!(gauge(names::GUILDS), Some(155.0));
        assert_eq!(
            counter_value(&rows, names::GUILD_PLAYBACK_SECONDS, &[("guild_id", "42")]),
            Some(30)
        );
        assert_eq!(
            counter_value(
                &rows,
                names::TRACK_RESOLVES,
                &[("source", "youtube"), ("outcome", "ok")]
            ),
            Some(1)
        );
        let hist = rows.iter().find_map(|r| match &r.3 {
            DebugValue::Histogram(v) if r.0.key().name() == names::COMMAND_DURATION => {
                Some(v.iter().map(|x| x.into_inner()).collect::<Vec<_>>())
            },
            _ => None,
        });
        assert_eq!(hist, Some(vec![1.5]));
    }

    #[test]
    fn no_event_function_emits_a_label_outside_the_allowed_set() {
        let rows = record(|| {
            command_started("play", CommandKind::Slash, Some(1));
            command_finished("play", Duration::from_secs(1));
            command_failed("play");
            autocomplete_finished(Duration::from_secs(1));
            gateway_event(GatewayEvent::Ready);
            set_gateway_connected(true);
            set_gateway_latency(Duration::from_millis(40));
            set_guilds(1);
            set_guild_info(1, "g");
            set_voice_connections(1);
            set_tracks_playing(1);
            set_queued_tracks(1);
            track_resolved(Source::Other, ResolveOutcome::Error, Duration::from_secs(1));
            track_failed(Source::Other, FailureReason::Other);
            track_started(Source::Other, 1);
            track_ended(EndOutcome::Finished);
            guild_playback_seconds(1, 1);
            guild_voice_connected_seconds(1, 1);
            proxy_fallback(ProxyOutcome::Error, 1);
            search_fallback(SearchFallbackReason::Empty, SearchFallbackOutcome::Found);
            sleevenote_request(RequestOutcome::Ok, Duration::from_secs(1));
            suggestion_label(SuggestionLabelOutcome::Stripped);
        });
        let allowed: BTreeSet<&str> = [
            "command", "kind", "event", "source", "outcome", "reason", "guild_id", "name",
            "version",
        ]
        .into_iter()
        .collect();
        assert!(
            rows.len() >= 20,
            "every event function should have recorded something"
        );
        for row in &rows {
            for (k, _) in labels(row) {
                assert!(
                    allowed.contains(k.as_str()),
                    "{}: label key {k:?} not allowed",
                    row.0.key().name()
                );
            }
        }
    }

    #[test]
    fn events_without_a_recorder_do_nothing() {
        // No recorder is installed in this process: the calls are no-ops.
        command_started("play", CommandKind::Slash, Some(1));
        set_guilds(3);
        track_ended(EndOutcome::Skipped);
        proxy_fallback(ProxyOutcome::Exhausted, 5);
    }
}
