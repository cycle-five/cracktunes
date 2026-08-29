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
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            user_agent: default_user_agent(),
            http: None,
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
    async fn fetch<T: DeserializeOwned>(
        &self,
        endpoint: Endpoint,
        id: &str,
    ) -> Result<(T, Option<CacheStatus>)> {
        validate_id(id)?;
        let url = self.join(&["v1", endpoint.segment(), id])?;
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
