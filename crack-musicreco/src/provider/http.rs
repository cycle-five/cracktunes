//! Shared HTTP plumbing for providers: client construction and reading
//! `Retry-After`. Extracted out of musicatlas (Task 3) once ReccoBeats needed
//! the identical policy -- a fix here (a missing header, a dropped timeout)
//! now fixes every provider at once instead of only the one that happened to
//! get patched.

use crate::{Error, Result};
use std::time::Duration;

/// 🪤 MANDATORY on every provider client. musicatlas answers a default/absent
/// User-Agent with 403 and the SAME body as a bad key, so omitting this looks
/// exactly like a bad credential. Applied to every provider, not only the one
/// it was measured on: there is no reason to expect the others are more
/// forgiving, only that nobody has been burned by it yet.
pub(crate) const USER_AGENT: &str = concat!("cracktunes/", env!("CARGO_PKG_VERSION"));

/// Ceiling for one call. This runs on the track-end path, so a hung peer must
/// not hold up the next song.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Build a provider's HTTP client: explicit User-Agent, no automatic
/// redirects, and a request timeout.
///
/// # Errors
/// [`Error::Config`] if the client cannot be built.
pub(crate) fn client(provider: &'static str) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
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
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Config(format!("could not build the {provider} HTTP client: {e}")))
}

/// Read `Retry-After` (whole seconds) from a response's headers.
///
/// 🪤 Must be called BEFORE `resp.text()`, which consumes the response -- a
/// header not taken here is gone.
pub(crate) fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Minimal percent-encoding for a query value; avoids a dependency for one
/// use. `pub(crate)` at the provider level (not private to one provider)
/// because ReccoBeats and Task 5's MusicBrainz resolver both need to
/// interpolate provider-supplied text into a URL, and a resolver reaching
/// into a specific provider's internals for a generic utility is worse
/// layering than both reaching into shared plumbing.
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
