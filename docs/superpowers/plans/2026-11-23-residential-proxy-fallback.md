# Residential-proxy fallback for gated YouTube — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a direct (datacenter) YouTube resolve fails the login/age gate, retry that one track through a residential proxy (DataImpulse), carrying both the resolve and the stream through one sticky residential exit IP; normal tracks stay direct.

**Architecture:** A `ProxyProvider` seam (one impl: DataImpulse residential, fresh sticky `;sessid-` per track) + a gate-error detector + a proxied `YoutubeDl` constructor (reqwest `.proxy()` client **and** yt-dlp `--proxy`, same session) + a `ProxyFallback` songbird `Compose` wrapper that rebuilds the source through the proxy only on a gate error. Wired into the two YouTube resolve chokepoints in `crack-music`. Inert without creds.

**Tech Stack:** Rust, songbird (CycleFive fork, `YoutubeDl.user_args` + client-arg), reqwest, yt-dlp, async-trait, once_cell.

**Spec:** `docs/superpowers/specs/2026-11-23-residential-proxy-fallback-design.md`

## Global Constraints

- **Two-leg IP binding:** the resolve (yt-dlp) and the stream byte-fetch (reqwest `get_stream`) MUST use the **same sticky session → same residential exit IP**. The `ProxyFallback` wrapper builds the proxied source **once** and reuses it for both `aux_metadata` and `create_async`.
- **DataImpulse:** gateway host from config, **port 823** default (1000 is dead). Sticky = base username + `;sessid-<hex>`. Each sessid pins one exit IP.
- **Credentials never leak:** no username/password/authenticated-URL in logs, tracing, `Debug`, errors, panels, or test fixtures. The password type redacts in `Debug`. Tests use fake creds only.
- **Inert by default:** provider is `Some` only when ALL creds are present; otherwise the feature is off and behavior is byte-for-byte today's.
- **Scope:** YouTube gated fallback only. Non-gated tracks and non-YouTube sources are untouched and never build a proxied source.
- Sanctioned constructor only: extend `crack_types::http::ytdl_for_url`; never call `songbird::input::YoutubeDl::new` directly (clippy-banned).
- No `git add -A`. Commit trailer exactly `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other `Co-Authored-By` line. Commit per task.

## Review Focus

- **Credential leakage** — the password/authenticated URL must not appear in any `Debug`/`Display`/log/error. (Task 1 pins a redaction test; every task avoids logging the session/URL.)
- **Semicolon in proxy auth** — `;sessid-` in the username must survive both legs: reqwest via `.basic_auth(user, pass)` (not URL-embedded), yt-dlp via the `--proxy` URL (URL-encode `;`→`%3B` if yt-dlp mis-parses). (Task 3 pins both.)
- **Same-IP across legs** — a resolve on one exit IP and a stream on another 403s. The wrapper must reuse one proxied source (one session) for metadata + stream, not build two. (Task 4 asserts a single proxied build is reused.)
- **Gate false-positive** — a genuinely unavailable/age-unrelated failure must NOT spend a proxy round. (Task 2 pins benign errors to `false`.)
- **Inert without creds** — missing/partial creds ⇒ no proxied source ever built, no behavior change. (Task 1 + Task 5 pin `from_env` `None` and the direct path unchanged.)

---

### Task 1: Proxy provider seam + config

**Files:**
- Create: `crack-music/src/proxy/mod.rs`
- Modify: `crack-music/src/lib.rs` (add `pub mod proxy;`)
- Test: inline `#[cfg(test)]` in `proxy/mod.rs`

**Interfaces:**
- Produces:
  - `enum ProxyKind { Residential }` (extensible)
  - `struct ProxySession { pub proxy_url: String, user: String, password: ProxyPassword }` with `pub fn reqwest_proxy(&self) -> reqwest::Result<reqwest::Proxy>` (built via `reqwest::Proxy::all(base)?.basic_auth(&self.user, self.password.expose())`) and `pub fn ytdl_proxy_url(&self) -> &str`.
  - `trait ProxyProvider: Send + Sync { fn kind(&self) -> ProxyKind; fn session(&self) -> ProxySession; }`
  - `struct DataImpulseResidential { host, port, base_user, password: ProxyPassword }` impl `ProxyProvider`; `session()` makes a fresh `sessid` (`secrets`-free: `format!("{:016x}", rand u64)` or `uuid` simple) and builds user `"{base_user};sessid-{sessid}"`, `proxy_url` `"http://{user}:{pw}@{host}:{port}"` (percent-encode `;` in the URL user; the reqwest path uses `basic_auth` and does not embed).
  - `struct ProxyPassword(String)` with `Debug`/`Display` printing `"***"` and `fn expose(&self) -> &str`.
  - `fn from_env() -> Option<Box<dyn ProxyProvider>>` — `Some` iff `PROXY_FALLBACK_HOST`, `PROXY_FALLBACK_USER`, `PROXY_FALLBACK_PASSWORD` all set (port from `PROXY_FALLBACK_PORT`, default `823`); else `None`.
  - `fn provider() -> Option<&'static dyn ProxyProvider>` — global `once_cell::sync::Lazy<Option<Box<dyn ProxyProvider>>>` over `from_env()`.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn session_user_has_fresh_sessid() { /* two session() calls → different sessid suffixes, both start with base_user + ";sessid-" */ }
#[test] fn ytdl_proxy_url_is_authenticated_and_percent_encodes_semicolon() { /* contains "%3Bsessid-" and host:823 and the password */ }
#[test] fn password_redacts_in_debug() { let p = ProxyPassword("hunter2".into()); assert_eq!(format!("{p:?}"), "***"); assert!(!format!("{:?}", di_provider_with(p)).contains("hunter2")); }
#[test] fn from_env_none_without_full_creds() { /* with HOST+USER set but PASSWORD unset (use a scoped env guard) → None */ }
#[test] fn reqwest_proxy_builds() { /* session().reqwest_proxy().is_ok() */ }
```

- [ ] **Step 2: Run — expect FAIL** (`cargo test -p crack-music proxy::`)
- [ ] **Step 3: Implement `proxy/mod.rs`** per the Interfaces block. `session()` is the only place a sessid is minted. Never log the session/url/password. Keep `ProxyKind` non-exhaustive-friendly (a plain enum is fine; one variant now).
- [ ] **Step 4: Run — expect PASS**; `cargo clippy -p crack-music` clean.
- [ ] **Step 5: Commit** (`feat(proxy): provider seam + DataImpulse residential config`).

### Task 2: Gate-error detection

**Files:**
- Create: `crack-music/src/proxy/gate.rs`
- Modify: `crack-music/src/proxy/mod.rs` (`pub mod gate;`)
- Test: inline in `gate.rs`

**Interfaces:**
- Produces: `pub fn is_gate_error(stderr: &str) -> bool` — case-insensitive substring match on the known yt-dlp gate phrases.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn matches_age_and_login_gates() {
    for s in ["Sign in to confirm your age", "This video requires login",
              "Sign in to confirm you’re not a bot", "This video may be inappropriate for some users"] {
        assert!(is_gate_error(&format!("ERROR: [youtube] XSbZidsgMfw: {s}")));
    }
}
#[test] fn ignores_benign_failures() {
    for s in ["Video unavailable", "HTTP Error 404: Not Found", "Private video", "failed with http status code: 403 Forbidden"] {
        assert!(!is_gate_error(s));
    }
}
```

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** a lowercase-and-contains check over a `const` phrase list: `["sign in to confirm your age", "requires login", "confirm you’re not a bot", "confirm you're not a bot", "inappropriate for some users", "age-restricted"]`. Note in a doc-comment that the live datacenter string is pinned by the acceptance gate and this list is the tunable source of truth.
- [ ] **Step 4: Run — expect PASS**
- [ ] **Step 5: Commit** (`feat(proxy): yt-dlp gate-error detector`).

### Task 3: Proxied `YoutubeDl` constructor

**Files:**
- Modify: `crack-types/src/http.rs` (add beside `ytdl_for_url`)
- Test: inline in `http.rs`

**Interfaces:**
- Consumes: a caller-built proxied `reqwest::Client` and the authenticated `proxy_url` (from `ProxySession`).
- Produces: `pub fn ytdl_for_url_proxied(client: reqwest::Client, url: &str, proxy_url: &str) -> Result<songbird::input::YoutubeDl<'static>, CrackedError>` — identical guarding to `ytdl_for_url` (reject `url` starting with `-`), but `user_args` begins `["--proxy", proxy_url]` then the existing args, and the passed-in (proxied) client is used.

- [ ] **Step 1: Write failing test**

```rust
#[test] fn proxied_ytdl_passes_proxy_arg_and_url() {
    let c = reqwest::Client::new();
    let yt = ytdl_for_url_proxied(c, "https://www.youtube.com/watch?v=XSbZidsgMfw", "http://u:p@gw:823").unwrap();
    let dbg = format!("{yt:?}");
    assert!(dbg.contains("--proxy") && dbg.contains("http://u:p@gw:823"));
}
#[test] fn proxied_ytdl_rejects_option_like_url() { assert!(ytdl_for_url_proxied(reqwest::Client::new(), "--evil", "http://u:p@gw:823").is_err()); }
```
(If `YoutubeDl`'s `Debug` does not expose `user_args`, assert via a thin accessor or by constructing args through a shared `fn proxied_user_args(proxy_url) -> Vec<String>` that the test checks directly — keep the proxy URL out of any logging path.)

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement.** Reuse `ytdl_for_url`'s arg list; prepend `--proxy`. Do not log `proxy_url`.
- [ ] **Step 4: Run — expect PASS**; clippy clean (the test asserting on a proxy URL is the one place it appears — a fake).
- [ ] **Step 5: Commit** (`feat(http): proxied ytdl_for_url variant`).

### Task 4: `ProxyFallback` Compose wrapper

**Files:**
- Create: `crack-music/src/sources/proxy_fallback.rs`
- Modify: `crack-music/src/sources/mod.rs` (`pub mod proxy_fallback;`)
- Test: inline (reuse the `Scripted` test harness pattern from `sources/retry.rs`)

**Interfaces:**
- Consumes: `is_gate_error` (Task 2), `ProxySession`/`provider()` (Task 1), `ytdl_for_url_proxied` (Task 3).
- Produces:
  - `struct ProxyFallback { inner: Box<dyn Compose>, video_url: String, provider: &'static dyn ProxyProvider, proxied: Option<Box<dyn Compose>> }`
  - `pub fn with_proxy_fallback(input: Input, video_url: String) -> Input` — wraps the lazy source **iff** `provider()` is `Some`; otherwise returns `input` unchanged.
- Behavior: on `aux_metadata`/`create_async`/`create`, run `inner`; if it errors and `is_gate_error(&err.to_string())`, lazily build the proxied source **once** (fresh session; a reqwest client with `session.reqwest_proxy()` + `ytdl_for_url_proxied(client, &video_url, session.ytdl_proxy_url())`), store it in `self.proxied`, and serve this and all later calls from it (one session ⇒ one exit IP for metadata + stream). A non-gate error propagates unchanged. Compose into `retry_refused`-friendly form (both wrappers may apply).

- [ ] **Step 1: Write failing tests** (scripted inner, fake global provider via a test seam or a constructor that takes a `&'static dyn ProxyProvider`):

```rust
#[tokio::test] async fn gate_error_triggers_one_proxied_build_reused() { /* inner gate-fails twice; proxied build counted once; metadata then stream both served by proxied */ }
#[tokio::test] async fn non_gate_error_propagates() { /* inner fails "HTTP Error 404" → error out, no proxied build */ }
#[test] fn no_provider_returns_input_unchanged() { /* with_proxy_fallback when provider() is None → same Input variant, not wrapped */ }
```
(For testability, give `ProxyFallback` a `pub(crate) fn with_provider(inner, video_url, &'static dyn ProxyProvider)` so tests inject a fake provider whose `session()` builds a source from a scripted factory; `with_proxy_fallback` is the production entry that reads the global.)

- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** the wrapper (model it on `sources/retry.rs::RetryRefused`). Build the proxied source through an injectable factory so the real path uses `ytdl_for_url_proxied` and tests use a script. Never log `video_url` with the session.
- [ ] **Step 4: Run — expect PASS**; clippy clean.
- [ ] **Step 5: Commit** (`feat(sources): ProxyFallback wrapper for gated resolves`).

### Task 5: Wire into the YouTube resolve chokepoints

**Files:**
- Modify: `crack-music/src/music/query.rs` (`get_track_source_and_metadata`, `get_track_source`)
- Test: inline / existing query tests

**Interfaces:**
- Consumes: `with_proxy_fallback` (Task 4).
- Change: for the YouTube variants that carry a concrete video URL — `VideoLink(url)` (and the `VideoLink` arm of `get_track_source_and_metadata`) — build the Input and wrap it with `with_proxy_fallback(input, url)`. For `YoutubeSearch`/`Keywords`: after the existing resolve, take the top result's `source_url` (when present) and wrap the produced Input with `with_proxy_fallback(input, source_url)`; if no URL is available, leave the Input unwrapped (no behavior change). Do not change non-YouTube arms.

- [ ] **Step 1: Write failing test** — a resolve-site test asserting that, with a provider configured (inject via the test seam), a `VideoLink` resolve returns a wrapped Input (and, without a provider, an unwrapped one). Keep it hermetic (no live yt-dlp): assert on the wrapping decision, not on network.
- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** the wrapping at the named arms only. Preserve existing error flow when unwrapped.
- [ ] **Step 4: Run — expect PASS**; full `cargo test -p crack-music` green; clippy clean.
- [ ] **Step 5: Commit** (`feat(query): route gated YouTube resolves through the proxy fallback`).

### Task 6: Config surface, docs, inert-by-default integration

**Files:**
- Modify: `.env.example` (add the four `PROXY_FALLBACK_*` keys, commented, with the port-823 and prepaid-cap notes; no real values)
- Modify: a README/docs section describing the feature, activation (set all four), scope (gated-only), and the no-metering/prepaid posture
- Test: an integration-style test (crack-music) that `from_env` + `provider()` is `None` with no env and `Some` with a full fake set, and that `with_proxy_fallback` is a no-op in the `None` case.

- [ ] **Step 1: Write failing test** (`provider_absent_is_total_noop`, `provider_present_wraps`). Use a scoped env-var guard so tests don't leak global env.
- [ ] **Step 2: Run — expect FAIL**
- [ ] **Step 3: Implement** docs + `.env.example` + any thin glue. Confirm nothing logs creds (grep the new modules for `tracing`/`println` touching session/url/password in review).
- [ ] **Step 4: Run — expect PASS**; workspace builds; clippy clean.
- [ ] **Step 5: Commit** (`docs(proxy): env surface + feature docs`).

---

## Controller acceptance gate (live, before version bump)

On a **datacenter** IP (throwaway cloud box), yt-dlp + the built binary:
1. Direct resolve of `XSbZidsgMfw` fails with the gate error — **capture the exact stderr** and confirm `is_gate_error` matches it (tune Task 2's list + add a pinned test if the live phrase differs).
2. With `PROXY_FALLBACK_*` set, the gated track **resolves and streams** through the proxy (bytes served, exit IP is the DataImpulse residential IP, not the box's).
3. A normal track (`dQw4w9WgXcQ`) resolves/streams **direct** — no proxy connection.
4. Unset the creds ⇒ gated track fails as in (1).
5. No credential appears in logs/tracing/errors across the above.

Then: version bump (minor), PR, CI green, merge, tag, deploy to TuneTitan (staging) + verify.
