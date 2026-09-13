//! The primary recommender. The only provider that hands back a directly
//! playable YouTube id, which is why it is tried first despite being metered.

use super::http;
use crate::{Error, Playable, RawTrack, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const NAME: &str = "musicatlas";
pub const DEFAULT_BASE_URL: &str = "https://musicatlas.ai";

#[derive(Debug, Serialize)]
struct Request<'a> {
    artist: &'a str,
    track: &'a str,
}

/// 🔑 `success` is the discriminator, NOT the HTTP status: logical failures
/// arrive as 200.
#[derive(Debug, Deserialize)]
struct Response {
    success: bool,
    #[serde(default)]
    matches: Vec<Match>,
    #[serde(default)]
    error: Option<String>,
}

/// musicatlas' own refusal of a credential, as measured:
/// `{"error":"Invalid or unconfirmed API key"}`. Typed so that a 403 which is
/// NOT this shape -- a Cloudflare challenge page -- cannot pass for it.
#[derive(Debug, Deserialize)]
struct KeyRefusal {
    error: String,
}

#[derive(Debug, Deserialize)]
struct Match {
    artist: String,
    title: String,
    #[serde(default)]
    platform_ids: PlatformIds,
}

#[derive(Debug, Default, Deserialize)]
struct PlatformIds {
    #[serde(default)]
    youtube: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MusicAtlas {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl MusicAtlas {
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    /// The same, against a different host. Tests point this at a local mock so
    /// the suite never spends any of the live 100/day quota.
    ///
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        // 🪤 This was `.unwrap_or_default()`, which is not a harmless fallback
        // here. `Client::default()` carries **no User-Agent**, and an explicit
        // one is a binding constraint of this crate (see `http::client`): a
        // builder failure would have produced a client that gets refused, and
        // the refusal would have read as `InvalidKey` -- sending whoever
        // debugged it to rotate a credential that was fine.
        //
        // It also made `new`'s documented `# Errors` unreachable: the function
        // said it could return `Error::Config` and no input could make it.
        let base_url = base_url.into();
        // 🪤 Ruling 30 (Task 5 review): reqwest defers URL parsing to
        // `send()`, so a malformed base URL used to surface as a transient
        // `Error::Transport` on the FIRST call -- spending one of the
        // 100/day metered budget on a request that never leaves.
        http::validate_base_url(NAME, &base_url)?;
        Ok(Self {
            api_key: api_key.into(),
            base_url,
            http: http::client(NAME, http::USER_AGENT)?,
        })
    }
}

#[async_trait]
impl crate::provider::Recommender for MusicAtlas {
    fn name(&self) -> &'static str {
        NAME
    }

    /// 100 calls/day on the free tier, and no header reports the remainder.
    fn is_metered(&self) -> bool {
        true
    }

    async fn recommend(
        &self,
        _track: &RawTrack,
        seed: Option<&Seed>,
        want: usize,
    ) -> Result<Vec<Recommendation>> {
        if want == 0 {
            // This is the METERED provider: a call spent for zero wanted
            // results is one of the 100/day gone for nothing.
            return Ok(Vec::new());
        }
        // The orchestrator never calls a provider that needs a seed without
        // one. Should it happen anyway, there is nothing to ask about.
        let Some(seed) = seed else {
            return Ok(Vec::new());
        };

        let url = format!("{}/api/similar_tracks", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&Request {
                artist: &seed.artist,
                track: &seed.title,
            })
            .send()
            .await
            .map_err(|source| Error::Transport {
                provider: NAME,
                source,
            })?;

        let status = resp.status().as_u16();
        // 🪤 Read BEFORE the body: `resp.text()` consumes the response, so a
        // header not taken here is gone. `Retry-After` was being discarded
        // this way, and the 429 arm then hardcoded `None` -- throwing away the
        // provider's own answer to "when may I try again" on the one provider
        // where that answer costs something to guess wrong.
        let retry_after = http::retry_after(resp.headers());

        let body = resp.text().await.map_err(|source| Error::Transport {
            provider: NAME,
            source,
        })?;

        // 🪤 Not every 403 is a bad key. musicatlas sits behind Cloudflare
        // (measured 2026-09-12: `server: cloudflare`), and a challenge or WAF
        // block is also a 403 -- with an HTML page, not this JSON. Since Task 6
        // an `InvalidKey` disables the provider for the life of the process,
        // so a CDN hiccup would silently end metered autoplay until restart.
        // Only the provider's own refusal earns that.
        if status == 403 {
            return Err(match serde_json::from_str::<KeyRefusal>(&body) {
                Ok(refusal) => Error::InvalidKey {
                    provider: NAME,
                    message: refusal.error,
                },
                Err(_) => Error::UnexpectedBody {
                    provider: NAME,
                    message: format!(
                        "403 without musicatlas' key-refusal body (a CDN or WAF block?): {}",
                        http::excerpt(&body)
                    ),
                },
            });
        }
        if status == 429 {
            return Err(Error::RateLimited {
                provider: NAME,
                retry_after,
            });
        }
        // Redirects are not followed (see the client builder), so a 3xx
        // arrives here intact rather than as a second billed request.
        if (300..400).contains(&status) {
            return Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!(
                    "{status} redirect, not followed -- following it would spend a second \
                     metered call. Check the base url."
                ),
            });
        }
        // 🪤 Before this, a 500 with a non-JSON body ("Internal Server Error")
        // fell through to the parse below and became `UnexpectedBody`, which
        // `is_transient()` reports as FALSE -- so a plain outage was
        // classified permanent and never retried. Server faults are the
        // textbook transient case.
        if status >= 500 {
            return Err(Error::RateLimited {
                provider: NAME,
                retry_after,
            });
        }

        let parsed: Response = serde_json::from_str(&body).map_err(|e| Error::UnexpectedBody {
            provider: NAME,
            message: format!("{e}: {}", http::excerpt(&body)),
        })?;

        if !parsed.success {
            return Err(Error::NotATrack {
                provider: NAME,
                artist: seed.artist.clone(),
                title: seed.title.clone(),
                message: parsed.error.unwrap_or_else(|| "no reason given".into()),
            });
        }

        Ok(parsed
            .matches
            .into_iter()
            .filter_map(|m| {
                m.platform_ids.youtube.map(|id| Recommendation {
                    artist: m.artist,
                    title: m.title,
                    playable: Playable::YouTubeId(id),
                    isrc: None,
                    source: NAME.into(),
                })
            })
            .take(want)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Recommender as _;
    use crate::test_support::{serve, Canned};
    use std::sync::atomic::Ordering;

    fn seed() -> Seed {
        Seed {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            mbid: None,
            confidence: 100,
        }
    }

    fn raw() -> RawTrack {
        RawTrack {
            title: "Queen - Bohemian Rhapsody".into(),
            artist: None,
            uploader: None,
            video_id: None,
        }
    }

    /// The METERED provider: a call without a seed would be one of the
    /// 100/day spent asking about nothing.
    #[tokio::test]
    async fn no_seed_makes_no_request() {
        let (base, hits, _seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        assert!(p.recommend(&raw(), None, 10).await.unwrap().is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    const OK: (u16, &str) = (
        200,
        r#"{"success":true,"matches":[
        {"artist":"A","title":"T","platform_ids":{"youtube":"vid1","spotify":"sp1"}},
        {"artist":"B","title":"U","platform_ids":{"spotify":"sp2"}}]}"#,
    );

    #[tokio::test]
    async fn maps_matches_and_keeps_only_playable_ones() {
        let (base, _, _seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let out = p.recommend(&raw(), Some(&seed()), 10).await.unwrap();
        assert_eq!(out.len(), 1, "the match with no youtube id is dropped");
        assert_eq!(out[0].youtube_id(), Some("vid1"));
        assert_eq!(out[0].source, "musicatlas");
        // 🪤 Was unasserted. The fixture distinguishes the two fields ("A"
        // and "T"), so swapping `artist: m.title, title: m.artist` in the map
        // passed every test in this file -- and a swapped seed is a wrong
        // search on every subsequent provider.
        assert_eq!(out[0].artist, "A", "artist must come from the artist field");
        assert_eq!(out[0].title, "T", "title must come from the title field");
    }

    /// 🪤 `want` was never actually exercised. Every other fixture yields at
    /// most one playable match, so `.take(want)`, `.take(want - 1)` and no
    /// `.take` at all produced identical output. This is the shape that can
    /// tell them apart: more playable matches than asked for.
    #[tokio::test]
    async fn want_is_a_ceiling_and_it_is_exact() {
        const THREE: (u16, &str) = (
            200,
            r#"{"success":true,"matches":[
            {"artist":"A","title":"T","platform_ids":{"youtube":"v1"}},
            {"artist":"B","title":"U","platform_ids":{"youtube":"v2"}},
            {"artist":"C","title":"V","platform_ids":{"youtube":"v3"}}]}"#,
        );
        let (base, _, _seen) = serve(vec![THREE]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let out = p.recommend(&raw(), Some(&seed()), 2).await.unwrap();
        assert_eq!(out.len(), 2, "asked for 2 of 3 playable matches");
        // In order, so a `.take` that also reorders is caught.
        assert_eq!(out[0].youtube_id(), Some("v1"));
        assert_eq!(out[1].youtube_id(), Some("v2"));
    }

    /// One logical call must cost exactly one metered request. A followed
    /// redirect would make it two, against a 100/day budget with no header
    /// reporting the remainder.
    ///
    /// 🪤 The first version of this test served a 302 with **no `Location`
    /// header**, and so proved nothing: reqwest has nothing to follow, and the
    /// test passed identically with the redirect policy removed. A sabotage run
    /// caught that. The redirect must be one a client would actually take.
    #[tokio::test]
    async fn a_redirect_is_refused_rather_than_followed() {
        // First request: redirect to ourselves, with a real `Location` so a
        // permissive client follows it. Second: a normal success, which is
        // what makes a followed redirect look like it worked.
        let (base, hits, _seen) = serve(vec![
            Canned {
                status: 302,
                body: "",
                headers: &[("Location", "http://{addr}/v1/recommend")],
            },
            Canned {
                status: 200,
                body: r#"{"success":true,"matches":[]}"#,
                headers: &[],
            },
        ])
        .await;

        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let got = p.recommend(&raw(), Some(&seed()), 5).await;

        // 🔑 The assertion that matters is the REQUEST COUNT, not the returned
        // value: following the redirect yields `Ok([])`, a perfectly plausible
        // answer, while having spent two of 100 daily calls.
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "one logical call must cost exactly one metered request; a followed \
             redirect spends two and looks like success"
        );
        assert!(
            matches!(got, Err(Error::UnexpectedBody { .. })),
            "an unfollowed 3xx should surface, got {got:?}"
        );
    }

    /// A server fault is the textbook transient case. A 500 with a non-JSON
    /// body used to fall through to the JSON parse and become
    /// `UnexpectedBody`, which `is_transient()` reports as false -- so a plain
    /// outage was classified permanent and never retried.
    #[tokio::test]
    async fn a_server_fault_is_retryable() {
        let (base, _, _seen) = serve(vec![(500, "Internal Server Error")]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 5)
            .await
            .expect_err("500 is an error");
        assert!(
            err.is_transient(),
            "a 5xx must be retryable, got {err} (is_transient=false)"
        );
    }

    /// 🪤 THE TRAP. A status-only client reads this as success.
    #[tokio::test]
    async fn success_false_on_http_200_is_a_not_a_track_error() {
        let (base, _, _seen) = serve(vec![(
            200,
            r#"{"success":false,"error":"That doesn't appear to be a released track."}"#,
        )])
        .await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 10)
            .await
            .expect_err("must not read as success");
        assert!(matches!(err, Error::NotATrack { .. }), "got {err}");
        assert!(!err.is_transient(), "a bad seed is permanent, not a flake");
    }

    #[tokio::test]
    async fn a_403_is_an_invalid_key_and_is_not_transient() {
        let (base, _, _seen) =
            serve(vec![(403, r#"{"error":"Invalid or unconfirmed API key"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 10)
            .await
            .expect_err("403");
        match &err {
            Error::InvalidKey { message, .. } => {
                assert_eq!(message, "Invalid or unconfirmed API key");
            },
            other => panic!("expected InvalidKey, got {other}"),
        }
        assert!(!err.is_transient());
    }

    /// 🪤 musicatlas is behind Cloudflare, whose challenge and WAF blocks are
    /// also 403s -- with a page of HTML. An `InvalidKey` disables the provider
    /// for the process (Ruling 23), so this must NOT read as one.
    #[tokio::test]
    async fn a_403_that_is_not_musicatlas_refusing_the_key_is_not_an_invalid_key() {
        let page: &'static str = Box::leak(
            format!(
                "<!DOCTYPE html><title>Just a moment...</title>{}",
                "<div class=\"cf\"></div>".repeat(200)
            )
            .into_boxed_str(),
        );
        let (base, hits, _seen) = serve(vec![(403, page)]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 10)
            .await
            .expect_err("403");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        match err {
            Error::UnexpectedBody { message, .. } => {
                assert!(message.starts_with("403 without"), "got {message}");
                assert!(message.len() < 400, "the page must not reach the log whole");
            },
            other => panic!("expected UnexpectedBody, got {other}"),
        }
    }

    /// P09 (review): before the `want == 0` guard, this returned `Ok([])`
    /// after spending 1 of the 100/day metered calls -- a plausible-looking
    /// output that hid a wasted call. Asserting only `.len() == 0` cannot see
    /// that; `hits == 0` is the assertion that matters here.
    #[tokio::test]
    async fn want_caps_the_returned_count() {
        let (base, hits, _seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        assert_eq!(
            p.recommend(&raw(), Some(&seed()), 0).await.unwrap().len(),
            0
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "must not spend a metered call for zero wanted"
        );
    }

    /// Not in the measured contract as an observed musicatlas response, but the
    /// code has a dedicated branch for it -- untested, a 429 would fall through
    /// to the JSON parser and come back as `UnexpectedBody` instead of a
    /// transient `RateLimited` the orchestrator knows to back off from.
    #[tokio::test]
    async fn a_429_is_rate_limited_and_is_transient() {
        let (base, _, _seen) = serve(vec![(429, r#"{"error":"slow down"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 10)
            .await
            .expect_err("429");
        assert!(matches!(err, Error::RateLimited { .. }), "got {err}");
        assert!(
            err.is_transient(),
            "a rate limit is exactly the retryable case"
        );
    }
    /// 🪤 An explicit User-Agent is a binding constraint of this crate, not a
    /// nicety: musicatlas answers an unrecognised agent with **403 and a
    /// bad-key body**. A client that lost its UA would therefore be diagnosed
    /// as `InvalidKey`, sending whoever debugged it to rotate a credential that
    /// was never the problem.
    ///
    /// Nothing asserted it until now, and the constructor used to reach
    /// `Client::default()` -- which carries no UA -- through
    /// `.unwrap_or_default()`. Asserting on the REQUEST WE SENT is the only
    /// way to see this; every response-shaped assertion in this file passes
    /// with the header absent.
    #[tokio::test]
    async fn the_request_carries_an_explicit_user_agent() {
        use crate::provider::http::USER_AGENT;

        let (base, _, seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let _ = p.recommend(&raw(), Some(&seed()), 5).await;

        let reqs = seen.lock().expect("test mutex");
        let req = reqs.first().expect("the provider made a request");
        let lower = req.to_lowercase();
        assert!(
            lower.contains("user-agent:"),
            "no User-Agent header in the request we sent:\n{req}"
        );
        assert!(
            req.contains(USER_AGENT),
            "User-Agent is not ours (expected {USER_AGENT}):\n{req}"
        );
    }
    /// 🪤 `Retry-After` was read nowhere and the 429 arm hardcoded `None`,
    /// throwing away the provider's own answer to "when may I try again" on
    /// the one provider where guessing wrong costs metered calls.
    ///
    /// The header has to be taken BEFORE `resp.text()`, which consumes the
    /// response. The COMPILER pins that ordering -- reading it after would be
    /// E0382, a borrow of moved/consumed data, not something a test could
    /// even exercise. What THIS test pins is that the header's VALUE is
    /// actually carried through into the returned error, which nothing
    /// enforces at compile time.
    /// L4 / Ruling 30 (Task 5 review): rejected at construction, not spent as
    /// a metered call that never leaves the process.
    #[test]
    fn with_base_url_rejects_a_malformed_url() {
        let err = MusicAtlas::with_base_url("k", "not a url")
            .expect_err("must be rejected before any request");
        assert!(matches!(err, Error::Config(_)), "got {err}");
    }

    #[tokio::test]
    async fn a_429_carries_the_providers_own_retry_after() {
        let (base, _, _seen) = serve(vec![Canned {
            status: 429,
            body: r#"{"error":"slow down"}"#,
            headers: &[("Retry-After", "42")],
        }])
        .await;

        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p
            .recommend(&raw(), Some(&seed()), 5)
            .await
            .expect_err("429 is an error");
        match err {
            Error::RateLimited { retry_after, .. } => assert_eq!(
                retry_after,
                Some(std::time::Duration::from_secs(42)),
                "the provider said 42 seconds and we must carry that"
            ),
            other => panic!("expected RateLimited, got {other}"),
        }
    }
}
