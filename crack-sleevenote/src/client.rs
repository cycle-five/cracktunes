//! The async HTTP client.

use crate::error::{Error, ErrorDetail, Result};
use crate::model::{Album, Playlist, Track};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;
use url::Url;

/// Base URL used when none is configured.
///
/// sleevenote's own `PORT` defaults to 3000 and it ships no authentication of
/// its own, so loopback is the only default that is safe to assume.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:3000";

/// Environment variable read by [`ClientBuilder::from_env`] for the base URL.
pub const BASE_URL_ENV: &str = "SLEEVENOTE_BASE_URL";

/// Environment variable read by [`ClientBuilder::from_env`] for the timeout,
/// in whole seconds.
pub const TIMEOUT_SECS_ENV: &str = "SLEEVENOTE_TIMEOUT_SECS";

/// Default whole-request timeout: three minutes.
///
/// This is deliberately enormous. sleevenote is designed to be called
/// synchronously while it drives a real browser: a cold produce is 8-15s, its
/// own `PRODUCE_BUDGET_MS` defaults to 150s, and a large playlist that has to
/// be scrolled can approach that. A conventional 5- or 30-second HTTP timeout
/// would convert ordinary cold-cache operation into transport errors and hide
/// the service's own 504, which is the signal that actually means "too slow".
/// A warm hit still returns in single-digit milliseconds.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

/// Default re-requests after an `extraction_silent` failure.
///
/// One, not more. Measured in production: ~14% of requests fail this way and
/// every failing id succeeded on a later attempt, so a single retry takes
/// user-visible failure from roughly 1-in-7 to roughly 1-in-50. A second retry
/// would buy about another 1%, against a request that costs 4-6 seconds -- the
/// bot is holding a Discord interaction open the whole time.
pub const DEFAULT_EXTRACTION_SILENT_RETRIES: u8 = 1;

/// The id pattern the service enforces, mirrored client-side.
pub const ID_PATTERN: &str = "^[A-Za-z0-9]{1,64}$";

/// The response header sleevenote reports cache disposition in.
pub const CACHE_HEADER: &str = "x-cache";

/// How the service answered: from cache or by producing.
///
/// Informational -- correctness never depends on it -- but worth logging, since
/// a sudden collapse in `Fresh` is what a cache regression looks like from the
/// outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    /// Served from cache, within TTL.
    Fresh,
    /// Served from cache past its TTL while a refresh happens elsewhere.
    Stale,
    /// Not cached; the service produced this response.
    Miss,
    /// Served from the negative cache: a previously confirmed-absent id.
    Negative,
}

impl CacheStatus {
    /// The wire spelling.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            CacheStatus::Fresh => "fresh",
            CacheStatus::Stale => "stale",
            CacheStatus::Miss => "miss",
            CacheStatus::Negative => "negative",
        }
    }
}

impl fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CacheStatus {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "fresh" => Ok(CacheStatus::Fresh),
            "stale" => Ok(CacheStatus::Stale),
            "miss" => Ok(CacheStatus::Miss),
            "negative" => Ok(CacheStatus::Negative),
            _ => Err(()),
        }
    }
}

/// The answer from `GET /health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Health {
    /// 200, body `sleevenote ok`.
    Ok,
    /// 503, body `sleevenote unhealthy`.
    Unhealthy,
}

impl Health {
    /// Parse the exact status/body pair the service documents. Anything else is
    /// not a health answer -- most likely something else replied.
    fn parse(status: u16, body: &str) -> Option<Self> {
        match (status, body.trim()) {
            (200, "sleevenote ok") => Some(Health::Ok),
            (503, "sleevenote unhealthy") => Some(Health::Unhealthy),
            _ => None,
        }
    }
}

/// Which entity endpoint a request targets.
#[derive(Debug, Clone, Copy)]
enum Endpoint {
    Track,
    Album,
    Playlist,
}

impl Endpoint {
    fn segment(self) -> &'static str {
        match self {
            Endpoint::Track => "track",
            Endpoint::Album => "album",
            Endpoint::Playlist => "playlist",
        }
    }

    /// Whether this endpoint returns a listing, and so can come back partial.
    ///
    /// A track has no listing to be short of, and the service refuses to apply
    /// its completeness predicate to one -- sending `partial=allow` there would
    /// be noise at best.
    fn is_listing(self) -> bool {
        matches!(self, Endpoint::Album | Endpoint::Playlist)
    }
}

/// Configuration for a [`Client`].
///
/// ```
/// use crack_sleevenote::ClientBuilder;
/// use std::time::Duration;
///
/// let client = ClientBuilder::new()
///     .base_url("http://sleevenote.internal:3000")
///     .timeout(Duration::from_secs(240))
///     .build()
///     .expect("valid base url");
/// # let _ = client;
/// ```
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: String,
    timeout: Duration,
    user_agent: String,
    http: Option<reqwest::Client>,
    allow_partial_listings: bool,
    extraction_silent_retries: u8,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            user_agent: default_user_agent(),
            http: None,
            // Strict, matching the service's own default: a caller that has
            // not thought about partial listings must not be handed one.
            allow_partial_listings: false,
            // One retry, on `extraction_silent` alone. See the builder method.
            extraction_silent_retries: DEFAULT_EXTRACTION_SILENT_RETRIES,
        }
    }
}

fn default_user_agent() -> String {
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")).to_owned()
}

impl ClientBuilder {
    /// A builder with [`DEFAULT_BASE_URL`] and [`DEFAULT_TIMEOUT`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A builder seeded from [`BASE_URL_ENV`] and [`TIMEOUT_SECS_ENV`].
    ///
    /// Unset, empty, or unparseable values leave the corresponding default in
    /// place; a bad base URL is reported by [`ClientBuilder::build`], not here.
    #[must_use]
    pub fn from_env() -> Self {
        let mut builder = Self::new();
        if let Some(url) = std::env::var(BASE_URL_ENV).ok().filter(|s| !s.is_empty()) {
            builder.base_url = url;
        }
        if let Some(secs) = std::env::var(TIMEOUT_SECS_ENV)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
        {
            builder.timeout = Duration::from_secs(secs);
        }
        builder
    }

    /// Point the client at a sleevenote deployment. Trailing slash optional.
    #[must_use]
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Override the per-request timeout. See [`DEFAULT_TIMEOUT`] before
    /// shortening it.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the `User-Agent`.
    #[must_use]
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Accept listings the service could only partly recover.
    ///
    /// Sends `?partial=allow` on album and playlist requests, never on tracks
    /// -- a track has no listing, and the service ignores the flag there.
    ///
    /// Off by default. With it off, a listing the service could not fully
    /// extract is a [`ErrorCode::ExtractionIncomplete`] 502 and the recovered
    /// tracks are discarded; with it on, they arrive with `complete: false` and
    /// the caller decides what to say about the shortfall. See
    /// [`Album::shortfall`](crate::Album::shortfall).
    #[must_use]
    pub fn allow_partial_listings(mut self, allow: bool) -> Self {
        self.allow_partial_listings = allow;
        self
    }

    /// How many times to re-request after an `extraction_silent` failure.
    ///
    /// Defaults to [`DEFAULT_EXTRACTION_SILENT_RETRIES`]. Set `0` to disable.
    ///
    /// 🔑 **Only `extraction_silent` is retried, and deliberately so.** It is
    /// the one code in the taxonomy measured as transient: production logs
    /// show ~14% of requests failing this way and **every** failing id
    /// succeeding on a later attempt. `extraction_empty` reads almost
    /// identically and is NOT retried -- it means extraction genuinely stopped
    /// matching Spotify's page, which a retry cannot fix and a deploy can.
    /// Widening this to other codes would re-merge the taxonomy the error
    /// module exists to keep apart.
    #[must_use]
    pub fn extraction_silent_retries(mut self, retries: u8) -> Self {
        self.extraction_silent_retries = retries;
        self
    }

    /// Reuse an existing [`reqwest::Client`] and its connection pool.
    ///
    /// The timeout configured here still applies: it is set per request, so it
    /// overrides whatever the supplied client carries. The `User-Agent` set
    /// here does not -- that belongs to the supplied client.
    #[must_use]
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = Some(http);
        self
    }

    /// Validate the base URL and build the client.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidBaseUrl`] if the base URL does not parse, is not
    /// `http`/`https`, or cannot be used as a base for path joining.
    /// [`Error::Transport`] if reqwest cannot build its own client.
    pub fn build(self) -> Result<Client> {
        let base_url = Url::parse(&self.base_url).map_err(|e| Error::InvalidBaseUrl {
            url: self.base_url.clone(),
            message: e.to_string(),
        })?;
        if base_url.cannot_be_a_base() {
            return Err(Error::InvalidBaseUrl {
                url: self.base_url.clone(),
                message: "cannot be used as a base for a path".to_owned(),
            });
        }
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(Error::InvalidBaseUrl {
                url: self.base_url.clone(),
                message: format!("scheme must be http or https, got `{}`", base_url.scheme()),
            });
        }

        let http = match self.http {
            Some(http) => http,
            None => reqwest::Client::builder()
                .user_agent(self.user_agent)
                .build()?,
        };

        Ok(Client {
            http,
            base_url,
            timeout: self.timeout,
            allow_partial_listings: self.allow_partial_listings,
            extraction_silent_retries: self.extraction_silent_retries,
        })
    }
}

/// An async client for one sleevenote deployment.
///
/// Cheap to clone: the inner [`reqwest::Client`] shares its connection pool.
///
/// ```no_run
/// # async fn demo() -> Result<(), crack_sleevenote::Error> {
/// let client = crack_sleevenote::Client::new()?;
/// let track = client.track("2h8wlptrZOjSnZQKoNnLge").await?;
/// println!("{} by {}", track.name, track.artists[0].name);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: Url,
    timeout: Duration,
    allow_partial_listings: bool,
    extraction_silent_retries: u8,
}

impl Client {
    /// A client against [`DEFAULT_BASE_URL`] with [`DEFAULT_TIMEOUT`].
    ///
    /// # Errors
    ///
    /// See [`ClientBuilder::build`].
    pub fn new() -> Result<Self> {
        ClientBuilder::new().build()
    }

    /// A client configured from the environment. See [`ClientBuilder::from_env`].
    ///
    /// # Errors
    ///
    /// See [`ClientBuilder::build`].
    pub fn from_env() -> Result<Self> {
        ClientBuilder::from_env().build()
    }

    /// Start configuring a client.
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// The deployment this client talks to.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// The per-request timeout in force.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// `GET /v1/track/:id`.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn track(&self, id: &str) -> Result<Track> {
        self.fetch(Endpoint::Track, id)
            .await
            .map(|(track, _)| track)
    }

    /// `GET /v1/track/:id`, also reporting the `X-Cache` disposition.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn track_with_cache(&self, id: &str) -> Result<(Track, Option<CacheStatus>)> {
        self.fetch(Endpoint::Track, id).await
    }

    /// `GET /v1/album/:id`.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn album(&self, id: &str) -> Result<Album> {
        self.fetch(Endpoint::Album, id)
            .await
            .map(|(album, _)| album)
    }

    /// `GET /v1/album/:id`, also reporting the `X-Cache` disposition.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn album_with_cache(&self, id: &str) -> Result<(Album, Option<CacheStatus>)> {
        self.fetch(Endpoint::Album, id).await
    }

    /// `GET /v1/playlist/:id`.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn playlist(&self, id: &str) -> Result<Playlist> {
        self.fetch(Endpoint::Playlist, id)
            .await
            .map(|(playlist, _)| playlist)
    }

    /// `GET /v1/playlist/:id`, also reporting the `X-Cache` disposition.
    ///
    /// # Errors
    ///
    /// Any [`Error`]; see that type for the taxonomy.
    pub async fn playlist_with_cache(&self, id: &str) -> Result<(Playlist, Option<CacheStatus>)> {
        self.fetch(Endpoint::Playlist, id).await
    }

    /// `GET /health`.
    ///
    /// # Errors
    ///
    /// [`Error::Transport`] if the request fails, or
    /// [`Error::UnexpectedStatus`] if something other than sleevenote's exact
    /// documented status/body pair answered.
    pub async fn health(&self) -> Result<Health> {
        let url = self.join(&["health"])?;
        let response = self.http.get(url).timeout(self.timeout).send().await?;
        let status = response.status().as_u16();
        let body = response.text().await?;
        match Health::parse(status, &body) {
            Some(health) => Ok(health),
            None => Err(Error::UnexpectedStatus { status, body }),
        }
    }

    /// Issue one entity request and interpret the response.
    /// One attempt, plus up to [`Client::extraction_silent_retries`] more on
    /// `extraction_silent`.
    ///
    /// 🪤 The retry is here, at the single chokepoint every entity method
    /// funnels through, rather than in each of `track`/`album`/`playlist`.
    /// Putting it in the three public methods would mean a fourth endpoint
    /// added later silently does not retry.
    async fn fetch<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        id: &str,
    ) -> Result<(T, Option<CacheStatus>)> {
        // Validate ONCE, outside the loop: a bad id is not transient and
        // re-checking it per attempt only obscures where the error came from.
        validate_id(id)?;

        let mut attempt = 0u8;
        loop {
            let result = self.fetch_once(endpoint, id).await;
            let Err(err) = result else { return result };

            // Only `extraction_silent`, and only while attempts remain. Every
            // other error -- including `extraction_empty`, which reads almost
            // the same -- returns untouched.
            if !matches!(err, Error::ExtractionSilent(_))
                || attempt >= self.extraction_silent_retries
            {
                return Err(err);
            }
            attempt += 1;
            tracing::warn!(
                %id,
                attempt,
                retries = self.extraction_silent_retries,
                "sleevenote returned extraction_silent; retrying: {err}"
            );
        }
    }

    /// A single request, with no retry policy of its own.
    async fn fetch_once<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        id: &str,
    ) -> Result<(T, Option<CacheStatus>)> {
        let url = self.entity_url(endpoint, id)?;
        tracing::debug!(%url, "sleevenote request");

        let response = self.http.get(url).timeout(self.timeout).send().await?;
        let status = response.status().as_u16();
        let cache = response
            .headers()
            .get(CACHE_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<CacheStatus>().ok());
        let body = response.text().await?;

        if !(200..300).contains(&status) {
            return Err(Error::from_response(status, &body));
        }

        match serde_json::from_str::<T>(&body) {
            Ok(value) => Ok((value, cache)),
            Err(source) => Err(Error::Decode { source, body }),
        }
    }

    /// The full request URL for one entity, including the partial opt-in.
    ///
    /// Split out from [`Client::fetch`] so the query can be asserted without a
    /// live service: the opt-in is a wire detail, and a test that has to make a
    /// request to see it is a test nobody runs.
    fn entity_url(&self, endpoint: Endpoint, id: &str) -> Result<Url> {
        let mut url = self.join(&["v1", endpoint.segment(), id])?;
        if self.allow_partial_listings && endpoint.is_listing() {
            // The service tests `req.query.partial === 'allow'` exactly; no
            // other value opts in.
            url.query_pairs_mut().append_pair("partial", "allow");
        }
        Ok(url)
    }

    /// Append path segments to the base URL, percent-encoding each one.
    fn join(&self, segments: &[&str]) -> Result<Url> {
        let mut url = self.base_url.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|()| Error::InvalidBaseUrl {
                    url: self.base_url.to_string(),
                    message: "cannot be used as a base for a path".to_owned(),
                })?;
            // A base URL written with a trailing slash would otherwise produce
            // an empty segment, i.e. `http://host//v1/track/x`.
            path.pop_if_empty().extend(segments);
        }
        Ok(url)
    }
}

/// Reject an id client-side using the service's own pattern.
///
/// This is not just a saved round trip. The id goes into the request path, and
/// refusing anything outside `[A-Za-z0-9]` here means no caller-supplied string
/// can ever add a path segment or a query to the URL we build.
fn validate_id(id: &str) -> Result<()> {
    if !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Ok(());
    }
    Err(Error::InvalidId(ErrorDetail {
        status: 400,
        id: id.to_owned(),
        message: format!("id must match {ID_PATTERN}"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ids_that_the_service_would_reject() {
        for bad in ["", "not!valid", "a/b", "../etc", "a b", &"x".repeat(65)] {
            let err = validate_id(bad).expect_err("should reject");
            assert!(matches!(err, Error::InvalidId(_)), "{bad:?} -> {err}");
        }
    }

    #[test]
    fn accepts_ids_that_the_service_would_accept() {
        for good in ["2h8wlptrZOjSnZQKoNnLge", "a", &"9".repeat(64)] {
            validate_id(good).expect("should accept");
        }
    }

    #[test]
    fn a_partial_opt_in_reaches_listings_and_never_tracks() {
        let client = ClientBuilder::new()
            .base_url("http://sleevenote.test:3000")
            .allow_partial_listings(true)
            .build()
            .unwrap();

        // The service matches `partial === 'allow'` exactly.
        assert_eq!(
            client.entity_url(Endpoint::Album, "abc").unwrap().as_str(),
            "http://sleevenote.test:3000/v1/album/abc?partial=allow"
        );
        assert_eq!(
            client
                .entity_url(Endpoint::Playlist, "abc")
                .unwrap()
                .as_str(),
            "http://sleevenote.test:3000/v1/playlist/abc?partial=allow"
        );
        // A track has no listing; the flag must not leak onto it.
        assert_eq!(
            client.entity_url(Endpoint::Track, "abc").unwrap().as_str(),
            "http://sleevenote.test:3000/v1/track/abc"
        );
    }

    #[test]
    fn the_default_client_stays_strict() {
        // Opting in is the caller's decision, and silence means strict -- the
        // same default the service itself applies.
        let client = ClientBuilder::new()
            .base_url("http://sleevenote.test:3000")
            .build()
            .unwrap();
        for endpoint in [Endpoint::Track, Endpoint::Album, Endpoint::Playlist] {
            let url = client.entity_url(endpoint, "abc").unwrap();
            assert_eq!(url.query(), None, "{endpoint:?} carried a query");
        }
    }

    #[test]
    fn builds_endpoint_urls_without_doubled_slashes() {
        for base in ["http://host:3000", "http://host:3000/"] {
            let client = ClientBuilder::new().base_url(base).build().unwrap();
            let url = client.join(&["v1", "track", "abc"]).unwrap();
            assert_eq!(url.as_str(), "http://host:3000/v1/track/abc");
        }
    }

    #[test]
    fn preserves_a_base_path_prefix() {
        let client = ClientBuilder::new()
            .base_url("https://gw.example/sleevenote")
            .build()
            .unwrap();
        let url = client.join(&["v1", "album", "abc"]).unwrap();
        assert_eq!(url.as_str(), "https://gw.example/sleevenote/v1/album/abc");
    }

    #[test]
    fn rejects_a_non_http_base_url() {
        let err = ClientBuilder::new()
            .base_url("ftp://host/")
            .build()
            .expect_err("should reject");
        assert!(matches!(err, Error::InvalidBaseUrl { .. }), "{err}");
    }

    #[test]
    fn defaults_are_generous_enough_for_a_cold_produce() {
        // sleevenote's own PRODUCE_BUDGET_MS default is 150s; a client timeout
        // at or below that would pre-empt the service's own 504.
        assert!(DEFAULT_TIMEOUT > Duration::from_secs(150));
        assert_eq!(Client::new().unwrap().timeout(), DEFAULT_TIMEOUT);
    }

    #[test]
    fn parses_the_documented_health_answers() {
        assert_eq!(Health::parse(200, "sleevenote ok\n"), Some(Health::Ok));
        assert_eq!(
            Health::parse(503, "sleevenote unhealthy"),
            Some(Health::Unhealthy)
        );
        assert_eq!(Health::parse(200, "ok"), None);
        assert_eq!(Health::parse(503, "sleevenote ok"), None);
    }

    #[test]
    fn parses_the_documented_cache_dispositions() {
        assert_eq!("fresh".parse(), Ok(CacheStatus::Fresh));
        assert_eq!("stale".parse(), Ok(CacheStatus::Stale));
        assert_eq!("miss".parse(), Ok(CacheStatus::Miss));
        assert_eq!("negative".parse(), Ok(CacheStatus::Negative));
        assert_eq!("FRESH".parse::<CacheStatus>(), Err(()));
    }
}

#[cfg(test)]
mod retry_tests {
    //! The `extraction_silent` retry, proven by counting REQUESTS rather than
    //! by inspecting the returned value. A retry that quietly never re-requests
    //! still returns the right error, so the request count is the only thing
    //! that distinguishes "retried and both failed" from "never retried".
    //!
    //! A hand-rolled listener rather than a mock-server dev-dependency: the
    //! crate's tests are otherwise offline and unit-level, and the whole
    //! contract under test is "how many times did it ask".
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves `bodies` in order, one per connection, then 200s forever.
    /// Returns the base url and a live count of requests served.
    async fn serve(bodies: Vec<(u16, &'static str)>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let n = served.fetch_add(1, Ordering::SeqCst);
                let (status, body) = bodies
                    .get(n)
                    .copied()
                    .unwrap_or((200, r#"{"id":"x","type":"track","name":"ok","artists":[],"album":null,"url":"u","durationMs":1}"#));
                // Read the request line so the client is not writing into a
                // socket nobody drained.
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), hits)
    }

    const SILENT: (u16, &str) = (
        502,
        r#"{"error":"extraction_silent","id":"x","message":"nothing matched"}"#,
    );
    const EMPTY: (u16, &str) = (
        502,
        r#"{"error":"extraction_empty","id":"x","message":"zero tracks"}"#,
    );

    #[tokio::test]
    async fn extraction_silent_is_retried_and_can_succeed() {
        let (base, hits) = serve(vec![SILENT]).await;
        let client = ClientBuilder::new().base_url(base).build().unwrap();

        client.track("abc").await.expect("second attempt succeeds");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "one failure plus one retry == two requests"
        );
    }

    #[tokio::test]
    async fn extraction_silent_past_the_budget_surfaces_the_error() {
        // Both attempts fail: the caller must see ExtractionSilent, not a
        // success and not some flattened variant.
        let (base, hits) = serve(vec![SILENT, SILENT]).await;
        let client = ClientBuilder::new().base_url(base).build().unwrap();

        let err = client.track("abc").await.expect_err("both attempts fail");
        assert!(matches!(err, Error::ExtractionSilent(_)), "got {err}");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "bounded at one retry");
    }

    #[tokio::test]
    async fn extraction_empty_is_never_retried() {
        // 🔑 The distinction the taxonomy exists for. `extraction_empty` means
        // extraction stopped matching Spotify's page -- retrying it burns 4-6
        // seconds of a held Discord interaction to fail identically.
        let (base, hits) = serve(vec![EMPTY, (200, r#"{"id":"x"}"#)]).await;
        let client = ClientBuilder::new().base_url(base).build().unwrap();

        let err = client.track("abc").await.expect_err("must not retry");
        assert!(matches!(err, Error::ExtractionEmpty(_)), "got {err}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "retrying this would have succeeded on the canned 200, which is exactly \
             the bug: it must NOT reach the second response"
        );
    }

    #[tokio::test]
    async fn retries_can_be_disabled() {
        let (base, hits) = serve(vec![SILENT]).await;
        let client = ClientBuilder::new()
            .base_url(base)
            .extraction_silent_retries(0)
            .build()
            .unwrap();

        let err = client.track("abc").await.expect_err("no retry configured");
        assert!(matches!(err, Error::ExtractionSilent(_)), "got {err}");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_invalid_id_is_never_retried_and_never_requested() {
        let (base, hits) = serve(vec![]).await;
        let client = ClientBuilder::new().base_url(base).build().unwrap();

        let err = client.track("not!valid").await.expect_err("rejected");
        assert!(matches!(err, Error::InvalidId(_)), "got {err}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "validated before any request"
        );
    }
}
