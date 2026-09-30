# catacombs: website login flow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend catacombs (the owner's Discord OAuth library) with a browser redirect login flow and cookie sessions, so a server-rendered website can use it, and release it as v0.1.0.

**Architecture:** catacombs today serves Discord Activities: JS gets a code from the Discord SDK, POSTs it to `/exchange`, and gets a JWT in the body. We add `GET /login` → Discord → `GET /callback` (with an OAuth `state` CSRF check) that sets the same JWT as an HttpOnly cookie, teach the `AuthenticatedUser` extractor to read that cookie, and make logout clear it. The existing SDK flow keeps working unchanged. First, master's CI is made green, since it has never passed.

**Tech Stack:** Rust 2021, axum 0.8, axum-extra 0.12 (`cookie`), cookie 0.18, reqwest 0.12, jsonwebtoken 10, tokio.

**Spec:** `docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md` in cracktunes, section "catacombs: the website flow (prerequisite PR)".

**Repo:** `github.com/cycle-five/catacombs`, default branch `master`. Clone to `~/projects/catacombs` (persistent, outside any other repo). Work on branch `feat/website-flow`.

## Global Constraints

- MSRV becomes **1.88** (`rust-version = "1.88"`, and the CI MSRV job's toolchain `1.88.0`). Measured: `home` needs 1.88, `jsonwebtoken` 10 needs 1.85.
- Version becomes **0.1.0** in `Cargo.toml` and `CHANGELOG.md`.
- Typed serde only — no `serde_json::json!` or `serde_json::Value` for data we build or inspect, including in tests and mock servers.
- Every test must be seen to fail against a sabotaged implementation (owner's rule). The PR body carries a mutation → caught-by table; a mutation nothing catches is reported, not hidden.
- Assert on what is **sent** to Discord (headers, form fields) and on **request counts**, not only on responses.
- Commit trailer, exactly: `Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)` and no other Co-Authored-By line.
- No `git add -A`; add paths explicitly.
- Formatting: `cargo fmt` (repo has `rustfmt.toml`, max width 100).
- CI must pass under all three feature sets: default (`sqlx-storage,rustls-tls`), `memory-storage,rustls-tls`, `sqlx-storage,native-tls`.

## Review Focus

1. **Open redirect via `return_to`.** `//evil.example`, `/\evil.example`, `https://evil.example`, and CR/LF-bearing values must all fall back to `/` — at login *and* again at callback (the cookie could be planted). Pinned in Task 4 (`safe_return_to` table + login test + callback test).
2. **Callback with no or a wrong `state`** must exchange nothing with Discord (token request count 0), not merely return 400. Pinned in Task 4.
3. **The user cancels on Discord** (`?error=access_denied`, no `code`): must land back on `return_to` logged out, with no token request and no session cookie — not a 400 page. Pinned in Task 4.
4. **Logout with an expired or missing session** must still clear the cookie and return 204 (today it would 401 and leave the cookie). Pinned in Task 5.
5. **Cookie and header both present** (a site that uses both): the cookie wins, deterministically. Pinned in Task 3.

---

### Task 1: Make master green, and drop what nothing uses

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/storage/memory.rs:27-37`
- Modify: `src/lib.rs:1-35` (doc example)
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Create (only if Step 6 finds an advisory in an uncompiled crate): `.cargo/audit.toml`
- Every file `cargo fmt` touches

**Interfaces:**
- Consumes: nothing.
- Produces: a crate whose CI passes; no public API change.

- [ ] **Step 1: Clone and branch**

```bash
cd ~/projects && gh repo clone cycle-five/catacombs && cd catacombs
git switch -c feat/website-flow
```

- [ ] **Step 2: Reproduce the red baseline**

Run each and record the result:
```bash
cargo fmt --all -- --check                     # expect: diffs in src/auth.rs, src/models/*.rs
cargo clippy --no-default-features --features memory-storage --all-targets -- -D warnings
                                               # expect: "multiple fields are never read" at src/storage/memory.rs:29
cargo test                                     # expect: 35 passed
cargo test --no-default-features --features "memory-storage,rustls-tls"   # expect: 39 passed
```

- [ ] **Step 3: Remove unused dependencies**

Verified unused by grep (`oauth2`, `dotenvy`, `tower`, `tower-http`, `tracing-subscriber` appear nowhere in `src/` except a doc comment; axum's `ws` feature and `axum-extra`'s `typed-header` are unused). In `Cargo.toml`:

- delete the lines for `oauth2`, `dotenvy`, `tower`, `tower-http`, `tracing-subscriber` (and their comments),
- change `axum = { version = "0.8", features = ["ws", "macros"] }` to `axum = { version = "0.8", features = ["macros"] }`,
- change `axum-extra = { version = "0.12", features = ["typed-header"] }` to `axum-extra = { version = "0.12", features = ["cookie"] }` (Task 3 uses it),
- add under `# Web framework`: `cookie = "0.18"` (for `cookie::time::Duration`; `cookie` re-exports `time`),
- set `rust-version = "1.88"`,
- add a dev-dependencies block:

```toml
[dev-dependencies]
tokio-test = "0.4"
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

In `src/lib.rs`'s doc example, delete the line `//!     dotenvy::dotenv().ok();`.

Run: `cargo check --all-targets && cargo check --no-default-features --features memory-storage --all-targets`
Expected: both succeed.

- [ ] **Step 4: Fix the memory-storage dead-code error**

`StoredEntitlement` mirrors the `entitlements` table; memory storage writes every field and reads few. In `src/storage/memory.rs`, directly above `struct StoredEntitlement {`, add:

```rust
#[expect(
    dead_code,
    reason = "mirrors the entitlements table row; memory storage writes every field but only tests read them back"
)]
```

(`#[expect]` rather than `#[allow]`: it fails the build if the fields ever become read, so the attribute cannot outlive its reason.)

Run: `cargo clippy --no-default-features --features memory-storage --all-targets -- -D warnings`
Expected: no errors.

- [ ] **Step 5: Format**

Run: `cargo fmt --all && cargo fmt --all -- --check`
Expected: second command prints nothing, exit 0.

- [ ] **Step 6: Find and resolve the audit advisory**

```bash
cargo install cargo-audit --locked   # skip if already installed
cargo audit
```

For each advisory reported, run `cargo tree -i <crate> -e normal` under each of the three CI feature sets. If the crate is **compiled** under any of them: bump the dependency that pulls it (`cargo update -p <crate>` or raise the version in `Cargo.toml`) and re-run. If it appears only in `Cargo.lock` and is **not compiled** in any feature set (sqlx 0.8's lock lists `sqlx-mysql`/`rsa` even for a postgres-only build), create `.cargo/audit.toml`:

```toml
[advisories]
# <ID>: <crate> is in Cargo.lock through <parent> but is not compiled under any
# feature set this crate offers (checked with `cargo tree -i <crate> -e normal`
# under default, memory-storage and native-tls).
ignore = ["<ID>"]
```

with the real ID, crate and parent filled in. Run `cargo audit` again. Expected: exit 0.

- [ ] **Step 7: Fix the CI workflow**

In `.github/workflows/ci.yml`:
- the `msrv` job: rename to `name: MSRV (1.88.0)` and set `toolchain: "1.88.0"`;
- the `security` job: add, at the job level (same indentation as `runs-on`):

```yaml
    permissions:
      contents: read
      checks: write
```

(`rustsec/audit-check` creates a check run; without `checks: write` it failed with "Resource not accessible by integration".)

- [ ] **Step 8: Gate the crates.io publish on the token existing**

The repo has no `CRATES_IO_TOKEN` secret, so today a tag fails at `cargo publish` and the release job (`needs: publish`) is skipped. Publishing stays the owner's later decision; a tag must still make a GitHub release. In `.github/workflows/release.yml`, in the `publish` job, add at job level:

```yaml
    env:
      CRATES_IO_TOKEN: ${{ secrets.CRATES_IO_TOKEN }}
```

and replace the `Publish to crates.io` step with:

```yaml
      - name: Publish to crates.io
        if: env.CRATES_IO_TOKEN != ''
        run: cargo publish --token "$CRATES_IO_TOKEN"

      - name: Note that crates.io publishing is off
        if: env.CRATES_IO_TOKEN == ''
        run: echo "::notice::CRATES_IO_TOKEN is not set, so this tag makes a GitHub release and does not publish to crates.io."
```

- [ ] **Step 9: Verify the whole CI matrix locally**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --features memory-storage --all-targets -- -D warnings
cargo clippy --no-default-features --features "sqlx-storage,native-tls" --all-targets -- -D warnings
cargo test
cargo test --no-default-features --features "memory-storage,rustls-tls"
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features
cargo +1.88.0 check --all-features     # rustup toolchain install 1.88.0 first if missing
cargo audit
```
Expected: all succeed; test counts unchanged (35 and 39).

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src .github
git add .cargo/audit.toml 2>/dev/null || true
git commit -m "ci: make master green; drop unused dependencies; release without a crates.io token

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 2: A configurable Discord API base, and one login path both flows share

**Files:**
- Modify: `src/config.rs` (add `api_base` to `DiscordConfig`, read it in `from_env`)
- Modify: `src/routes/auth.rs` (URLs; extract `complete_login`)
- Create: `tests/common/mod.rs` (mock Discord + state builder)
- Create: `tests/exchange.rs`
- Modify: `.env.example` (document `DISCORD_API_BASE`)

**Interfaces:**
- Consumes: Task 1's dependencies (`tower` util, `http-body-util` in dev).
- Produces:
  - `DiscordConfig.api_base: String` (serde default `"https://discord.com/api/v10"`; `pub fn default_api_base() -> String` in `config.rs`).
  - `pub(crate) struct Login { pub jwt: String, pub discord_access_token: String }` and `pub(crate) async fn complete_login(state: &AppState, code: &str) -> Result<Login, StatusCode>` in `src/routes/auth.rs`.
  - Test helpers in `tests/common/mod.rs`: `spawn_mock_discord() -> (String, Recorded)`, `test_state(api_base: &str) -> Arc<AppState>`, `Recorded { token_requests: Arc<Mutex<Vec<TokenRequest>>>, me_requests: Arc<Mutex<Vec<Option<String>>>> }`, `TokenRequest { authorization: Option<String>, form: HashMap<String, String> }`, constants `CLIENT_ID`, `CLIENT_SECRET`, `REDIRECT_URI`, `JWT_SECRET`, `MOCK_USER_ID`, `MOCK_ACCESS_TOKEN`, and `async fn body_string(resp) -> String`.

- [ ] **Step 1: Add `api_base` to the config**

In `src/config.rs`, add to `DiscordConfig` after `premium_sku_id`:

```rust
    /// Base URL of Discord's REST API. Overridable so tests can point the
    /// token and user calls at a local mock.
    #[serde(default = "default_api_base")]
    pub api_base: String,
```

and near `default_host`:

```rust
/// Discord's REST API, version 10.
pub fn default_api_base() -> String {
    "https://discord.com/api/v10".to_string()
}
```

In `Config::from_env`, inside the `DiscordConfig { .. }` literal add:

```rust
            api_base: std::env::var("DISCORD_API_BASE").unwrap_or_else(|_| default_api_base()),
```

In `.env.example`, under the Discord block add:
```
# Optional: override Discord's API base (tests use a local mock)
# DISCORD_API_BASE=https://discord.com/api/v10
```

- [ ] **Step 2: Use it for every Discord REST call**

In `src/routes/auth.rs` replace each hardcoded base:
- `"https://discord.com/api/v10/oauth2/token"` (twice: exchange and refresh) → `format!("{}/oauth2/token", state.config.discord.api_base)`
- `"https://discord.com/api/v10/oauth2/token/revoke"` → `format!("{}/oauth2/token/revoke", state.config.discord.api_base)`
- the entitlements URL `format!("https://discord.com/api/v10/applications/{}/entitlements...", ...)` → same format with `{}` for `state.config.discord.api_base` prefixed.
- `get_discord_user_info(access_token: &str, http_client: &reqwest::Client)` gains a first parameter `api_base: &str` and uses `format!("{api_base}/users/@me")`; update its callers to pass `&state.config.discord.api_base`.

Run: `grep -n "discord.com/api" src/` — Expected: only `default_api_base` in `config.rs`.

- [ ] **Step 3: Extract `complete_login`**

In `src/routes/auth.rs`, move the whole body of `exchange_code` — from the `exchange_code_with_discord` call through `generate_token` — into:

```rust
/// A completed Discord login: our JWT and Discord's own access token.
pub(crate) struct Login {
    pub jwt: String,
    pub discord_access_token: String,
}

/// Exchange `code` with Discord, upsert the user, refresh entitlements, and
/// mint our JWT. Shared by the SDK flow (`POST /exchange`) and the website
/// flow (`GET /callback`), so the two cannot drift.
pub(crate) async fn complete_login(state: &AppState, code: &str) -> Result<Login, StatusCode> {
    // ... the moved body, unchanged except `&state` → `state` where needed ...
    Ok(Login {
        jwt: jwt_token,
        discord_access_token: discord_token.access_token,
    })
}
```

and reduce `exchange_code` to:

```rust
pub async fn exchange_code(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CodeExchangeRequest>,
) -> Result<Json<TokenResponse>, StatusCode> {
    tracing::info!("Exchanging authorization code for access token");
    let login = complete_login(&state, &payload.code).await?;
    Ok(Json(TokenResponse {
        access_token: login.jwt,
        discord_access_token: Some(login.discord_access_token),
    }))
}
```

Run: `cargo test` — Expected: 35 passed (pure refactor).

- [ ] **Step 4: Write the mock Discord and state helpers**

Create `tests/common/mod.rs`:

```rust
//! A local stand-in for Discord's token and user endpoints that records
//! exactly what catacombs sent it.
#![allow(dead_code)] // each test binary uses a different subset

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Form, Json, Router,
};
use catacombs::{AppState, Config, DiscordConfig, MemoryStorage, SecurityConfig, ServerConfig};
use serde::Serialize;

pub const CLIENT_ID: &str = "client-id-123";
pub const CLIENT_SECRET: &str = "client-secret-456";
pub const REDIRECT_URI: &str = "https://dash.example/auth/callback";
pub const JWT_SECRET: &str = "test-jwt-secret";
pub const MOCK_USER_ID: &str = "112233445566778899";
pub const MOCK_ACCESS_TOKEN: &str = "discord-access-token";
/// A code the mock rejects the way Discord does: 400 invalid_grant.
pub const BAD_CODE: &str = "bad-code";

#[derive(Debug, Clone)]
pub struct TokenRequest {
    pub authorization: Option<String>,
    pub form: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Recorded {
    pub token_requests: Arc<Mutex<Vec<TokenRequest>>>,
    /// The Authorization header of each `/users/@me` request.
    pub me_requests: Arc<Mutex<Vec<Option<String>>>>,
}

#[derive(Serialize)]
struct MockToken<'a> {
    access_token: &'a str,
    token_type: &'a str,
    expires_in: i64,
    refresh_token: &'a str,
    scope: &'a str,
}

#[derive(Serialize)]
struct MockUser<'a> {
    id: &'a str,
    username: &'a str,
    avatar: Option<&'a str>,
    global_name: Option<&'a str>,
    discriminator: Option<&'a str>,
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn token(
    State(rec): State<Recorded>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let rejected = form.get("code").map(String::as_str) == Some(BAD_CODE);
    rec.token_requests.lock().unwrap().push(TokenRequest {
        authorization: header(&headers, "authorization"),
        form,
    });
    if rejected {
        return (StatusCode::BAD_REQUEST, "invalid_grant").into_response();
    }
    Json(MockToken {
        access_token: MOCK_ACCESS_TOKEN,
        token_type: "Bearer",
        expires_in: 604_800,
        refresh_token: "discord-refresh-token",
        scope: "identify",
    })
    .into_response()
}

async fn me(State(rec): State<Recorded>, headers: HeaderMap) -> Json<MockUser<'static>> {
    rec.me_requests
        .lock()
        .unwrap()
        .push(header(&headers, "authorization"));
    Json(MockUser {
        id: MOCK_USER_ID,
        username: "mockuser",
        avatar: None,
        global_name: Some("Mock User"),
        discriminator: None,
    })
}

/// Start the mock on an ephemeral port; returns its base URL.
pub async fn spawn_mock_discord() -> (String, Recorded) {
    let rec = Recorded::default();
    let app = Router::new()
        .route("/oauth2/token", post(token))
        .route("/users/@me", get(me))
        .with_state(rec.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), rec)
}

pub fn test_config(api_base: &str) -> Config {
    Config {
        discord: DiscordConfig {
            client_id: CLIENT_ID.to_string(),
            client_secret: CLIENT_SECRET.to_string(),
            redirect_uri: REDIRECT_URI.to_string(),
            bot_token: "bot-token".to_string(),
            premium_sku_id: None,
            api_base: api_base.to_string(),
        },
        security: SecurityConfig {
            jwt_secret: JWT_SECRET.to_string(),
            encryption_key: "unused-by-memory-storage".to_string(),
        },
        server: ServerConfig::default(),
    }
}

pub fn test_state(api_base: &str) -> Arc<AppState> {
    Arc::new(AppState::new(test_config(api_base), MemoryStorage::new()))
}

pub async fn body_string(resp: Response) -> String {
    use http_body_util::BodyExt;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
```

- [ ] **Step 5: Write the failing exchange tests**

Create `tests/exchange.rs`:

```rust
#![cfg(feature = "memory-storage")]

mod common;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use base64::Engine;
use catacombs::{auth::validate_token, routes::auth_router};
use common::*;
use tower::ServiceExt;

fn app(api_base: &str) -> Router {
    auth_router().with_state(test_state(api_base))
}

fn exchange(code: &str) -> Request<Body> {
    Request::post("/exchange")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(format!(r#"{{"code":"{code}"}}"#)))
        .unwrap()
}

#[tokio::test]
async fn exchange_sends_the_code_with_basic_auth_and_the_configured_redirect() {
    let (base, rec) = spawn_mock_discord().await;

    let resp = app(&base).oneshot(exchange("good-code")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let sent = rec.token_requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "exactly one token request");
    let form = &sent[0].form;
    assert_eq!(form.get("grant_type").map(String::as_str), Some("authorization_code"));
    assert_eq!(form.get("code").map(String::as_str), Some("good-code"));
    assert_eq!(form.get("redirect_uri").map(String::as_str), Some(REDIRECT_URI));
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{CLIENT_ID}:{CLIENT_SECRET}"));
    assert_eq!(sent[0].authorization.as_deref(), Some(format!("Basic {basic}").as_str()));

    let me = rec.me_requests.lock().unwrap().clone();
    assert_eq!(me, vec![Some(format!("Bearer {MOCK_ACCESS_TOKEN}"))]);

    #[derive(serde::Deserialize)]
    struct Body_ {
        access_token: String,
    }
    let body: Body_ = serde_json::from_str(&body_string(resp).await).unwrap();
    let claims = validate_token(&body.access_token, JWT_SECRET).unwrap();
    assert_eq!(claims.sub, MOCK_USER_ID);
}

#[tokio::test]
async fn a_rejected_code_is_401_and_never_fetches_the_user() {
    let (base, rec) = spawn_mock_discord().await;

    let resp = app(&base).oneshot(exchange(BAD_CODE)).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(rec.token_requests.lock().unwrap().len(), 1);
    assert!(rec.me_requests.lock().unwrap().is_empty());
}
```

- [ ] **Step 6: Run the tests; they must pass, then must fail when sabotaged**

Run: `cargo test --no-default-features --features "memory-storage,rustls-tls" --test exchange`
Expected: 2 passed.

Sabotage each, run, confirm the named test fails, revert:
- in `exchange_code_with_discord`, change `("redirect_uri", ...)` to `("redirect_uri", "x")` → `exchange_sends_the_code...` fails.
- in `get_discord_user_info`, drop the `Authorization` header → `exchange_sends_the_code...` fails (me header `None`) — or the handler 500s; either is a failure.
- in `complete_login`, call `get_discord_user_info` before checking the exchange result (e.g. with a dummy token when exchange errors) → `a_rejected_code...` fails.

Record each in a scratch table for the PR body.

- [ ] **Step 7: Commit**

```bash
cargo fmt --all
git add src/config.rs src/routes/auth.rs tests/common/mod.rs tests/exchange.rs .env.example
git commit -m "refactor: one login path for both flows; point Discord calls at a configurable base

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 3: `WebConfig`, and a session cookie the extractor reads first

**Files:**
- Modify: `src/config.rs` (add `WebConfig`, `Config.web`)
- Modify: `src/lib.rs` (re-export `WebConfig`)
- Modify: `src/auth.rs` (session TTL constant; `token_from_parts`; extractor uses it)
- Modify: `tests/common/mod.rs` (`test_config` gains `web`)

**Interfaces:**
- Consumes: Task 2's `tests/common`.
- Produces:
  - `pub struct WebConfig { pub scopes: Vec<String>, pub cookie_name: String, pub secure_cookies: bool }` with `Default` = `["identify"]`, `"catacombs_session"`, `true`; `Config.web: WebConfig` (`#[serde(default)]`); re-exported as `catacombs::WebConfig`.
  - `pub const SESSION_TTL_SECS: i64 = 86_400;` in `src/auth.rs`, used by `generate_token`.
  - `pub(crate) fn token_from_parts(parts: &Parts, cookie_name: &str) -> Option<String>` in `src/auth.rs`.

- [ ] **Step 1: Write the failing extractor tests**

Append to the `tests` module in `src/auth.rs`:

```rust
    fn parts(headers: &[(&str, &str)], uri: &str) -> axum::http::request::Parts {
        let mut req = axum::http::Request::builder().uri(uri);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.body(()).unwrap().into_parts().0
    }

    #[test]
    fn the_session_cookie_wins_over_the_header() {
        let p = parts(
            &[
                ("cookie", "other=1; catacombs_session=from-cookie"),
                ("authorization", "Bearer from-header"),
            ],
            "/?token=from-query",
        );
        assert_eq!(
            token_from_parts(&p, "catacombs_session").as_deref(),
            Some("from-cookie")
        );
    }

    #[test]
    fn the_header_then_the_query_are_fallbacks() {
        let header_only = parts(&[("authorization", "Bearer from-header")], "/?token=q");
        assert_eq!(
            token_from_parts(&header_only, "catacombs_session").as_deref(),
            Some("from-header")
        );
        let query_only = parts(&[], "/?token=from-query");
        assert_eq!(
            token_from_parts(&query_only, "catacombs_session").as_deref(),
            Some("from-query")
        );
        assert_eq!(token_from_parts(&parts(&[], "/"), "catacombs_session"), None);
    }

    #[test]
    fn a_cookie_with_another_name_is_not_a_session() {
        let p = parts(&[("cookie", "catacombs_session_old=x")], "/");
        assert_eq!(token_from_parts(&p, "catacombs_session"), None);
    }

    #[test]
    fn tokens_live_as_long_as_the_session_cookie() {
        let before = chrono::Utc::now().timestamp();
        let token = generate_token(1, "u", TEST_JWT_SECRET).unwrap();
        let exp = validate_token(&token, TEST_JWT_SECRET).unwrap().exp;
        assert!((exp - before - SESSION_TTL_SECS).abs() <= 2);
    }
```

Run: `cargo test --lib auth::tests`
Expected: FAIL to compile — `token_from_parts` and `SESSION_TTL_SECS` not found.

- [ ] **Step 2: Add `WebConfig`**

In `src/config.rs`, add to `Config` after `server`:

```rust
    /// The website flow: `/login`, `/callback` and the session cookie.
    #[serde(default)]
    pub web: WebConfig,
```

and:

```rust
/// Settings for the browser redirect flow and its session cookie.
#[derive(Debug, Clone, Deserialize)]
pub struct WebConfig {
    /// OAuth scopes requested at `/login`.
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    /// Name of the cookie holding the session JWT.
    #[serde(default = "default_cookie_name")]
    pub cookie_name: String,
    /// Mark cookies `Secure`. Browsers accept `Secure` cookies on
    /// `http://localhost`, so this stays on even for local development.
    #[serde(default = "default_true")]
    pub secure_cookies: bool,
}

fn default_scopes() -> Vec<String> {
    vec!["identify".to_string()]
}

fn default_cookie_name() -> String {
    "catacombs_session".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            scopes: default_scopes(),
            cookie_name: default_cookie_name(),
            secure_cookies: true,
        }
    }
}
```

In `Config::from_env`'s final `Ok(Self { .. })` add `web: WebConfig::default(),`. In `src/lib.rs` extend the config re-export: `pub use config::{Config, ConfigError, DiscordConfig, SecurityConfig, ServerConfig, WebConfig};`. In `tests/common/mod.rs`, import `WebConfig` and add `web: WebConfig::default(),` to `test_config`.

- [ ] **Step 3: Implement `token_from_parts` and `SESSION_TTL_SECS`**

In `src/auth.rs` add:

```rust
/// How long a session lasts: the JWT's lifetime and the cookie's `Max-Age`.
pub const SESSION_TTL_SECS: i64 = 24 * 60 * 60;

/// Find the session token: the session cookie first, then an
/// `Authorization: Bearer` header, then a `?token=` query parameter.
pub(crate) fn token_from_parts(parts: &Parts, cookie_name: &str) -> Option<String> {
    let jar = axum_extra::extract::cookie::CookieJar::from_headers(&parts.headers);
    jar.get(cookie_name)
        .map(|c| c.value().to_owned())
        .or_else(|| {
            parts
                .headers
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(String::from)
        })
        .or_else(|| {
            parts
                .uri
                .query()
                .and_then(|q| serde_urlencoded::from_str::<HashMap<String, String>>(q).ok())
                .and_then(|params| params.get("token").cloned())
        })
}
```

In the extractor, replace the whole `let token = parts.headers ... ;` chain with:

```rust
        let token = token_from_parts(parts, &app_state.config.web.cookie_name);
```

and update its doc comment's list to: "1. the session cookie (`config.web.cookie_name`) 2. `Authorization: Bearer <token>` 3. `?token=<token>`". In `generate_token`, replace `chrono::Duration::hours(24)` with `chrono::Duration::seconds(SESSION_TTL_SECS)` and change its doc line to "The token expires after [`SESSION_TTL_SECS`]."

- [ ] **Step 4: Run and sabotage**

Run: `cargo test --lib auth::tests && cargo test --no-default-features --features "memory-storage,rustls-tls"`
Expected: all pass.

Sabotage, confirm failure, revert: swap the cookie and header `or_else` order → `the_session_cookie_wins...` fails; use `starts_with` on the cookie name (e.g. iterate `jar.iter()` and match by prefix) → `a_cookie_with_another_name...` fails; hardcode `hours(24)` with TTL `3600` → `tokens_live_as_long...` fails.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/config.rs src/lib.rs src/auth.rs tests/common/mod.rs
git commit -m "feat: WebConfig, and a session cookie the extractor reads before the header

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 4: `GET /login` and `GET /callback`

**Files:**
- Create: `src/routes/web.rs`
- Modify: `src/routes/mod.rs` (declare `web`, re-export)
- Modify: `src/routes/auth.rs` (`auth_router` adds the two routes; route doc list)
- Create: `tests/web_flow.rs`

**Interfaces:**
- Consumes: `complete_login` (Task 2), `WebConfig` + `SESSION_TTL_SECS` (Task 3), test helpers.
- Produces:
  - `auth_router()` now also serves `GET /login?return_to=<path>` and `GET /callback`.
  - `pub fn safe_return_to(raw: Option<&str>) -> String` in `catacombs::routes::web`.
  - `pub const STATE_COOKIE: &str = "catacombs_oauth_state"`, `pub const RETURN_COOKIE: &str = "catacombs_return_to"`, `pub const AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize"`.
  - `pub(crate) fn session_cookie(web: &WebConfig, jwt: String) -> Cookie<'static>` and `pub(crate) fn removal(name: String) -> Cookie<'static>` (Task 5 uses both).

- [ ] **Step 1: Write the failing `safe_return_to` unit tests**

Create `src/routes/web.rs` with only the tests first:

```rust
//! The website flow: a browser redirect to Discord and back, ending in a
//! session cookie. The SDK flow (`POST /exchange`) is untouched.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_same_site_paths_survive() {
        let cases: &[(Option<&str>, &str)] = &[
            (Some("/g/123"), "/g/123"),
            (Some("/g/123?x=1#y"), "/g/123?x=1#y"),
            (Some("/"), "/"),
            (None, "/"),
            (Some(""), "/"),
            (Some("g/123"), "/"),
            (Some("//evil.example"), "/"),
            (Some("/\\evil.example"), "/"),
            (Some("https://evil.example/"), "/"),
            (Some("/a\r\nSet-Cookie: x=y"), "/"),
            (Some("/a\\b"), "/"),
        ];
        for (raw, want) in cases {
            assert_eq!(safe_return_to(*raw), *want, "input {raw:?}");
        }
    }

    #[test]
    fn constant_time_eq_compares_content_and_length() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
    }
}
```

Add `pub mod web;` to `src/routes/mod.rs`.
Run: `cargo test --lib routes::web` — Expected: FAIL to compile (`safe_return_to`, `constant_time_eq` missing).

- [ ] **Step 2: Implement the module**

Above the tests in `src/routes/web.rs`:

```rust
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Redirect,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{auth::SESSION_TTL_SECS, config::WebConfig, routes::auth::complete_login, AppState};

/// Discord's authorize page (not the REST API base).
pub const AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
/// Holds the OAuth `state` between `/login` and `/callback`.
pub const STATE_COOKIE: &str = "catacombs_oauth_state";
/// Holds where to send the user after `/callback`.
pub const RETURN_COOKIE: &str = "catacombs_return_to";
/// How long a login may take before its state cookie expires.
const LOGIN_TTL_SECS: i64 = 10 * 60;

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    pub return_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// Discord sets this (e.g. `access_denied`) when the user cancels.
    pub error: Option<String>,
}

#[derive(Serialize)]
struct AuthorizeParams<'a> {
    response_type: &'a str,
    client_id: &'a str,
    scope: &'a str,
    state: &'a str,
    redirect_uri: &'a str,
}

/// Where a login may send the user back to: a path on this site, or `/`.
///
/// Rejects anything a browser could read as another origin: absolute URLs,
/// protocol-relative `//host`, and backslashes (`/\host` is `//host` to
/// browsers). Rejects control characters, which could split a header.
pub fn safe_return_to(raw: Option<&str>) -> String {
    match raw {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && !p.chars().any(char::is_control) =>
        {
            p.to_owned()
        }
        _ => "/".to_owned(),
    }
}

/// Compare without an early exit, so timing does not reveal a prefix match.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn short_cookie(web: &WebConfig, name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .http_only(true)
        .secure(web.secure_cookies)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(cookie::time::Duration::seconds(LOGIN_TTL_SECS))
        .build()
}

/// The session cookie holding `jwt`.
pub(crate) fn session_cookie(web: &WebConfig, jwt: String) -> Cookie<'static> {
    Cookie::build((web.cookie_name.clone(), jwt))
        .http_only(true)
        .secure(web.secure_cookies)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(cookie::time::Duration::seconds(SESSION_TTL_SECS))
        .build()
}

/// A cookie that, passed to `CookieJar::remove`, deletes `name` at `/`.
pub(crate) fn removal(name: String) -> Cookie<'static> {
    Cookie::build((name, "")).path("/").build()
}

/// `GET /login?return_to=/path` — start a login.
pub async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<LoginQuery>,
) -> (CookieJar, Redirect) {
    let csrf = random_state();
    let return_to = safe_return_to(q.return_to.as_deref());
    let scope = state.config.web.scopes.join(" ");
    let params = serde_urlencoded::to_string(AuthorizeParams {
        response_type: "code",
        client_id: &state.config.discord.client_id,
        scope: &scope,
        state: &csrf,
        redirect_uri: &state.config.discord.redirect_uri,
    })
    .expect("authorize params are plain strings");
    let web = &state.config.web;
    let jar = jar
        .add(short_cookie(web, STATE_COOKIE, csrf))
        .add(short_cookie(web, RETURN_COOKIE, return_to));
    (jar, Redirect::to(&format!("{AUTHORIZE_URL}?{params}")))
}

/// `GET /callback?code&state` — finish a login.
pub async fn callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(q): Query<CallbackQuery>,
) -> Result<(CookieJar, Redirect), StatusCode> {
    let expected = jar.get(STATE_COOKIE).map(|c| c.value().to_owned());
    // Validated again here: the cookie could have been planted.
    let return_to = safe_return_to(jar.get(RETURN_COOKIE).map(Cookie::value));
    let jar = jar
        .remove(removal(STATE_COOKIE.to_owned()))
        .remove(removal(RETURN_COOKIE.to_owned()));

    if let Some(error) = q.error {
        // The user cancelled on Discord. Back where they were, logged out.
        tracing::info!("Discord login not completed: {error}");
        return Ok((jar, Redirect::to(&return_to)));
    }

    let (Some(code), Some(got), Some(expected)) = (q.code, q.state, expected) else {
        tracing::warn!("callback without code, state or state cookie");
        return Err(StatusCode::BAD_REQUEST);
    };
    if !constant_time_eq(got.as_bytes(), expected.as_bytes()) {
        tracing::warn!("callback state mismatch");
        return Err(StatusCode::BAD_REQUEST);
    }

    let login = complete_login(&state, &code).await?;
    let jar = jar.add(session_cookie(&state.config.web, login.jwt));
    Ok((jar, Redirect::to(&return_to)))
}
```

In `src/routes/auth.rs`, add to `auth_router()`:

```rust
        .route("/login", get(super::web::login))
        .route("/callback", get(super::web::callback))
```

and to its doc list: "- `GET /login` - Start the website flow (redirect to Discord)" and "- `GET /callback` - Finish the website flow (sets the session cookie)". In `src/routes/mod.rs` extend: `pub use web::{callback, login, safe_return_to};`.

Run: `cargo test --lib routes::web` — Expected: 2 passed.

- [ ] **Step 3: Write the flow tests**

Create `tests/web_flow.rs`:

```rust
#![cfg(feature = "memory-storage")]

mod common;

use axum::{
    body::Body,
    http::{header, Request, Response, StatusCode},
    Router,
};
use catacombs::{
    auth::validate_token,
    routes::{auth_router, web::{AUTHORIZE_URL, RETURN_COOKIE, STATE_COOKIE}},
};
use common::*;
use tower::ServiceExt;

fn app(api_base: &str) -> Router {
    auth_router().with_state(test_state(api_base))
}

fn set_cookies<B>(resp: &Response<B>) -> Vec<String> {
    resp.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

fn cookie<'a>(set: &'a [String], name: &str) -> Option<&'a String> {
    set.iter().find(|c| c.starts_with(&format!("{name}=")))
}

fn value(set_cookie: &str) -> &str {
    set_cookie.split(';').next().unwrap().split_once('=').unwrap().1
}

fn location<B>(resp: &Response<B>) -> String {
    resp.headers()[header::LOCATION].to_str().unwrap().to_owned()
}

async fn get(app: Router, uri: &str, cookie: Option<&str>) -> Response<Body> {
    let mut req = Request::get(uri);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap()
}

#[tokio::test]
async fn login_redirects_to_discord_with_a_state_it_also_remembers() {
    let (base, _rec) = spawn_mock_discord().await;

    let resp = get(app(&base), "/login?return_to=/g/42", None).await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    let loc = location(&resp);
    assert!(loc.starts_with(&format!("{AUTHORIZE_URL}?")), "{loc}");
    let query: std::collections::HashMap<String, String> =
        serde_urlencoded::from_str(loc.split_once('?').unwrap().1).unwrap();
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["scope"], "identify");
    assert_eq!(query["redirect_uri"], REDIRECT_URI);

    let set = set_cookies(&resp);
    let state = cookie(&set, STATE_COOKIE).expect("state cookie");
    assert_eq!(value(state), query["state"]);
    assert!(query["state"].len() >= 32, "state has real entropy");
    for c in [state, cookie(&set, RETURN_COOKIE).expect("return cookie")] {
        assert!(c.contains("HttpOnly"), "{c}");
        assert!(c.contains("SameSite=Lax"), "{c}");
        assert!(c.contains("Secure"), "{c}");
    }
    assert_eq!(value(cookie(&set, RETURN_COOKIE).unwrap()), "/g/42");
}

#[tokio::test]
async fn login_will_not_remember_an_offsite_return() {
    let (base, _rec) = spawn_mock_discord().await;
    let resp = get(app(&base), "/login?return_to=//evil.example/x", None).await;
    let set = set_cookies(&resp);
    assert_eq!(value(cookie(&set, RETURN_COOKIE).unwrap()), "/");
}

#[tokio::test]
async fn a_callback_without_a_state_cookie_exchanges_nothing() {
    let (base, rec) = spawn_mock_discord().await;
    let resp = get(app(&base), "/callback?code=good&state=abc", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(rec.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_callback_with_the_wrong_state_exchanges_nothing() {
    let (base, rec) = spawn_mock_discord().await;
    let resp = get(
        app(&base),
        "/callback?code=good&state=abc",
        Some(&format!("{STATE_COOKIE}=abd")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(rec.token_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_good_callback_sets_the_session_and_returns_the_user() {
    let (base, rec) = spawn_mock_discord().await;

    let resp = get(
        app(&base),
        "/callback?code=good&state=abc",
        Some(&format!("{STATE_COOKIE}=abc; {RETURN_COOKIE}=/g/42")),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&resp), "/g/42");
    assert_eq!(rec.token_requests.lock().unwrap().len(), 1);
    let set = set_cookies(&resp);
    let session = cookie(&set, "catacombs_session").expect("session cookie");
    for attr in ["HttpOnly", "SameSite=Lax", "Secure", "Path=/", "Max-Age=86400"] {
        assert!(session.contains(attr), "{attr} missing from {session}");
    }
    let claims = validate_token(value(session), JWT_SECRET).unwrap();
    assert_eq!(claims.sub, MOCK_USER_ID);
    // Both one-shot cookies are deleted.
    for name in [STATE_COOKIE, RETURN_COOKIE] {
        let c = cookie(&set, name).unwrap_or_else(|| panic!("{name} not cleared"));
        assert!(c.contains("Max-Age=0"), "{c}");
    }
}

#[tokio::test]
async fn a_planted_offsite_return_cookie_is_ignored_at_callback() {
    let (base, _rec) = spawn_mock_discord().await;
    let resp = get(
        app(&base),
        "/callback?code=good&state=abc",
        Some(&format!("{STATE_COOKIE}=abc; {RETURN_COOKIE}=https://evil.example")),
    )
    .await;
    assert_eq!(location(&resp), "/");
}

#[tokio::test]
async fn cancelling_on_discord_returns_the_user_logged_out() {
    let (base, rec) = spawn_mock_discord().await;
    let resp = get(
        app(&base),
        "/callback?error=access_denied&state=abc",
        Some(&format!("{STATE_COOKIE}=abc; {RETURN_COOKIE}=/g/42")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&resp), "/g/42");
    assert!(rec.token_requests.lock().unwrap().is_empty());
    assert!(cookie(&set_cookies(&resp), "catacombs_session").is_none());
}

#[tokio::test]
async fn the_session_cookie_authenticates_me() {
    let (base, _rec) = spawn_mock_discord().await;
    let app = app(&base);
    let login = get(
        app.clone(),
        "/callback?code=good&state=abc",
        Some(&format!("{STATE_COOKIE}=abc")),
    )
    .await;
    let set = set_cookies(&login);
    let jwt = value(cookie(&set, "catacombs_session").unwrap()).to_owned();

    let me = get(app, "/me", Some(&format!("catacombs_session={jwt}"))).await;

    assert_eq!(me.status(), StatusCode::OK);
    assert!(body_string(me).await.contains(MOCK_USER_ID));
}
```

Note: `the_session_cookie_authenticates_me` relies on `app.clone()` sharing one `Arc<AppState>` — `with_state` was called once in `app()`, so the clones share the same `MemoryStorage`.

- [ ] **Step 4: Run, then sabotage**

Run: `cargo test --no-default-features --features "memory-storage,rustls-tls"`
Expected: all pass, including 8 in `web_flow`.

Sabotage each, confirm the named test fails, revert:
| Mutation | Must fail |
|---|---|
| `safe_return_to`: drop `!p.starts_with("//")` | `only_same_site_paths_survive`, `login_will_not_remember_an_offsite_return` |
| `callback`: use the raw cookie value instead of `safe_return_to(...)` | `a_planted_offsite_return_cookie_is_ignored_at_callback` |
| `callback`: skip the `constant_time_eq` check | `a_callback_with_the_wrong_state_exchanges_nothing` |
| `callback`: treat a missing state cookie as `""` and compare | `a_callback_without_a_state_cookie_exchanges_nothing` |
| `callback`: move the `error` branch after the code/state check | `cancelling_on_discord_returns_the_user_logged_out` |
| `session_cookie`: drop `.http_only(true)` | `a_good_callback_sets_the_session...` |
| `callback`: stop removing `RETURN_COOKIE` | `a_good_callback_sets_the_session...` |
| `login`: generate an 8-byte state | `login_redirects_to_discord...` |

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add src/routes/web.rs src/routes/mod.rs src/routes/auth.rs tests/web_flow.rs
git commit -m "feat: the website flow -- /login and /callback with a state check and a session cookie

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
```

---

### Task 5: Logout clears the cookie; docs; v0.1.0

**Files:**
- Modify: `src/routes/auth.rs` (`logout`)
- Modify: `tests/web_flow.rs` (logout tests)
- Modify: `README.md`, `CHANGELOG.md`, `Cargo.toml` (version)

**Interfaces:**
- Consumes: `removal` (Task 4).
- Produces: `POST /logout` always returns 204 and a cookie deletion; clears stored tokens when the caller is authenticated.

- [ ] **Step 1: Write the failing logout tests**

Append to `tests/web_flow.rs`:

```rust
async fn post(app: Router, uri: &str, cookie: Option<&str>) -> Response<Body> {
    let mut req = Request::post(uri);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap()
}

#[tokio::test]
async fn logout_clears_the_session_cookie_and_the_stored_tokens() {
    let (base, _rec) = spawn_mock_discord().await;
    let state = test_state(&base);
    let app = auth_router().with_state(state.clone());
    let login = get(
        app.clone(),
        "/callback?code=good&state=abc",
        Some(&format!("{STATE_COOKIE}=abc")),
    )
    .await;
    let jwt = value(cookie(&set_cookies(&login), "catacombs_session").unwrap()).to_owned();
    let user_id: i64 = MOCK_USER_ID.parse().unwrap();
    let before = state.storage.get_user(user_id, "k").await.unwrap().unwrap();
    assert!(before.refresh_token.is_some(), "login stored a refresh token");

    let resp = post(app, "/logout", Some(&format!("catacombs_session={jwt}"))).await;

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let set = set_cookies(&resp);
    let cleared = cookie(&set, "catacombs_session").expect("session cookie cleared");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    let after = state.storage.get_user(user_id, "k").await.unwrap().unwrap();
    assert!(after.refresh_token.is_none());
}

#[tokio::test]
async fn logout_without_a_valid_session_still_clears_the_cookie() {
    let (base, _rec) = spawn_mock_discord().await;
    let resp = post(app(&base), "/logout", Some("catacombs_session=expired.or.garbage")).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let set = set_cookies(&resp);
    assert!(cookie(&set, "catacombs_session")
        .expect("cleared")
        .contains("Max-Age=0"));
}
```

Add `use catacombs::UserStorage;` to the file's imports (`get_user` is a trait method). Check `User`'s refresh-token field name in `src/models/user.rs` before running; if it is not `refresh_token`, use the real name. `get_user`'s second argument is the encryption key, which memory storage ignores.

Run: `cargo test --no-default-features --features "memory-storage,rustls-tls" --test web_flow logout`
Expected: FAIL — no `Set-Cookie` on the first; 401 on the second.

- [ ] **Step 2: Rewrite `logout`**

In `src/routes/auth.rs` replace `logout` with:

```rust
/// Log out: always delete the session cookie; if the caller is
/// authenticated, also clear their stored Discord tokens.
///
/// A missing or expired session is not an error here -- the cookie still has
/// to go, or the browser keeps sending a dead token.
pub async fn logout(
    user: Result<AuthenticatedUser, StatusCode>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> (CookieJar, StatusCode) {
    if let Ok(user) = user {
        tracing::info!("Logging out user: {} ({})", user.username, user.user_id);
        if let Err(e) = state.storage.clear_user_tokens(user.user_id).await {
            tracing::error!("Failed to clear tokens for logout: {}", e);
        }
    }
    let jar = jar.remove(super::web::removal(state.config.web.cookie_name.clone()));
    (jar, StatusCode::NO_CONTENT)
}
```

adding `use axum_extra::extract::cookie::CookieJar;` to the imports.

Run: `cargo test --no-default-features --features "memory-storage,rustls-tls"` — Expected: all pass.
Sabotage: remove the `jar.remove(...)` → both logout tests fail; drop the `clear_user_tokens` call → the first fails. Revert.

- [ ] **Step 3: Docs and version**

- `Cargo.toml`: `version = "0.1.0"`.
- `CHANGELOG.md`: under `## [Unreleased]` insert a `## [0.1.0] - 2026-09-30` section:

```markdown
### Added

- Website flow: `GET /login?return_to=` redirects to Discord with an OAuth
  `state`; `GET /callback` checks it, completes the login and sets an HttpOnly,
  `SameSite=Lax` session cookie holding the JWT, then returns the user to
  `return_to` (same-site paths only).
- `WebConfig` (`Config.web`): scopes, session cookie name, `Secure` flag.
- `DiscordConfig.api_base` (`DISCORD_API_BASE`), so tests can use a local mock.

### Changed

- The `AuthenticatedUser` extractor reads the session cookie first, then the
  `Authorization` header, then `?token=`.
- `POST /logout` always returns 204 and deletes the session cookie; it clears
  stored tokens only when the caller is authenticated (it used to 401).
- MSRV is 1.88. Unused dependencies (`oauth2`, `dotenvy`, `tower`,
  `tower-http`, `tracing-subscriber`, axum `ws`) are gone.
- A tag without a `CRATES_IO_TOKEN` secret makes a GitHub release and skips
  crates.io.
```

and add the link line `[0.1.0]: https://github.com/cycle-five/catacombs/compare/v0.0.1...v0.1.0` (there is no `v0.0.1` tag; use `https://github.com/cycle-five/catacombs/releases/tag/v0.1.0` instead and fix the `[Unreleased]` compare link to `v0.1.0...HEAD`).
- `README.md`:
  - Installation: replace the crates.io snippet with
    ```toml
    catacombs = { git = "https://github.com/cycle-five/catacombs", tag = "v0.1.0" }
    ```
    and a line: "catacombs is not published to crates.io yet."
  - Remove the crates.io and docs.rs badges (they point at nothing).
  - Add a "Website flow" section: mount `auth_router()` at `/auth`; set `DISCORD_REDIRECT_URI` to `https://<site>/auth/callback` and register it in the Discord developer portal; link to `/auth/login?return_to=/where/next`; protect handlers with `AuthenticatedUser`; `POST /auth/logout` to sign out; the session is a `SameSite=Lax` HttpOnly cookie, so state-changing endpoints should additionally require a JSON content type or check `Origin`.

- [ ] **Step 4: Full verification (the CI matrix, again)**

Run every command from Task 1 Step 9. Expected: all green. `cargo doc` must not warn about the new items.

- [ ] **Step 5: Commit, push, PR**

```bash
cargo fmt --all
git add src/routes/auth.rs tests/web_flow.rs README.md CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "feat: logout clears the session cookie; v0.1.0

Co-Authored-By: Claude & Lothrop (cycle.five@proton.me)"
git push -u origin feat/website-flow
gh pr create --title "v0.1.0: a website login flow -- /login, /callback and a session cookie" --body-file <body>
```

The PR body: design and change (why an Activity-shaped library needed a website flow; the cracktunes spec path), the green-CI baseline and what was wrong, the mutation → caught-by table from Tasks 2–5, and the release-workflow change. End with the attribution lines from the session's system reminder.

- [ ] **Step 6: Iterate to green, merge, tag**

Fix CI failures forward. Merge. Then:

```bash
git switch master && git pull --ff-only
git tag -s v0.1.0 -m "v0.1.0: website login flow"
git push origin v0.1.0
gh run watch -R cycle-five/catacombs   # the Release workflow
gh release view v0.1.0 -R cycle-five/catacombs
```

Expected: the Release workflow's publish step is skipped with the notice, and `github-release` creates the v0.1.0 release. **Do not run `gh release create`** — the workflow owns the release (case A).
