//! MusicBrainz as a seed canonicalizer, not a recommender: it has no
//! similar-track capability at all, but it scores a title-parsed guess so a
//! metered call (musicatlas, ReccoBeats) is never spent on a bad one.

use crate::provider::http;
use crate::resolver::{title_parse::TitleParseResolver, SeedResolver};
use crate::{Error, RawTrack, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

const NAME: &str = "musicbrainz";
pub const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2";

/// 🪤 HARD limit, and it is enforced against the IP, not per guild or per
/// process. Exceeding it blocks every service on this host, not just one
/// guild's autoplay -- see [`MusicBrainz`]'s doc for what that means for how
/// many instances may exist.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1_000);

/// 🪤 Deliberately NOT `#[serde(default)]` -- same trap as ReccoBeats'
/// `Page.content` (R10). An error body (a non-2xx `{"error": "..."}`) has no
/// `recordings` field at all; a default would parse that shape as "no match"
/// instead of the parse failure it actually is. In practice the status is
/// classified before this ever runs, so this only guards a 2xx with an
/// unexpected shape -- but the container/field asymmetry is the same lesson.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    recordings: Vec<Recording>,
}

#[derive(Debug, Deserialize)]
struct Recording {
    id: String,
    #[serde(default)]
    score: u8,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<Credit>,
}

#[derive(Debug, Deserialize)]
struct Credit {
    name: String,
}

/// A MusicBrainz seed canonicalizer: scores a [`TitleParseResolver`] guess
/// against MusicBrainz's recording search, raising its confidence and
/// attaching an mbid on a hit.
///
/// 🔑 The 1 req/sec gate ([`MIN_INTERVAL`]) lives in `self.gate`, an
/// instance-local mutex -- it serializes calls made through THIS instance
/// only. The limit is enforced by MusicBrainz against the calling IP, so the
/// process must hold exactly ONE shared `MusicBrainz` (behind an `Arc`, owned
/// by Task 6's orchestrator): two independent instances would each obey
/// 1/sec internally while jointly sending 2/sec from the same address.
#[derive(Debug)]
pub struct MusicBrainz {
    base_url: String,
    http: reqwest::Client,
    gate: Mutex<Option<Instant>>,
}

impl MusicBrainz {
    /// # Errors
    /// [`Error::Config`] if `contact` is empty or whitespace-only, or if the
    /// HTTP client cannot be built.
    pub fn new(contact: &str) -> Result<Self> {
        Self::with_base_url(contact, DEFAULT_BASE_URL)
    }

    /// The same, against a different host. Tests point this at a local mock.
    ///
    /// # Errors
    /// [`Error::Config`] if `contact` is empty or whitespace-only, or if the
    /// HTTP client cannot be built.
    pub fn with_base_url(contact: &str, base_url: impl Into<String>) -> Result<Self> {
        // 🪤 A contact-less UA is not merely impolite here: MusicBrainz's
        // policy is to IP-ban clients that omit one, which would take down
        // every provider on the host, not just this resolver.
        if contact.trim().is_empty() {
            return Err(Error::Config(
                "MusicBrainz requires a contact address in the User-Agent (an empty one risks an IP ban for the whole host)".into(),
            ));
        }
        let ua = format!("cracktunes/{} ( {contact} )", env!("CARGO_PKG_VERSION"));
        Ok(Self {
            base_url: base_url.into(),
            http: http::client(NAME, &ua)?,
            gate: Mutex::new(None),
        })
    }

    /// Block until at least [`MIN_INTERVAL`] has elapsed since the previous
    /// call to this method returned, on this instance.
    async fn throttle(&self) {
        let mut last = self.gate.lock().await;
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < MIN_INTERVAL {
                tokio::time::sleep(MIN_INTERVAL - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }

    /// Lucene-escape a value for use inside a double-quoted query term.
    ///
    /// 🪤 R16: quoting the term is not enough on its own -- an unescaped `"`
    /// inside the value would close the quoted term early. `\` must be
    /// escaped FIRST: escaping `"` before `\` would also match the `\`s just
    /// inserted by the first pass and double-escape them.
    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
}

#[async_trait]
impl SeedResolver for MusicBrainz {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>> {
        // Reuse the offline parse to get something to ask ABOUT. With no
        // separator (and no supplied artist) there is nothing to query, so
        // the 1/sec budget is not spent.
        let Some(guess) = TitleParseResolver::new().resolve(raw).await? else {
            return Ok(None);
        };
        // 🪤 R16: quoted AND escaped. Unquoted, `artist:Guns N' Roses` splits
        // into three terms (`Guns` OR `N'` OR `Roses`) instead of naming one
        // artist.
        let q = format!(
            r#"artist:"{}" AND recording:"{}""#,
            Self::escape(&guess.artist),
            Self::escape(&guess.title)
        );
        let url = format!(
            "{}/recording?query={}&fmt=json&limit=1",
            self.base_url.trim_end_matches('/'),
            http::encode_query(&q)
        );

        self.throttle().await;
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|source| Error::Transport {
                provider: NAME,
                source,
            })?;

        let status = resp.status().as_u16();
        // 🪤 Read BEFORE the body: `resp.text()` consumes the response, so a
        // header not taken here is gone.
        let retry_after = http::retry_after(resp.headers());
        let body = resp.text().await.map_err(|source| Error::Transport {
            provider: NAME,
            source,
        })?;

        // 🪤 R15: 503 is how MusicBrainz specifically signals rate limiting,
        // folded into the same "any 5xx is transient" arm musicatlas and
        // ReccoBeats use, so callers do not need a MusicBrainz-specific case.
        if status >= 500 {
            return Err(Error::RateLimited {
                provider: NAME,
                retry_after,
            });
        }
        // Redirects are not followed (shared client builder), so a 3xx
        // arrives here intact rather than as a second physical request.
        if (300..400).contains(&status) {
            return Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!("{status} redirect, not followed. Check the base url."),
            });
        }
        if !(200..300).contains(&status) {
            return Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!("{status}: {body}"),
            });
        }

        let parsed: SearchResponse =
            serde_json::from_str(&body).map_err(|e| Error::UnexpectedBody {
                provider: NAME,
                message: format!("{e}: {body}"),
            })?;

        Ok(parsed.recordings.into_iter().next().map(|r| Seed {
            artist: r
                .artist_credit
                .first()
                .map_or(guess.artist, |c| c.name.clone()),
            title: r.title,
            mbid: Some(r.id),
            confidence: r.score,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{serve, Canned};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    const HIT: (u16, &str) = (
        200,
        r#"{"recordings":[{"id":"mbid-1","score":100,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"}]}]}"#,
    );

    fn track(title: &str) -> RawTrack {
        RawTrack {
            title: title.into(),
            artist: None,
            uploader: None,
        }
    }

    #[tokio::test]
    async fn a_confident_match_raises_confidence_and_carries_the_mbid() {
        let (base, _hits, _seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap()
            .expect("a seed");
        assert_eq!(seed.artist, "Queen");
        assert_eq!(seed.title, "Bohemian Rhapsody");
        assert_eq!(seed.confidence, 100);
        assert_eq!(seed.mbid.as_deref(), Some("mbid-1"));
    }

    #[tokio::test]
    async fn no_recordings_yields_no_seed_rather_than_an_error() {
        let (base, _hits, _seen) = serve(vec![(200, r#"{"recordings":[]}"#)]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        assert!(mb.resolve(&track("Queen - Nope")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_title_with_no_separator_makes_no_request_at_all() {
        let (base, hits, _seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let got = mb.resolve(&track("Never Gonna Give You Up")).await.unwrap();
        assert!(got.is_none());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "nothing to query with, so the 1/sec budget is not spent"
        );
    }

    /// R16: the WHOLE request line, for an artist that needs quoting to stay
    /// one term and a title whose quote and `&` need escaping/encoding.
    /// Anything short of a whole-line comparison (a `starts_with`, a
    /// `contains`) would pass with a dropped `AND`, a missing quote, or an
    /// extra trailing param.
    #[tokio::test]
    async fn a_multi_word_artist_and_a_quoted_title_are_quoted_and_escaped_on_the_wire() {
        let (base, _hits, seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let raw = RawTrack {
            title: r#"Song "Live" & Loud"#.into(),
            artist: Some("Guns N' Roses".into()),
            uploader: None,
        };
        let _ = mb.resolve(&raw).await;

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(reqs.len(), 1);
        let expected_query =
            http::encode_query(r#"artist:"Guns N' Roses" AND recording:"Song \"Live\" & Loud""#);
        assert_eq!(
            reqs[0].lines().next(),
            Some(
                format!("GET /recording?query={expected_query}&fmt=json&limit=1 HTTP/1.1").as_str()
            ),
            "request: {}",
            reqs[0]
        );
    }

    #[tokio::test]
    async fn the_user_agent_carries_the_contact_address() {
        let (base, _hits, seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("ops@cracktun.es", base).expect("client builds");
        let _ = mb.resolve(&track("Queen - Bohemian Rhapsody")).await;

        let reqs = seen.lock().expect("test mutex");
        assert!(
            reqs[0].to_lowercase().contains("user-agent:") && reqs[0].contains("ops@cracktun.es"),
            "no contact address in User-Agent: {}",
            reqs[0]
        );
    }

    #[tokio::test]
    async fn an_empty_contact_is_a_config_error() {
        let err = MusicBrainz::new("   ").expect_err("blank contact must be rejected");
        assert!(matches!(err, Error::Config(_)), "got {err}");
    }

    /// 🪤 R15: 503 is how MusicBrainz specifically signals rate limiting.
    #[tokio::test]
    async fn a_503_is_rate_limited_and_carries_retry_after() {
        let (base, _hits, _seen) = serve(vec![Canned {
            status: 503,
            body: r#"{"error":"slow down"}"#,
            headers: &[("Retry-After", "7")],
        }])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("503");
        assert!(err.is_transient(), "rate limiting must be retried");
        match err {
            Error::RateLimited { retry_after, .. } => {
                assert_eq!(retry_after, Some(Duration::from_secs(7)));
            },
            other => panic!("expected RateLimited, got {other}"),
        }
    }

    /// A malformed query or similar client-side rejection is not a flake --
    /// retrying the same seed will not help.
    #[tokio::test]
    async fn a_400_is_unexpected_body_naming_the_status() {
        let (base, _hits, _seen) = serve(vec![(400, r#"{"error":"bad query"}"#)]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("400");
        match &err {
            Error::UnexpectedBody { message, .. } => {
                assert!(
                    message.contains("400"),
                    "message should name the status: {message}"
                );
            },
            other => panic!("expected UnexpectedBody, got {other:?}"),
        }
        assert!(!err.is_transient(), "a 400 is not a flake");
    }

    /// 🪤 THE TRAP for `#[serde(default)]` on `SearchResponse.recordings`. A
    /// 2xx whose body has no `recordings` field at all must be a parse
    /// error, not a silent "no match".
    #[tokio::test]
    async fn a_response_with_no_recordings_field_is_an_error_not_no_match() {
        let (base, _hits, _seen) = serve(vec![(200, r#"{"error":"something else"}"#)]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("a missing `recordings` field must not read as no match");
        assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
    }

    /// No test in the crate asserts the `Transport` mapping until now --
    /// every prior transport-shaped test used a hung peer (a timeout), not a
    /// refused connection. Through `resolve`, not reqwest directly.
    #[tokio::test]
    async fn a_refused_connection_is_a_transient_transport_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        drop(listener); // nothing is listening at `addr` any more

        let mb =
            MusicBrainz::with_base_url("a@b.c", format!("http://{addr}")).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("nothing is listening");
        assert!(matches!(err, Error::Transport { .. }), "got {err}");
        assert!(err.is_transient(), "a refused connection should be retried");
    }

    /// R18: the 1/sec floor is a property of ONE instance's gate, pinned
    /// without touching the network -- real TCP under paused time is
    /// meaningless (auto-advance during IO waits hides the floor entirely).
    /// 🪤 Pinned against a LITERAL, not `MIN_INTERVAL` itself -- comparing
    /// elapsed time to `2 * MIN_INTERVAL` would make the sabotage
    /// "`MIN_INTERVAL` -> 0" invisible: the threshold collapses to 0 right
    /// alongside the thing being measured, and `elapsed() >= 0` always holds.
    #[test]
    fn min_interval_is_the_measured_one_second_floor() {
        assert_eq!(MIN_INTERVAL, Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn sequential_throttle_calls_are_spaced_by_min_interval() {
        let mb =
            MusicBrainz::with_base_url("a@b.c", "http://unused.invalid").expect("client builds");
        let start = Instant::now();
        mb.throttle().await;
        mb.throttle().await;
        mb.throttle().await;
        assert!(
            start.elapsed() >= Duration::from_secs(2),
            "three calls must span at least 2 seconds, got {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_throttle_callers_are_still_spaced_by_min_interval() {
        let mb = Arc::new(
            MusicBrainz::with_base_url("a@b.c", "http://unused.invalid").expect("client builds"),
        );
        let start = Instant::now();
        let a = {
            let mb = Arc::clone(&mb);
            tokio::spawn(async move { mb.throttle().await })
        };
        let b = {
            let mb = Arc::clone(&mb);
            tokio::spawn(async move { mb.throttle().await })
        };
        a.await.expect("task a");
        b.await.expect("task b");
        assert!(
            start.elapsed() >= Duration::from_secs(1),
            "two concurrent callers must still be spaced by at least one second, got {:?}",
            start.elapsed()
        );
    }
}
