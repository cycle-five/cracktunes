# Prometheus metrics

Optional, off by default. The bot can serve its own counters, gauges and
histograms in the Prometheus text format, so you can graph and alert on what it
is doing.

## Turning it on

Set `METRICS_BIND` to an address and port, and publish that port from the
container:

```shell
METRICS_BIND=0.0.0.0:9091
```

The bot then logs `metrics on <addr>` and answers `GET /metrics` on that
address. With `METRICS_BIND` unset or empty, nothing listens, no recorder is
installed and the bot logs `metrics off` once.

The endpoint has no login. Bind it to a private address and never route it
through a public reverse proxy or tunnel. It is its own listener, not a route on
the web dashboard.

If the address cannot be used (not `host:port`, the port is taken, or the port
is the one `WEB_BIND` gives the dashboard) the bot
logs one ERROR line, `metrics off: METRICS_BIND=...`, and keeps running.
Metrics never make a command fail.

Counters with a bounded label set are exported at 0 from start-up, so
`increase()` sees the first event.

## Scrape config

```yaml
scrape_configs:
  - job_name: cracktunes
    scrape_interval: 15s
    static_configs:
      - targets: ["bots-cracktunes-1:9091"]
```

Two expressions worth alerting on:

```promql
# the bot has heard nothing from Discord for five minutes (alert on this one: it
# still fires if the bot's sampler task dies)
time() - cracktunes_gateway_last_event_timestamp_seconds > 300

# the same idea as a dashboard stat: 0 when no gateway event arrived in 120 s
cracktunes_gateway_connected

# three or more gated tracks needed the residential proxy in the last hour
sum(increase(cracktunes_proxy_fallback_total[1h])) >= 3
```

## Labels are closed sets

Every label value comes from a fixed list in the code. No track title, URL,
search query, user or channel ever appears, and neither does error text. The
only free-form labels are `command` (the command's own name), `name` (a server
name, on one metric), `version` (the build, on `cracktunes_build_info`) and
`guild_id`, which appears only on the five per-server metrics below.

## Gateway and state

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `cracktunes_build_info` | gauge | `version` | Always 1; the label is the running build. |
| `cracktunes_gateway_connected` | gauge | none | 1 if a gateway event arrived in the last 120 s (set to 1 at once on connect or resume), re-evaluated every 15 s. It cannot observe a disconnect directly. |
| `cracktunes_gateway_last_event_timestamp_seconds` | gauge | none | Unix time of the most recent gateway event received; 0 before the first. |
| `cracktunes_gateway_latency_seconds` | gauge | none | Gateway heartbeat latency. |
| `cracktunes_gateway_events_total` | counter | `event`: `ready`, `resumed`, `reconnect` | Gateway lifecycle events. `reconnect` is a `Ready` after the first `Ready` of this process: a fresh session rather than a resume. A `Ready` counts under both `ready` and `reconnect`. |
| `cracktunes_guilds` | gauge | none | Servers the bot is in. |
| `cracktunes_voice_connections` | gauge | none | Active voice connections. |
| `cracktunes_tracks_playing` | gauge | none | Tracks currently playing. |
| `cracktunes_queued_tracks` | gauge | none | Tracks queued across all servers. |

The gateway gauges assume one shard, which is what production runs. With more
than one, the last shard to report wins.

## Commands

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `cracktunes_commands_total` | counter | `command`, `kind`: `slash`, `prefix` | Commands invoked. |
| `cracktunes_command_errors_total` | counter | `command` | Commands that returned an error. |
| `cracktunes_command_duration_seconds` | histogram | `command` | Run time of commands that succeeded; a failed command is not timed. Buckets 0.1 to 30 s. |
| `cracktunes_autocomplete_duration_seconds` | histogram | none | Autocomplete response time. Buckets 0.1 to 5 s. |

## Tracks

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `cracktunes_track_resolves_total` | counter | `source`, `outcome`: `ok`, `error` | Per-track lookups of a playable stream. |
| `cracktunes_track_resolve_duration_seconds` | histogram | `source` | Time to resolve one track. Buckets 0.1 to 30 s. |
| `cracktunes_track_failures_total` | counter | `source`, `reason`: `no_results`, `gated`, `drm`, `unavailable`, `timeout`, `other` | Why a track could not be resolved. |
| `cracktunes_tracks_started_total` | counter | `source` | Tracks accepted into the queue. |
| `cracktunes_tracks_ended_total` | counter | `outcome`: `finished`, `skipped`, `errored` | Tracks that left playback. |

`source` is `youtube`, `soundcloud`, `spotify`, `file` or `other`. It is derived
from the kind of query: a keyword search is `youtube`, and a direct link is
bucketed by its host. `spotify` is emitted only when a Spotify track or link
query reaches a queue call directly, or a link whose host is Spotify's does; a
pasted Spotify link is turned into searches first and counts as `youtube`.

Read these with care:

- `cracktunes_tracks_started_total` counts tracks accepted into the queue, not
  tracks that have begun to sound.
- A pasted Spotify link is turned into searches, so its tracks count under
  `youtube`. Spotify link volume and failures show up in
  `cracktunes_sleevenote_requests_total` and its duration histogram. There is
  no sleevenote retries counter; a retried request shows as a slow one.
- A YouTube playlist link arrives already resolved from one fetch, so it
  records starts but no per-track resolves. `cracktunes_track_resolves_total`
  is not "tracks played".
- In `cracktunes_tracks_ended_total`, `skipped` covers a skip and a stop
  (songbird ends both in the same state), `finished` is a track that ran out,
  and `errored` is a playback error. `/stop` stops every queued track, and
  songbird fires the end event for each one, so a stop of N queued tracks adds N
  to `skipped`.
- `cracktunes_tracks_ended_total` can exceed `cracktunes_tracks_started_total`:
  ends are counted for every non-theme track, including `/gp` rounds, autoplay,
  queue resume and playlist loads, while starts are counted only for tracks
  queued through `/play`-style commands.
- A track that needed a second try (the fallback resolver) is one resolve: its
  final outcome and total time. A failure the fallback recovered from is not
  counted.

## Per server

Only these five carry `guild_id`.

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `cracktunes_guild_info` | gauge | `guild_id`, `name` | Always 1; joins a server name onto the `guild_id` series. |
| `cracktunes_guild_commands_total` | counter | `guild_id` | Commands invoked in the server. |
| `cracktunes_guild_tracks_started_total` | counter | `guild_id` | Tracks accepted into the queue in the server. |
| `cracktunes_guild_playback_seconds_total` | counter | `guild_id` | Seconds of playback in the server. |
| `cracktunes_guild_voice_connected_seconds_total` | counter | `guild_id` | Seconds connected to voice in the server. |

The two `_seconds_total` counters are whole seconds advanced by a 15-second
sampler, which carries the sub-second remainder from one tick to the next. A
paused track adds no playback time.

A renamed or departed server's old `cracktunes_guild_info` series stays until
the bot restarts.

## Fallbacks and upstream services

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `cracktunes_proxy_fallback_total` | counter | `outcome`: `recovered`, `exhausted`, `error` | Gated tracks sent through the residential proxy, counted once per track. |
| `cracktunes_proxy_fallback_attempts_total` | counter | none | Residential proxy sessions tried. |
| `cracktunes_search_fallback_total` | counter | `reason`: `empty`, `error`; `outcome`: `found`, `empty`, `error` | Fallback searches after the first search came back empty or failed. |
| `cracktunes_sleevenote_requests_total` | counter | `outcome`: `ok`, `error` | Calls to the sleevenote client, one sample per call. |
| `cracktunes_sleevenote_request_duration_seconds` | histogram | none | Sleevenote request time. Buckets 0.5 to 30 s. |
| `cracktunes_suggestion_label_total` | counter | `outcome`: `remembered`, `stripped` | `/play` suggestion labels that came back in place of a link. |

`cracktunes_proxy_fallback_total` counts once per gated track. After a track has
been reported as `exhausted` or `error`, a later attempt on the same track may
try more proxy sessions that `cracktunes_proxy_fallback_attempts_total` does not
count. See [residential-proxy-fallback.md](residential-proxy-fallback.md).

## Process metrics

The listener also exposes the standard process collector, with its usual names:
`process_cpu_seconds_total`, `process_resident_memory_bytes`,
`process_virtual_memory_bytes`, `process_virtual_memory_max_bytes`,
`process_open_fds`, `process_max_fds`, `process_threads` and
`process_start_time_seconds`. Which of these appear depends on the platform; on
Linux all do. `process_max_fds` and `process_virtual_memory_max_bytes` read 0
when the limit is unlimited.
