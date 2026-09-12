//! Shared HTTP plumbing for providers: client construction and reading
//! `Retry-After`. Extracted out of musicatlas (Task 3) once ReccoBeats needed
//! the identical policy -- a fix here (a missing header, a dropped timeout)
//! now fixes every provider at once instead of only the one that happened to
//! get patched.

use crate::{Error, Result};
use std::time::Duration;

/// The contact-less User-Agent musicatlas and ReccoBeats both use. NOT
/// suitable for every provider: MusicBrainz (Task 5) requires a UA with a
/// contact address and IP-bans clients that omit one, so `client()` takes the
/// User-Agent as a parameter rather than hardcoding this constant -- a fixed
/// signature here would have forced Task 5 to either hand-roll a second
/// client (re-losing the redirect policy and timeout) or send MusicBrainz a
/// contactless UA.
pub(crate) const USER_AGENT: &str = concat!("cracktunes/", env!("CARGO_PKG_VERSION"));

/// Ceiling for one call. This runs on the track-end path, so a hung peer must
/// not hold up the next song.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Build a provider's HTTP client with the crate's default request timeout.
/// See [`client_with_timeout`] for the policy and its rationale.
///
/// # Errors
/// [`Error::Config`] if the client cannot be built.
pub(crate) fn client(provider: &'static str, user_agent: &str) -> Result<reqwest::Client> {
    client_with_timeout(provider, user_agent, REQUEST_TIMEOUT)
}

/// Build a provider's HTTP client: an explicit User-Agent, no automatic
/// redirects, and a request timeout.
///
/// 🔑 Split out from [`client`] so a test can exercise the timeout itself
/// (a short one, against a peer that never answers) without waiting out the
/// real 10s default -- the public constructors (`MusicAtlas::with_base_url`,
/// `ReccoBeats::with_base_url`) only ever call `client`, so production
/// behaviour is unaffected.
///
/// # Errors
/// [`Error::Config`] if the client cannot be built.
pub(crate) fn client_with_timeout(
    provider: &'static str,
    user_agent: &str,
    timeout: Duration,
) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(user_agent)
        // 🪤 reqwest follows redirects by default. musicatlas is metered at
        // 100 calls/day with no header reporting the remainder, so a 302
        // anywhere upstream -- canonicalisation, a load-balancer quirk --
        // would turn one logical call into two physical ones and silently
        // halve the daily budget, with nothing to see it by. Verified: an
        // identical client against a mock returning 302 issued two requests.
        //
        // Applied to every provider, not only the metered one: a redirect we
        // did not expect is a fact worth surfacing, not something to quietly
        // obey.
        .redirect(reqwest::redirect::Policy::none())
        // A hung peer would otherwise stall `recommend()` forever rather than
        // surfacing `Transport`.
        .timeout(timeout)
        .build()
        .map_err(|e| Error::Config(format!("could not build the {provider} HTTP client: {e}")))
}

/// Local ceiling on how long we will ever wait once a provider tells us to
/// back off -- our own policy, not the provider's.
///
/// 🪤 `Retry-After: <u64::MAX>` parses into a `Duration` that does not itself
/// panic to construct, but `Instant::now() + that_duration` (the natural next
/// step for any caller that acts on it) does. Clamping here, at the one place
/// every provider's 429/5xx path funnels through, means no caller has to
/// remember to guard the arithmetic later.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60 * 60);

/// Read `Retry-After` (whole seconds) from a response's headers, clamped to
/// [`MAX_RETRY_AFTER`].
///
/// 🪤 Must be called BEFORE `resp.text()`, which consumes the response -- a
/// header not taken here is gone.
pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .map(|d| d.min(MAX_RETRY_AFTER))
}

/// Minimal percent-encoding for a query value. `pub(crate)` at the provider
/// level (not private to one provider) because ReccoBeats and Task 5's
/// MusicBrainz resolver both need to interpolate provider-supplied text into
/// a URL, and a resolver reaching into a specific provider's internals for a
/// generic utility is worse layering than both reaching into shared plumbing.
///
/// Hand-rolled rather than delegating to `reqwest`'s own query encoding or
/// `serde_urlencoded` because both encode a space as `+`, which has never
/// been measured against ReccoBeats or MusicBrainz -- `%20` is what was
/// measured, so `%20` is what stays. (`url`, a declared workspace dependency,
/// is unused elsewhere in this crate and has no percent-encoding helper
/// worth pulling in for this one case either.)
pub(crate) fn encode_query(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            },
            b' ' => "%20".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, RETRY_AFTER};

    /// 🪤 Verified against ReccoBeats-shaped input, not invented: reserved
    /// characters (`& # % + = ? /`), a space, multi-byte UTF-8, and the empty
    /// string. Nothing previously called `encode_query` directly -- the one
    /// request-shape assertion that touched it was satisfied by reqwest's own
    /// URL parser encoding spaces, so an identity function or one that passed
    /// `& + # %` straight through both survived the whole suite.
    #[test]
    fn encode_query_matches_measured_vectors() {
        assert_eq!(
            encode_query("Simon & Garfunkel #1 50% a+b=c?/"),
            "Simon%20%26%20Garfunkel%20%231%2050%25%20a%2Bb%3Dc%3F%2F"
        );
        assert_eq!(encode_query("Beyoncé"), "Beyonc%C3%A9");
        assert_eq!(encode_query("日本"), "%E6%97%A5%E6%9C%AC");
        assert_eq!(encode_query(""), "");
    }

    #[test]
    fn retry_after_rejects_an_http_date() {
        // RFC 9110 allows an HTTP-date here; this crate only understands the
        // delay-seconds form, so a date must read as "no answer given", not
        // panic or misparse.
        let mut h = HeaderMap::new();
        h.insert(
            RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn retry_after_trims_surrounding_whitespace() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, " 42 ".parse().unwrap());
        assert_eq!(retry_after(&h), Some(Duration::from_secs(42)));
    }

    #[test]
    fn retry_after_rejects_a_negative_value() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, "-1".parse().unwrap());
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn retry_after_clamps_a_huge_value_to_the_local_ceiling() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, u64::MAX.to_string().parse().unwrap());
        assert_eq!(retry_after(&h), Some(MAX_RETRY_AFTER));
    }

    /// A hung peer must produce our `Transport` error within the client's
    /// configured timeout, not hang the caller (and, in production, the
    /// track-end path) forever. Uses `client_with_timeout` directly with a
    /// short timeout rather than waiting out the real 10s default.
    #[tokio::test]
    async fn a_hung_peer_times_out_rather_than_hanging_forever() {
        let base = crate::test_support::accept_and_hang().await;
        let http = client_with_timeout("test", USER_AGENT, Duration::from_millis(200))
            .expect("client builds");

        let started = std::time::Instant::now();
        // An outer, generous guard: if the per-request timeout regressed away
        // entirely, this fails definitively instead of hanging the suite.
        let outcome = tokio::time::timeout(Duration::from_secs(2), http.get(&base).send()).await;
        let inner = outcome.expect("the client-side timeout must fire well within 2s");

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "took {:?}, longer than the ~1s a 200ms timeout should allow",
            started.elapsed()
        );
        let err = inner.expect_err("a hung peer must produce an error, not a response");
        assert!(err.is_timeout(), "expected a timeout error, got {err}");
    }
}
