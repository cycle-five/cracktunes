# Residential-proxy fallback for gated YouTube — design

**Date:** 2026-11-23
**Status:** approved (owner, 2026-11-23) — build through to a TuneTitan staging deploy.
**Supersedes, for cracktunes' purposes:** the lavalink-ops `feat/residential-proxy-pool`
branch. That branch is parked: its egress mechanism is proven dead (youtube-source
ignores both Lavalink `httpConfig` and JVM system-proxy properties — see its SDD
ledger and `docs/youtube-datacenter-gate-findings.md`), while its *control-plane
design* (provider adapter, sticky sessions, spend posture) is what carries over here.

## Intent & why

cracktunes plays YouTube only because the bot runs on a **residential IP**
(issue #398). Age/login-gated videos (e.g. Tyler, The Creator — "Yonkers",
`XSbZidsgMfw`) resolve fine from residential but fail from a datacenter IP with
`This video requires login` — the gate is **IP-reputation at resolve**, and
neither OAuth nor PoToken clears it (measured; `docs/youtube-datacenter-gate-findings.md`).
The moment the bot moves to a datacenter, gated content breaks.

The proven lever is **residential egress that carries the media bytes**. yt-dlp
honors `--proxy` natively (the premise spike used exactly this and cleared the
gate). cracktunes is already a songbird + yt-dlp app — the hard part (sending
audio to Discord) exists and works — so the fix is small: when a direct resolve
hits the gate, retry that one track through a paid residential proxy
(DataImpulse), and carry its stream bytes through the same residential exit IP.

## Goals

- A **gated YouTube track that fails a direct (datacenter) resolve** is retried
  through a residential proxy and plays.
- **Normal (non-gated) tracks never touch the proxy** — they resolve and stream
  direct from the datacenter IP. No per-GB spend on the common case.
- A **provider seam** so other proxy *kinds* (mobile, …) can be added later as a
  new implementation, not a refactor. Only `DataImpulse` residential is built now.
- **Fail-safe & inert by default:** no proxy creds configured → no fallback →
  gated tracks fail exactly as they do today. Configuring creds self-activates it.

## Non-goals

- Routing **all** traffic through the proxy, or any always-on proxy egress.
- In-bot usage **metering / spend enforcement**. DataImpulse is **prepaid**, a
  hard ceiling that cannot be overspent, and exposes no reliable usage API. The
  prepaid balance *is* the cap.
- Replacing the direct path, touching non-YouTube sources, or migrating to
  Lavalink. Lavalink stays shelved for this problem.
- Editing the songbird fork. Both egress legs are reachable from cracktunes
  without it (see below). The fork remains a back-pocket lever, unused here.

## Anti-goals (failure even if it "works")

- Proxy credentials (username/password, authenticated proxy URL) appearing in
  **logs, panels, errors, tracing, or test fixtures**. A single leak of the paid
  creds is a failure.
- A normal track silently paying for proxy bandwidth because detection was too
  broad.
- A gated track that resolves through the proxy but then **streams direct** and
  403s — the two legs MUST share one exit IP (see the IP-binding constraint).

## Constraints

- **Two-leg IP binding (central constraint).** A resolved `googlevideo` stream
  URL embeds `&ip=<resolver>` and returns `403` when fetched from any other IP
  (measured: 206 same-IP, 403 cross-IP). So the **resolve** leg (yt-dlp) and the
  **byte-fetch** leg (songbird's reqwest `HttpRequest`) MUST exit from the *same*
  residential IP. A sticky session that pins one exit IP for both legs satisfies
  this; a rotating session that changes IP between the two legs would 403.
- **DataImpulse gateway:** host from config, **port 823** (port 1000 is dead —
  measured). Sticky syntax: append **`;sessid-<id>`** to the base username; each
  distinct sessid pins one residential exit IP for that session's lifetime.
- Both egress legs are reachable without a fork edit:
  - Resolve: songbird `YoutubeDl::query()` runs yt-dlp with `self.user_args`
    (confirmed) → pass `--proxy <authenticated-url>`.
  - Stream: `YoutubeDl::get_stream()` fetches via `self.client`, and `client` is
    a **constructor argument** (`YoutubeDl::new(client, …)`) → pass a reqwest
    client built with `.proxy(…)` on the same session.
- `clippy.toml` bans `songbird::input::YoutubeDl::new` directly; the sanctioned
  constructor is `crack_types::http::ytdl_for_url(client, url)`. The proxied
  variant extends that same seam.
- No `git add -A`. Commit trailer exactly `Co-Authored-By: Claude & Lothrop
  (cycle.five@proton.me)` and no other `Co-Authored-By` line.

## Approaches considered

1. **Finish Lavalink + transparent redirect (redsocks/iptables).** Force
   youtube-source's egress through the proxy at L3/L4 (it honors no app-level
   proxy config). Rejected: privileged container (NET_ADMIN), its own netns, a
   custom image and a redsocks sidecar — heavy new infra, and it still requires
   adopting Lavalink into cracktunes, which hasn't happened. All to patch a tool
   that fights proxying.
2. **yt-dlp in-process, fork-edited stream leg.** Add `--proxy` to the resolve
   and edit the songbird fork so `HttpRequest` uses a proxy. Rejected as
   unnecessary: the client is already a constructor arg, so the stream leg is
   reachable without touching the fork (approach 3).
3. **yt-dlp in-process, proxied reqwest client (CHOSEN).** On a gated-resolve
   failure, rebuild the YouTube source with (a) a reqwest client carrying the
   proxy and (b) `--proxy` on yt-dlp, both on one fresh sticky session. Both legs
   exit the same residential IP; the IP-bound URL fetches fine. No fork edit, no
   new infra, no privileged container — a contained feature in the repo that
   already does everything else. The mechanism (yt-dlp `--proxy`) is already
   proven against the gate.

## The design

### Components (file structure)

- **`crack-music/src/proxy/mod.rs`** — the provider seam and config.
  - `enum ProxyKind { Residential /* , Mobile … later */ }`
  - `trait ProxyProvider { fn kind(&self) -> ProxyKind; fn session(&self) -> ProxySession; }`
    where `ProxySession` yields the **authenticated proxy URL** and the matching
    reqwest `Proxy` for one fresh sticky session (same creds/sessid for both).
  - `struct DataImpulseResidential { gateway_host, gateway_port=823, base_user,
    password }` — the only impl now. `session()` generates a fresh
    `;sessid-<hex>` and builds the URL `http://{base_user};sessid-{id}:{password}@{host}:{port}`.
  - `fn from_env() -> Option<Box<dyn ProxyProvider>>` — `Some` only when all
    required creds are present (inert-by-default), else `None`.
  - The provider/session types carry the password as a non-logging wrapper and
    never appear in `Debug`/`Display` output with creds intact.
- **`crack-music/src/proxy/gate.rs`** — `fn is_gate_error(stderr: &str) -> bool`.
  Conservative substring match on known yt-dlp gate phrases
  (`"Sign in to confirm your age"`, `"This video requires login"`,
  `"Sign in to confirm you're not a bot"`, `"confirm your age"`, `"age-restricted"`),
  case-insensitive. Tunable; pinned by unit tests on captured strings and
  verified against a live datacenter resolve in the acceptance gate.
- **`crack_types::http`** — add `ytdl_for_url_proxied(proxied_client, url,
  proxy_url)` beside `ytdl_for_url`: same seam, with `--proxy <proxy_url>`
  prepended to `user_args` and the proxied client passed through so `get_stream`
  inherits it.
- **`crack-music/src/music/query.rs`** — route YouTube URL/search resolve through
  a single helper that applies the fallback (below). This is the one behavioral
  change at the call sites.

### Flow

1. Resolve a YouTube track **direct** (datacenter IP), as today.
2. On failure, if `is_gate_error(stderr)` **and** a `ProxyProvider` is configured:
   take one `session()` (fresh sticky sessid), build `ytdl_for_url_proxied` with
   that session's proxy URL + a reqwest client carrying that session's proxy, and
   resolve again.
   - Both the retry resolve (yt-dlp `--proxy`) and the later stream fetch
     (`get_stream` via the proxied client) use the **same sessid → same exit IP**,
     satisfying the IP binding. The googlevideo URL (resolved through the
     residential IP) fetches 206 through that same IP.
3. If no provider is configured, or the error is not a gate error, surface the
   original failure unchanged (today's behavior).

Normal tracks reach step 1 and succeed; they never build a proxied source.

### Why fresh-sessid-per-track

It's the minimal state that satisfies the IP binding: one track's resolve and
stream share a sessid (hence one exit IP) for the track's lifetime, and different
tracks get different IPs. No shared rotation timer, no global session manager, no
cap accounting — all of which are YAGNI given prepaid spend and gated-only
traffic. A long track holding one residential IP for its play is the desired
behavior, not a limitation.

## Config

Environment variables (read like the workspace's other secrets; absent ⇒ feature
off). Names to finalize in the plan, shape:

- `PROXY_FALLBACK_HOST` (DataImpulse gateway host)
- `PROXY_FALLBACK_PORT` (default `823`)
- `PROXY_FALLBACK_USER` (base username, pre-sessid)
- `PROXY_FALLBACK_PASSWORD`

All four required to activate. Never logged; redacted in any `Debug`.

## What's left to the builder

- Exact env-var names and how they thread into the resolve call sites (the plan
  pins the `query.rs` wiring and which resolve sites route through the helper).
- The precise `is_gate_error` phrase set (start from the list above; the
  acceptance gate confirms the real datacenter string and the plan's test pins it).
- Whether the proxied reqwest client is built per fallback (simplest) or cached —
  per-fallback is fine for the rare gated case unless profiling says otherwise.
- The non-logging wrapper for the password (reuse an existing secret wrapper in
  the workspace if one exists; otherwise a small newtype whose `Debug` is `***`).

## Acceptance (the gate before merge)

On a **datacenter** IP (a throwaway cloud box, as in the prior findings):

1. Direct resolve of `XSbZidsgMfw` fails with a gate error (captures the real
   string to pin `is_gate_error`).
2. With creds configured, the same track resolves **and streams** through the
   proxy (loads + serves bytes, not just metadata).
3. A normal track (`dQw4w9WgXcQ`) still resolves/streams **direct** (no proxy
   connection observed).
4. Creds removed ⇒ gated track fails as in step 1 (fail-safe).
5. No credential appears in logs, tracing, or error text in any of the above.
