# Metrics and monitoring for cracktunes — design

**Status:** awaiting owner approval. Written 2026-11-25.

**Two parts, planned separately, in this order:**

1. **cracktunes** (this repo): a release that exposes metrics. Minor bump, v0.48.0.
2. **homelab** (`cycle-five/homelab`): firewall rule, scrape job, alert rules
   with tests, Grafana dashboard.

Part 2 depends on the metric names in part 1, which is why one document covers
both. Each part gets its own plan.

## Why

On 2026-11-25 the owner asked whether anything records when cracktunes uses
its residential-proxy fallback (v0.47.0). Nothing does. The answer that day
came from reading 13 hours of container logs by hand. More broadly cracktunes
is absent from the estate's monitoring: no scrape job, no alert rule, no
dashboard. The owner wants it on the same Prometheus / Alertmanager / Grafana
stack as everything else.

What the owner said they want:

- **Discord pings, two:** the health check failing for more than 5 minutes,
  and "more than like 2 or 3" proxy-fallback uses in an hour.
- **Graphs:** "as much as we can that's useful": playback statistics broken
  down by server, command usage, voice-channel time, "probably more I'm not
  thinking of".
- **No public endpoint.** Scraping through Cloudflare "defeats the point of
  having the internal cluster".

## What exists today

**cracktunes**

- A `crack-metrics` cargo feature gates a `prometheus` registry in
  `crack-core/src/metrics.rs` with four command-level metrics. Production
  images are built with `CARGO_FEATURES=crack-coin` only, so it is compiled
  out, and every command logs `crack-metrics feature not enabled!` at WARN.
- Nothing serves the registry: the HTTP handler was deleted and its callers in
  `crack-cli/src/main.rs` are commented out.
- The fallbacks log one line each at most. The proxy fallback
  (`crack-music/src/sources/proxy_fallback.rs`) logs that it triggered, never
  how it ended.
- The web dashboard (`crack-web`, axum) listens on `WEB_BIND` (8090), and the
  edge Caddy forwards all of `dash.cracktun.es` to that port.

**homelab**

- Prometheus and Alertmanager on LXC 111 (`192.168.1.245`, a DHCP reservation
  tied to its MAC), Grafana 13 on LXC 112. Config, rules and dashboards are
  reconciled from `monitoring/` by `./homelab.sh monitoring --apply`
  (`monitoring/monitoring.py`, the `MANAGED` list). Rule files have promtool
  tests in `monitoring/tests/`. Alertmanager posts to Discord.
- The snapcam board is the model for a dashboard this repo owns: its own
  provider YAML and directory, so it lands in its own Grafana folder.
- **Production cracktunes runs on VM 110 `bots`, `192.168.30.30`, VLAN 30.**
  LAN → VLAN 30 is blocked by firewall rule 88; the single exception above it
  is rule 86 (SSH from the workstation). VLAN 30 → LAN is blocked by rules 301
  and 401. Prometheus cannot reach the bots VM.
- **TuneTitan runs on VM 103 `docker-staging`, `192.168.1.116`, on the LAN.**
  Prometheus already scrapes that host's node_exporter.

## Goals

- Both alerts the owner named reach Discord.
- One Grafana board answers "is the bot healthy, what is it playing, where,
  and how often is it falling back" without reading logs.
- A metric can be added later by touching one crate and one call site.

## Non-goals (later, each its own piece)

- Grafana panels that read the bot's Postgres ("top tracks", history from
  before this ships). Owner: "A now, the rest later."
- Host and container metrics for the VLAN 30 machines (node_exporter,
  cAdvisor).
- Log shipping, tracing.
- Alerts for TuneTitan. It is the beta tenant; it is scraped and graphed, not
  paged on.

## Anti-goals

- **A metrics endpoint reachable from the internet.**
- **Unbounded label values.** No track title, URL, search query, user id,
  channel id or error text is ever a label.
- **Metrics that can hurt playback.** A metrics listener that fails to bind
  logs an error and the bot carries on; no metric call may block or return an
  error into a command.
- **A firewall rule broader than one source, one destination, one port.**

## Approaches

**Where per-server history comes from** (owner picked A):

- **A. Prometheus only.** Every number is a counter, gauge or histogram the
  bot exposes. One firewall rule, one data source. History starts the day it
  ships; no "top tracks".
- **B. Prometheus plus Postgres panels.** Adds top tracks and old history, at
  the cost of a second hole (Grafana → the bots VM's Postgres), a read-only
  database user and SQL panels to maintain.
- **C. A plus database-derived gauges.** A few totals with no second hole;
  still no top tracks.

**How Prometheus reaches the bot** (owner picked 2):

1. Scrape `https://dash.cracktun.es/metrics` with a bearer token. No network
   change, but a public endpoint. Rejected by the owner.
2. **One narrow firewall rule, LAN → VLAN 30.** Chosen. It leaves the
   VLAN 30 → LAN blocks untouched: the bots VM can answer a scrape and still
   cannot open a connection to anything on the LAN.
3. A collector inside VLAN 30 pushing to Prometheus. Rejected: it opens a
   path from VLAN 30 into the LAN, the direction the isolation exists to stop.

Also rejected: a second network leg for the Prometheus container on VLAN 30,
which would make it a bridge between the segments.

## Part 1 — cracktunes

### Shape (my calls, not the owner's)

- **Library: the `metrics` facade with `metrics-exporter-prometheus`.** The
  events to count live in at least five crates (`crack-music`,
  `crack-testing`, `crack-types`, `crack-sleevenote`, `crack-core`), and the
  current registry sits in `crack-core`, which the lower crates cannot depend
  on. With the facade any crate records, and with no recorder installed every
  call is a no-op, which is what tests and unconfigured deployments get.
- **A new leaf crate, `crack-metrics`, owns every metric.** It depends on no
  other workspace crate. It holds:
  - every metric name as a constant;
  - enums for every label value (`Source`, `Outcome`, `FailureReason`, …),
    so a label value that is not in the enum does not compile;
  - one function per event, for example
    `proxy_fallback(ProxyOutcome::Recovered, attempts)`;
  - `install(bind)`, which installs the recorder, registers descriptions and
    starts the listener.

  No other crate writes a metric name or calls a `metrics` macro. This is the
  "enforce with types" rule applied to labels.
- **Always compiled in.** The `crack-metrics` cargo feature, the
  `prometheus` dependency, `crack-core/src/metrics.rs` and both
  `count_command` variants are deleted. The per-command WARN goes with them.
- **`METRICS_BIND` turns it on**, the way `WEB_BIND` does for the dashboard.
  Unset: no recorder, no listener, one INFO line `metrics off`. Set: its own
  listener on that address serving `GET /metrics`, one INFO line
  `metrics on <addr>`. A bind failure is one ERROR line and the bot continues.
- **A separate port, not a route on the dashboard.** Caddy forwards the whole
  of `dash.cracktun.es` to 8090, so a `/metrics` route there would be public.
  Port **9091** unless the builder finds it taken on either host.
- **No token on the endpoint.** The bind address and the firewall rule are the
  control; it serves counts, no secrets.

### Metrics

Prefix `cracktunes_`. Label values in braces are the complete set.

**Health**

| Metric | Type | Labels |
|---|---|---|
| `cracktunes_build_info` | gauge (always 1) | `version` |
| `cracktunes_gateway_connected` | gauge 0/1 | — |
| `cracktunes_gateway_latency_seconds` | gauge | — |
| `cracktunes_gateway_events_total` | counter | `event` {ready, resumed, reconnect} |
| `cracktunes_guilds` | gauge | — |
| `cracktunes_voice_connections` | gauge | — |
| `cracktunes_tracks_playing` | gauge | — |
| `cracktunes_queued_tracks` | gauge (all servers) | — |

**Commands**

| Metric | Type | Labels |
|---|---|---|
| `cracktunes_commands_total` | counter | `command`, `kind` {slash, prefix} |
| `cracktunes_command_errors_total` | counter | `command` |
| `cracktunes_command_duration_seconds` | histogram | `command` |
| `cracktunes_autocomplete_duration_seconds` | histogram | — |

The autocomplete histogram has a bucket edge at 3 s: Discord drops a response
slower than that, and poise's own "couldn't send autocomplete response" is not
ours to hook.

**Playback**

| Metric | Type | Labels |
|---|---|---|
| `cracktunes_track_resolves_total` | counter | `source`, `outcome` {ok, error} |
| `cracktunes_track_resolve_duration_seconds` | histogram | `source` |
| `cracktunes_track_failures_total` | counter | `source`, `reason` |
| `cracktunes_tracks_started_total` | counter | `source` |
| `cracktunes_tracks_ended_total` | counter | `outcome` {finished, skipped, errored} |

`source` {youtube, soundcloud, spotify, file, other}: what the user asked for,
so a Spotify link counts as spotify although it plays from YouTube.
`reason` {no_results, gated, drm, unavailable, timeout, other}: classified
from the error in one function in `crack-metrics`, with `other` as the
default so an unrecognised error is still counted.

**Per server** — the only metrics that carry `guild_id`:

| Metric | Type | Labels |
|---|---|---|
| `cracktunes_guild_info` | gauge (always 1) | `guild_id`, `name` |
| `cracktunes_guild_commands_total` | counter | `guild_id` |
| `cracktunes_guild_tracks_started_total` | counter | `guild_id` |
| `cracktunes_guild_playback_seconds_total` | counter | `guild_id` |
| `cracktunes_guild_voice_connected_seconds_total` | counter | `guild_id` |

The two `_seconds_total` counters advance while the session is running (added
on a short timer, or at each track end and on the timer for voice time), not
only when it ends: a six-hour session must show as six hours of slope, not a
single jump at disconnect.

Commands by server and commands by name are separate counters on purpose: one
counter with both labels is 155 servers × ~100 commands. `guild_info` joins a
name onto the others in Grafana; a renamed server leaves the old series behind
until the next restart, which is acceptable.

**Fallbacks**

| Metric | Type | Labels |
|---|---|---|
| `cracktunes_proxy_fallback_total` | counter | `outcome` {recovered, exhausted, error} |
| `cracktunes_proxy_fallback_attempts_total` | counter | — |
| `cracktunes_search_fallback_total` | counter | `reason` {empty, error}, `outcome` {found, empty, error} |
| `cracktunes_sleevenote_requests_total` | counter | `outcome` {ok, error} |
| `cracktunes_sleevenote_retries_total` | counter | `reason` |
| `cracktunes_suggestion_label_total` | counter | `outcome` {remembered, stripped} |

`cracktunes_proxy_fallback_total` counts **once per gated track**, when the
fallback finishes, not once per call into the source: `ProxyFallback` can be
entered through `create`, `create_async` and `aux_metadata` for the same
track. The proxy fallback also gains one log line per gated track with the
outcome and the attempts used.

**Process:** CPU seconds, resident memory, open file descriptors and start
time, under the standard `process_*` names, from a maintained collector crate
for the `metrics` facade. If none fits, the builder says so and this row is
dropped rather than hand-rolled.

### Tests

- Each event function in `crack-metrics` is tested against a local debugging
  recorder: the name, the label values and the amount recorded.
- The glue is tested where a seam exists. `ProxyFallback` already has an
  injected factory (`from_parts`): a gated source that recovers on the second
  attempt records one `recovered` and two attempts; one that never recovers
  records one `exhausted` and five; an ungated failure records nothing.
- A test fetches `/metrics` from a listener on an ephemeral port and finds
  `cracktunes_build_info` with the crate version.
- A test confirms that with `METRICS_BIND` unset nothing listens and event
  functions do nothing.

### Left to the builder

Exact histogram buckets; where in each handler the call goes; crate versions;
whether gauges are set on events or sampled on a timer; the process collector.

## Part 2 — homelab

### Network

- **One OPNsense rule, through the API, with a savepoint so it rolls back if
  not confirmed:** `pass`, interface `lan`, source `192.168.1.245`,
  destination `192.168.30.30`, TCP port 9091, **gateway left at default**,
  sequence 87 (between rule 86 and the block at 88), description naming
  Prometheus and cracktunes. The gateway matters: the catch-all LAN rule
  policy-routes everything to the WAN group.
- **Isolation re-proved, three checks, results recorded in
  `docs/network-inventory.md` beside rule 86:**
  1. Prometheus reaches `192.168.30.30:9091`.
  2. Prometheus cannot reach another port on VLAN 30 (`:8090`, `:22`).
  3. Another LAN host cannot reach `192.168.30.30:9091`.
- TuneTitan needs no rule; it is on the LAN.

### Deploy config

- `bots/docker-compose.yml` and the TuneTitan stack: `METRICS_BIND:
  0.0.0.0:9091` and the port published on the host's bind address, next to
  the existing 8090 line.
- `verify bots` and `verify tunetitan` assert that `/metrics` answers and
  that `cracktunes_build_info` carries the pinned version.

### Scrape

One job, `cracktunes`, two targets, both labelled `service: cracktunes`:

| Target | `instance` |
|---|---|
| `192.168.30.30:9091` | `cracktunes-prod` |
| `192.168.1.116:9091` | `tunetitan` |

### Alerts

`monitoring/cracktunes_rules.yml`, added to `rule_files` and to `MANAGED`,
with `monitoring/tests/cracktunes_rules_test.yml` exercising every rule.
All four match `instance="cracktunes-prod"` only and go to the existing
homelab Discord channel.

| Alert | Fires when | Severity |
|---|---|---|
| `CracktunesDown` | the scrape has failed for 5 minutes | critical |
| `CracktunesGatewayDisconnected` | `cracktunes_gateway_connected == 0` for 5 minutes | critical |
| `CracktunesProxyFallbackBurst` | 3 or more proxy fallbacks in the last hour | warning |
| `CracktunesMetricsMissing` | the target or the gateway metric has been absent for 10 minutes | warning |

The first two are the owner's "health check failing for more than 5 minutes":
the process is unreachable, or it is up and not connected to Discord. The
third is the owner's fallback alert; "more than like 2 or 3" is set at 3 and
is one number to change. The fourth exists because of a trap already recorded
in `docs/homelab-infra.md`: an alert on a metric that is absent cannot fire.

### Dashboard

`monitoring/cracktunes-dashboard.json`, with its own provider file and
directory so it lands in a `cracktunes` folder, both in `MANAGED`. An
`instance` picker at the top switches between production and TuneTitan.

```
[ instance: cracktunes-prod ▾ ]                              last 24h ▾
┌ Health ─────────────────────────────────────────────────────────────┐
│ Up  │ Discord │ Version │ Uptime │ Servers │ Voice now │ Playing now │
│ gateway latency ───────────  │ restarts / reconnects ────────────── │
├ Playback ───────────────────────────────────────────────────────────┤
│ tracks started by source ──  │ failures by reason ───────────────── │
│ resolve time p50 / p95 ────  │ ended: finished / skipped / errored  │
├ Fallbacks ──────────────────────────────────────────────────────────┤
│ proxy fallback by outcome (last hour: N)  │ search fallback ─────── │
│ sleevenote retries ────────  │ suggestion labels recovered ──────── │
├ Commands ───────────────────────────────────────────────────────────┤
│ commands / min by name ────  │ errors by command │ p95 time, top 10 │
│ autocomplete time, with the 3 s line ────────────────────────────── │
├ Servers ────────────────────────────────────────────────────────────┤
│ top 15 by tracks started │ top 15 by play time │ top 15 by commands │
│ table: server · tracks · play time · voice time · commands ──────── │
├ Process ────────────────────────────────────────────────────────────┤
│ CPU ─────────── │ memory ─────────── │ open files ───────────────── │
└─────────────────────────────────────────────────────────────────────┘
```

Panel choice inside each row is the builder's.

### Docs

`docs/network-inventory.md` (the rule and the isolation results),
`docs/homelab-infra.md` (the scrape job, alerts and a short runbook per
alert), and the 2026-10-27 dashboards handoff updated to say that the bots
VM's application metrics are now collected.

## Order of work

1. cracktunes v0.48.0: PR, review, merge, tag, release.
2. Deploy to TuneTitan with `METRICS_BIND`; add the scrape job. This proves
   the metrics against a real bot with no firewall change.
3. Add the firewall rule and run the three isolation checks.
4. Deploy to production (fresh backup first, as always); confirm the
   production target is up.
5. Alert rules with tests, then the dashboard.

Steps 3 and 4 change the firewall and production; each waits for the owner's
go-ahead when it is reached.
