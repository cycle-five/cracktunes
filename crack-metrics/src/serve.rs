//! The `/metrics` listener: a Prometheus recorder plus a tiny axum server on its own port.

use crate::names;
use axum::{extract::State, routing::get, Router};
use metrics::{describe_counter, describe_gauge, describe_histogram, gauge, Unit};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use metrics_process::Collector;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub const BIND_ENV: &str = "METRICS_BIND";

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("not a socket address (want host:port, e.g. 0.0.0.0:9091): {0:?}")]
    BadAddress(String),
    #[error("could not bind the metrics listener: {0}")]
    Bind(std::io::Error),
    #[error("a metrics recorder is already installed in this process")]
    AlreadyInstalled,
}

#[derive(Clone)]
struct Scrape {
    handle: PrometheusHandle,
    process: Collector,
}

/// Reads `METRICS_BIND` and starts the listener if it is set. Never panics, never
/// returns an error: a listener that cannot start logs one ERROR line and the bot
/// carries on. Must be called inside a tokio runtime.
pub fn spawn_if_configured(version: &'static str) {
    let bind = match std::env::var(BIND_ENV) {
        Ok(bind) if !bind.trim().is_empty() => bind,
        _ => {
            tracing::info!("metrics off");
            return;
        },
    };
    tokio::spawn(async move {
        match serve(bind.trim(), version).await {
            Ok(addr) => tracing::info!("metrics on {addr}"),
            Err(e) => tracing::error!("metrics off: {BIND_ENV}={bind}: {e}"),
        }
    });
}

/// The testable core: the address actually bound, or the reason metrics are off.
/// The address is parsed and bound before the global recorder is installed, so a
/// bad or taken address leaves the process untouched.
pub async fn serve(bind: &str, version: &'static str) -> Result<SocketAddr, ServeError> {
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| ServeError::BadAddress(bind.to_owned()))?;
    let listener = TcpListener::bind(addr).await.map_err(ServeError::Bind)?;
    let local = listener.local_addr().map_err(ServeError::Bind)?;

    let handle = install_recorder()?;
    describe_all();
    gauge!(names::BUILD_INFO, "version" => version).set(1.0);
    for zeroed in [
        names::GATEWAY_CONNECTED,
        names::VOICE_CONNECTIONS,
        names::TRACKS_PLAYING,
        names::QUEUED_TRACKS,
    ] {
        gauge!(zeroed).set(0.0);
    }
    let process = Collector::default();
    process.describe();

    let app = Router::new()
        .route("/metrics", get(scrape))
        .with_state(Scrape { handle, process });
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("metrics listener stopped: {e}");
        }
    });
    Ok(local)
}

async fn scrape(State(s): State<Scrape>) -> String {
    s.process.collect();
    s.handle.render()
}

fn install_recorder() -> Result<PrometheusHandle, ServeError> {
    let histograms: [(&str, &[f64]); 4] = [
        (names::COMMAND_DURATION, &names::COMMAND_BUCKETS),
        (names::TRACK_RESOLVE_DURATION, &names::COMMAND_BUCKETS),
        (names::AUTOCOMPLETE_DURATION, &names::AUTOCOMPLETE_BUCKETS),
        (
            names::SLEEVENOTE_REQUEST_DURATION,
            &names::SLEEVENOTE_BUCKETS,
        ),
    ];
    let mut builder = PrometheusBuilder::new();
    for (name, buckets) in histograms {
        builder = builder
            .set_buckets_for_metric(Matcher::Full(name.to_owned()), buckets)
            .map_err(|_| ServeError::AlreadyInstalled)?;
    }
    let recorder = builder.build_recorder();
    let handle = recorder.handle();
    metrics::set_global_recorder(recorder).map_err(|_| ServeError::AlreadyInstalled)?;
    Ok(handle)
}

fn describe_all() {
    describe_gauge!(
        names::BUILD_INFO,
        "Always 1; the version label is the running build."
    );
    describe_gauge!(
        names::GATEWAY_CONNECTED,
        "1 while the Discord gateway is connected."
    );
    describe_gauge!(
        names::GATEWAY_LATENCY,
        Unit::Seconds,
        "Gateway heartbeat latency."
    );
    describe_counter!(
        names::GATEWAY_EVENTS,
        "Gateway ready/resumed/reconnect events."
    );
    describe_gauge!(names::GUILDS, "Servers the bot is in.");
    describe_gauge!(names::VOICE_CONNECTIONS, "Active voice connections.");
    describe_gauge!(names::TRACKS_PLAYING, "Tracks currently playing.");
    describe_gauge!(names::QUEUED_TRACKS, "Tracks queued across all servers.");
    describe_counter!(names::COMMANDS, "Commands invoked, by name and kind.");
    describe_counter!(names::COMMAND_ERRORS, "Commands that returned an error.");
    describe_histogram!(names::COMMAND_DURATION, Unit::Seconds, "Command run time.");
    describe_histogram!(
        names::AUTOCOMPLETE_DURATION,
        Unit::Seconds,
        "Autocomplete response time."
    );
    describe_counter!(
        names::TRACK_RESOLVES,
        "Track resolutions, by source and outcome."
    );
    describe_histogram!(
        names::TRACK_RESOLVE_DURATION,
        Unit::Seconds,
        "Track resolution time."
    );
    describe_counter!(
        names::TRACK_FAILURES,
        "Track failures, by source and reason."
    );
    describe_counter!(
        names::TRACKS_STARTED,
        "Tracks that started playing, by source."
    );
    describe_counter!(names::TRACKS_ENDED, "Tracks that ended, by outcome.");
    describe_gauge!(names::GUILD_INFO, "Always 1; maps a guild_id to its name.");
    describe_counter!(names::GUILD_COMMANDS, "Commands invoked per server.");
    describe_counter!(names::GUILD_TRACKS_STARTED, "Tracks started per server.");
    describe_counter!(
        names::GUILD_PLAYBACK_SECONDS,
        Unit::Seconds,
        "Seconds of playback per server."
    );
    describe_counter!(
        names::GUILD_VOICE_CONNECTED_SECONDS,
        Unit::Seconds,
        "Seconds connected to voice per server."
    );
    describe_counter!(
        names::PROXY_FALLBACK,
        "Gated tracks sent through the residential proxy, by outcome."
    );
    describe_counter!(
        names::PROXY_FALLBACK_ATTEMPTS,
        "Residential proxy sessions tried."
    );
    describe_counter!(
        names::SEARCH_FALLBACK,
        "Search fallbacks, by reason and outcome."
    );
    describe_counter!(
        names::SLEEVENOTE_REQUESTS,
        "Requests to sleevenote, by outcome."
    );
    describe_counter!(
        names::SLEEVENOTE_RETRIES,
        "Retries of sleevenote requests, by reason."
    );
    describe_histogram!(
        names::SLEEVENOTE_REQUEST_DURATION,
        Unit::Seconds,
        "Sleevenote request time."
    );
    describe_counter!(
        names::SUGGESTION_LABEL,
        "Suggestion labels remembered or stripped."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    /// Minimal HTTP/1.1 GET: returns (status line, body).
    async fn get(addr: SocketAddr, path: &str) -> (String, String) {
        let mut s = TcpStream::connect(addr).await.unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).await.unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_owned(), body.to_owned())
    }

    #[tokio::test]
    async fn serves_build_info_and_zeroed_gauges() {
        let addr = serve("127.0.0.1:0", "9.9.9").await.expect("serve");
        let (status, body) = get(addr, "/metrics").await;
        assert!(status.contains("200"), "{status}");
        for want in [
            "cracktunes_build_info{version=\"9.9.9\"} 1",
            "cracktunes_gateway_connected 0",
            "cracktunes_voice_connections 0",
            "cracktunes_tracks_playing 0",
            "cracktunes_queued_tracks 0",
        ] {
            assert!(body.contains(want), "missing {want:?} in:\n{body}");
        }
        let (status, _) = get(addr, "/other").await;
        assert!(status.contains("404"), "{status}");
    }

    #[tokio::test]
    async fn a_bad_address_is_an_error_not_a_panic() {
        for b in ["9091", "localhost:9091", "", "0.0.0.0:99999"] {
            assert!(
                matches!(serve(b, "x").await, Err(ServeError::BadAddress(_))),
                "{b:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_taken_port_is_an_error_not_a_panic() {
        let held = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken = held.local_addr().unwrap().to_string();
        assert!(matches!(serve(&taken, "x").await, Err(ServeError::Bind(_))));
    }
}
