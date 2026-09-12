//! The free fallback recommender. No API key and no daily quota -- limits are
//! undisclosed, but a 429 still carries `Retry-After` -- and it never hands
//! back a directly playable id. See [`crate::model::Playable`] for why that
//! difference has its own type instead of being flattened into a single "url"
//! field.

use super::http;
use crate::{Error, Playable, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;

const NAME: &str = "reccobeats";
pub const DEFAULT_BASE_URL: &str = "https://api.reccobeats.com/v1";

/// 🪤 Deliberately NOT `#[serde(default)]`. The measured rejection for a seed
/// ReccoBeats cannot resolve (e.g. a Spotify id, which it does not accept) is
/// error code 4002, in a body like
/// `{"timestamp":"..","error":4002,"message":"Cannot find any track"}` --
/// which has no `content` field at all. A defaulted `content` would parse
/// that shape into an empty page, so a provider error would read as "no
/// recommendations" and the orchestrator would silently fall through instead
/// of seeing the failure. `artists` and `isrc` stay defaulted: those are
/// genuinely optional per-track fields, not "did the call even succeed".
#[derive(Debug, Deserialize)]
struct Page {
    content: Vec<Track>,
}

#[derive(Debug, Deserialize)]
struct Track {
    id: String,
    #[serde(rename = "trackTitle")]
    track_title: String,
    #[serde(default)]
    artists: Vec<Artist>,
    #[serde(default)]
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: String,
}

#[derive(Debug, Clone)]
pub struct ReccoBeats {
    base_url: String,
    http: reqwest::Client,
}

impl ReccoBeats {
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn new() -> Result<Self> {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// The same, against a different host. Tests point this at a local mock.
    ///
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            base_url: base_url.into(),
            http: http::client(NAME)?,
        })
    }

    /// One GET, classifying the status before parsing the body.
    async fn get(&self, url: &str) -> Result<Page> {
        let resp = self
            .http
            .get(url)
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

        // A rate limit and a server fault get the same transient treatment:
        // both are "try again later", and neither is this seed's fault.
        if status == 429 || status >= 500 {
            return Err(Error::RateLimited {
                provider: NAME,
                retry_after,
            });
        }
        // Redirects are not followed (see the shared client builder), so a
        // 3xx arrives here intact rather than as a second physical request.
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

        serde_json::from_str(&body).map_err(|e| Error::UnexpectedBody {
            provider: NAME,
            message: format!("{e}: {body}"),
        })
    }
}

#[async_trait]
impl crate::provider::Recommender for ReccoBeats {
    fn name(&self) -> &'static str {
        NAME
    }

    // `is_metered()` is left at its default of `false`: this is the free
    // fallback. If it were metered the orchestrator would skip it on shaky
    // seeds, and a free fallback that gets skipped is not a fallback.

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>> {
        if want == 0 {
            // Not even the search call is worth spending on a request for
            // nothing.
            return Ok(Vec::new());
        }

        let base = self.base_url.trim_end_matches('/');
        // 🪤 TWO calls. Recommendation seeds are ReccoBeats UUIDs; Spotify ids
        // are rejected outright (measured: error code 4002, "Cannot find any
        // track"), so the seed's own artist/title has to be resolved to a
        // ReccoBeats id first.
        let q = http::encode_query(&format!("{} {}", seed.artist, seed.title));
        let found = self
            .get(&format!("{base}/track/search?searchText={q}&size=1"))
            .await?;
        let Some(first) = found.content.into_iter().next() else {
            // No seed track: do NOT spend the second call.
            return Ok(Vec::new());
        };
        // The id is provider-supplied text interpolated straight into a URL.
        let seed_id = http::encode_query(&first.id);
        let page = self
            .get(&format!(
                "{base}/track/recommendation?size={want}&seeds={seed_id}"
            ))
            .await?;

        Ok(page
            .content
            .into_iter()
            .map(|t| {
                let artist = t
                    .artists
                    .first()
                    .map_or_else(String::new, |a| a.name.clone());
                // 🪤 An artistless track's search query is the title ALONE,
                // not `" - title"` -- `format!("{artist} - {title}")` with an
                // empty artist emits a leading " - " that no test would catch
                // unless it specifically checks for it.
                let query = if artist.is_empty() {
                    t.track_title.clone()
                } else {
                    format!("{artist} - {}", t.track_title)
                };
                Recommendation {
                    // ReccoBeats gives no video id, ever.
                    playable: Playable::SearchQuery(query),
                    artist,
                    title: t.track_title,
                    isrc: t.isrc,
                    source: NAME.into(),
                }
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
    use std::time::Duration;

    fn seed() -> Seed {
        Seed {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            mbid: None,
            confidence: 100,
        }
    }

    const SEARCH: (u16, &str) = (
        200,
        r#"{"content":[{"id":"uuid-1","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Queen"}],"isrc":"X","href":"https://open.spotify.com/track/z"}]}"#,
    );
    const RECO: (u16, &str) = (
        200,
        r#"{"content":[{"id":"uuid-2","trackTitle":"Mulla","artists":[{"name":"Kashcoming"}],"isrc":"USA2P2511772","href":"https://open.spotify.com/track/7x"}]}"#,
    );

    #[tokio::test]
    async fn search_then_recommend_takes_two_calls_and_yields_search_queries() {
        use crate::provider::http::USER_AGENT;

        let (base, hits, seen) = serve(vec![SEARCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 5).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2, "search + recommend");
        assert_eq!(out.len(), 1);
        // 🔑 ReccoBeats never returns a video id.
        assert_eq!(out[0].youtube_id(), None);
        assert_eq!(out[0].search_text(), "Kashcoming - Mulla");
        assert_eq!(out[0].isrc.as_deref(), Some("USA2P2511772"));

        // Assert on what we SENT, not only what came back -- every
        // response-shaped assertion above would pass with the wrong URL, the
        // wrong seed id, or no User-Agent at all.
        let reqs = seen.lock().expect("test mutex");
        assert_eq!(reqs.len(), 2);
        assert!(
            reqs[0].starts_with("GET /track/search?searchText=Queen%20Bohemian%20Rhapsody&size=1"),
            "request 1: {}",
            reqs[0]
        );
        assert!(
            reqs[1].starts_with("GET /track/recommendation?size=5&seeds=uuid-1"),
            "request 2 must seed from the search response's id, not the query text: {}",
            reqs[1]
        );
        assert!(
            reqs[0].to_lowercase().contains("user-agent:") && reqs[0].contains(USER_AGENT),
            "no explicit User-Agent in: {}",
            reqs[0]
        );
    }

    #[tokio::test]
    async fn an_empty_search_makes_no_recommendation_call() {
        let (base, hits, _seen) = serve(vec![(200, r#"{"content":[]}"#), RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        assert!(p.recommend(&seed(), 5).await.unwrap().is_empty());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "must NOT reach the canned recommendation response"
        );
    }

    #[tokio::test]
    async fn a_429_carries_retry_after() {
        let (base, _, _seen) = serve(vec![Canned {
            status: 429,
            body: r#"{"error":"slow down"}"#,
            headers: &[("Retry-After", "42")],
        }])
        .await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let err = p.recommend(&seed(), 5).await.expect_err("429");
        assert!(err.is_transient(), "rate limiting passes");
        match err {
            Error::RateLimited { retry_after, .. } => assert_eq!(
                retry_after,
                Some(Duration::from_secs(42)),
                "the provider said 42 seconds and we must carry that"
            ),
            other => panic!("expected RateLimited, got {other}"),
        }
    }

    /// The measured rejection for a seed ReccoBeats cannot resolve: HTTP 400
    /// with an error-code body. Not a flake -- retrying the same seed will
    /// not help.
    #[tokio::test]
    async fn a_bad_seed_rejection_is_not_a_transient_error() {
        let (base, _, _seen) = serve(vec![(
            400,
            r#"{"timestamp":"t","error":4002,"message":"Cannot find any track"}"#,
        )])
        .await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let err = p.recommend(&seed(), 5).await.expect_err("400 is an error");
        assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
        assert!(!err.is_transient(), "a bad seed is permanent, not a flake");
    }

    /// 🪤 THE TRAP for `#[serde(default)]` on `Page.content`. A 200 whose body
    /// has no `content` field at all must be a parse error, not a silent
    /// empty page -- see the trap comment on `Page`.
    #[tokio::test]
    async fn a_response_with_no_content_field_is_an_error_not_an_empty_page() {
        let (base, _, _seen) = serve(vec![(
            200,
            r#"{"timestamp":"t","error":4002,"message":"Cannot find any track"}"#,
        )])
        .await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let err = p
            .recommend(&seed(), 5)
            .await
            .expect_err("a missing `content` field must not read as an empty page");
        assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
    }

    #[tokio::test]
    async fn a_server_fault_is_transient() {
        let (base, _, _seen) = serve(vec![(500, "Internal Server Error")]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let err = p.recommend(&seed(), 5).await.expect_err("500 is an error");
        assert!(
            err.is_transient(),
            "a 5xx must be retryable, got {err} (is_transient=false)"
        );
    }

    /// The redirect-refusal policy lives in the shared `http::client()`, so
    /// this pins the same behaviour musicatlas has -- a mutation to the
    /// shared client should break both providers' tests, not just one.
    #[tokio::test]
    async fn a_redirect_is_refused_rather_than_followed() {
        let (base, hits, _seen) = serve(vec![
            Canned {
                status: 302,
                body: "",
                headers: &[("Location", "http://{addr}/track/search")],
            },
            SEARCH.into(),
        ])
        .await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let got = p.recommend(&seed(), 5).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a followed redirect would cost a second request"
        );
        assert!(
            matches!(got, Err(Error::UnexpectedBody { .. })),
            "an unfollowed 3xx should surface, got {got:?}"
        );
    }

    #[tokio::test]
    async fn want_zero_makes_no_requests() {
        let (base, hits, _seen) = serve(vec![SEARCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 0).await.unwrap();
        assert!(out.is_empty());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "must not even make the search call"
        );
    }

    /// 🪤 Every other fixture yields at most one recommendation, so
    /// `.take(want)`, `.take(want - 1)` and no `.take` at all would produce
    /// identical output. This is the shape that tells them apart: more
    /// recommendations than asked for.
    #[tokio::test]
    async fn want_is_a_ceiling_and_it_is_exact() {
        const THREE: (u16, &str) = (
            200,
            r#"{"content":[
            {"id":"uuid-a","trackTitle":"T1","artists":[{"name":"A1"}],"isrc":"I1","href":"h"},
            {"id":"uuid-b","trackTitle":"T2","artists":[{"name":"A2"}],"isrc":"I2","href":"h"},
            {"id":"uuid-c","trackTitle":"T3","artists":[{"name":"A3"}],"isrc":"I3","href":"h"}]}"#,
        );
        let (base, _, _seen) = serve(vec![SEARCH, THREE]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 2).await.unwrap();
        assert_eq!(out.len(), 2, "asked for 2 of 3");
        // In order, so a `.take` that also reorders is caught.
        assert_eq!(out[0].title, "T1");
        assert_eq!(out[1].title, "T2");
    }

    #[tokio::test]
    async fn artist_and_title_are_not_swapped() {
        let (base, _, _seen) = serve(vec![SEARCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 5).await.unwrap();
        assert_eq!(
            out[0].artist, "Kashcoming",
            "artist must come from the artist field"
        );
        assert_eq!(
            out[0].title, "Mulla",
            "title must come from the title field"
        );
    }

    #[tokio::test]
    async fn an_artistless_track_search_query_is_the_title_alone() {
        const RECO_NO_ARTIST: (u16, &str) = (
            200,
            r#"{"content":[{"id":"uuid-3","trackTitle":"Mulla","artists":[],"isrc":"X","href":"h"}]}"#,
        );
        let (base, _, _seen) = serve(vec![SEARCH, RECO_NO_ARTIST]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 5).await.unwrap();
        assert_eq!(out[0].artist, "");
        assert_eq!(
            out[0].search_text(),
            "Mulla",
            "no leading ' - ' when there is no artist"
        );
    }

    /// If this were `true`, the orchestrator would skip the one free fallback
    /// on exactly the shaky seeds it exists to cover.
    #[test]
    fn recco_beats_is_not_metered() {
        assert!(!ReccoBeats::with_base_url(DEFAULT_BASE_URL)
            .expect("client builds")
            .is_metered());
    }
}
