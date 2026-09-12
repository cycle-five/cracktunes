//! The primary recommender. The only provider that hands back a directly
//! playable YouTube id, which is why it is tried first despite being metered.

use crate::{Error, Playable, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const NAME: &str = "musicatlas";
pub const DEFAULT_BASE_URL: &str = "https://musicatlas.ai";
/// 🪤 MANDATORY. The API answers a default/absent User-Agent with 403 and the
/// SAME body as a bad key, so omitting this looks exactly like a bad credential.
pub const USER_AGENT: &str = concat!("cracktunes/", env!("CARGO_PKG_VERSION"));

/// Ceiling for one call. This runs on the track-end path, so a hung peer must
/// not hold up the next song.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
        // one is a binding constraint of this crate: musicatlas answers an
        // unrecognised agent with 403 and a bad-key body. So a builder failure
        // would have produced a client that gets refused, and the refusal would
        // have read as `InvalidKey` -- sending whoever debugged it to rotate a
        // credential that was fine.
        //
        // It also made `new`'s documented `# Errors` unreachable: the function
        // said it could return `Error::Config` and no input could make it.
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            // 🪤 reqwest follows redirects by default, and this provider is
            // metered at 100 calls/day with no header reporting the remainder.
            // A 302 anywhere upstream -- canonicalisation, a load-balancer
            // quirk -- turns one logical call into two physical ones and
            // silently halves the daily budget, with nothing to see it by.
            // Verified: an identical client against a mock returning 302
            // issued two requests.
            //
            // A redirect we did not expect is a fact worth surfacing, not
            // something to quietly obey.
            .redirect(reqwest::redirect::Policy::none())
            // A hung peer would otherwise stall `recommend()` forever rather
            // than surfacing `Transport`, and this sits on the track-end path.
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| Error::Config(format!("could not build the {NAME} HTTP client: {e}")))?;
        Ok(Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
            http,
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

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>> {
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
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(std::time::Duration::from_secs);

        let body = resp.text().await.map_err(|source| Error::Transport {
            provider: NAME,
            source,
        })?;

        if status == 403 {
            return Err(Error::InvalidKey {
                provider: NAME,
                message: body,
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
            message: format!("{e}: {body}"),
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves canned bodies in order. Returns the base url, a request counter,
    /// and the raw text of every request received.
    ///
    /// Counting REQUESTS is the point: a provider that silently skips or
    /// double-calls still returns a plausible value. Keeping the request TEXT
    /// is the same idea one level down -- the only way to assert on what we
    /// actually sent, rather than on what we got back.
    async fn serve(
        bodies: Vec<(u16, &'static str)>,
    ) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let n = served.fetch_add(1, Ordering::SeqCst);
                let (code, body) = bodies
                    .get(n)
                    .copied()
                    .unwrap_or((200, r#"{"success":true,"matches":[]}"#));
                // 🪤 Read until the end of the request head rather than once.
                // A single `read` is not guaranteed to deliver it all -- that
                // holds today only because these payloads are tiny and travel
                // over loopback, which is a property of the test environment,
                // not of TCP.
                let mut raw = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            raw.extend_from_slice(&buf[..n]);
                            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        },
                        Err(_) => break,
                    }
                }
                recorded
                    .lock()
                    .expect("test mutex")
                    .push(String::from_utf8_lossy(&raw).into_owned());
                let r = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        (format!("http://{addr}"), hits, seen)
    }

    fn seed() -> Seed {
        Seed {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            mbid: None,
            confidence: 100,
        }
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
        let out = p.recommend(&seed(), 10).await.unwrap();
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
        let out = p.recommend(&seed(), 2).await.unwrap();
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
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let served = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let n = served.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                // First request: redirect to ourselves, with a real `Location`
                // so a permissive client follows it. Second: a normal success,
                // which is what makes a followed redirect look like it worked.
                let r = if n == 0 {
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{addr}/v1/recommend\r\n\
                         Content-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                } else {
                    let body = r#"{"success":true,"matches":[]}"#;
                    format!(
                        "HTTP/1.1 200 X\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });

        let p = MusicAtlas::with_base_url("k", format!("http://{addr}")).expect("client builds");
        let got = p.recommend(&seed(), 5).await;

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
        let err = p.recommend(&seed(), 5).await.expect_err("500 is an error");
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
            .recommend(&seed(), 10)
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
        let err = p.recommend(&seed(), 10).await.expect_err("403");
        assert!(matches!(err, Error::InvalidKey { .. }), "got {err}");
        assert!(!err.is_transient());
    }

    #[tokio::test]
    async fn want_caps_the_returned_count() {
        let (base, _, _seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        assert_eq!(p.recommend(&seed(), 0).await.unwrap().len(), 0);
    }

    /// Not in the measured contract as an observed musicatlas response, but the
    /// code has a dedicated branch for it -- untested, a 429 would fall through
    /// to the JSON parser and come back as `UnexpectedBody` instead of a
    /// transient `RateLimited` the orchestrator knows to back off from.
    #[tokio::test]
    async fn a_429_is_rate_limited_and_is_transient() {
        let (base, _, _seen) = serve(vec![(429, r#"{"error":"slow down"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let err = p.recommend(&seed(), 10).await.expect_err("429");
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
        let (base, _, seen) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base).expect("client builds");
        let _ = p.recommend(&seed(), 5).await;

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
    /// response -- so this also pins the ordering, not just the parse.
    #[tokio::test]
    async fn a_429_carries_the_providers_own_retry_after() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf).await;
            let body = r#"{"error":"slow down"}"#;
            let r = format!(
                "HTTP/1.1 429 X\r\nRetry-After: 42\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(r.as_bytes()).await;
            let _ = s.shutdown().await;
        });

        let p = MusicAtlas::with_base_url("k", format!("http://{addr}")).expect("client builds");
        let err = p.recommend(&seed(), 5).await.expect_err("429 is an error");
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
