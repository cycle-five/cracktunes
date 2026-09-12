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
        Ok(Self::with_base_url(api_key, DEFAULT_BASE_URL))
    }

    #[must_use]
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .unwrap_or_default(),
        }
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
                retry_after: None,
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves canned bodies in order; returns base url and a request counter.
    /// Counting REQUESTS is the point: a provider that silently skips or
    /// double-calls still returns a plausible value.
    async fn serve(bodies: Vec<(u16, &'static str)>) -> (String, Arc<AtomicUsize>) {
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
                let (code, body) = bodies
                    .get(n)
                    .copied()
                    .unwrap_or((200, r#"{"success":true,"matches":[]}"#));
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let r = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        (format!("http://{addr}"), hits)
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
        let (base, _) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let out = p.recommend(&seed(), 10).await.unwrap();
        assert_eq!(out.len(), 1, "the match with no youtube id is dropped");
        assert_eq!(out[0].youtube_id(), Some("vid1"));
        assert_eq!(out[0].source, "musicatlas");
    }

    /// 🪤 THE TRAP. A status-only client reads this as success.
    #[tokio::test]
    async fn success_false_on_http_200_is_a_not_a_track_error() {
        let (base, _) = serve(vec![(
            200,
            r#"{"success":false,"error":"That doesn't appear to be a released track."}"#,
        )])
        .await;
        let p = MusicAtlas::with_base_url("k", base);
        let err = p
            .recommend(&seed(), 10)
            .await
            .expect_err("must not read as success");
        assert!(matches!(err, Error::NotATrack { .. }), "got {err}");
        assert!(!err.is_transient(), "a bad seed is permanent, not a flake");
    }

    #[tokio::test]
    async fn a_403_is_an_invalid_key_and_is_not_transient() {
        let (base, _) = serve(vec![(403, r#"{"error":"Invalid or unconfirmed API key"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let err = p.recommend(&seed(), 10).await.expect_err("403");
        assert!(matches!(err, Error::InvalidKey { .. }), "got {err}");
        assert!(!err.is_transient());
    }

    #[tokio::test]
    async fn want_caps_the_returned_count() {
        let (base, _) = serve(vec![OK]).await;
        let p = MusicAtlas::with_base_url("k", base);
        assert_eq!(p.recommend(&seed(), 0).await.unwrap().len(), 0);
    }

    /// Not in the measured contract as an observed musicatlas response, but the
    /// code has a dedicated branch for it -- untested, a 429 would fall through
    /// to the JSON parser and come back as `UnexpectedBody` instead of a
    /// transient `RateLimited` the orchestrator knows to back off from.
    #[tokio::test]
    async fn a_429_is_rate_limited_and_is_transient() {
        let (base, _) = serve(vec![(429, r#"{"error":"slow down"}"#)]).await;
        let p = MusicAtlas::with_base_url("k", base);
        let err = p.recommend(&seed(), 10).await.expect_err("429");
        assert!(matches!(err, Error::RateLimited { .. }), "got {err}");
        assert!(
            err.is_transient(),
            "a rate limit is exactly the retryable case"
        );
    }
}
