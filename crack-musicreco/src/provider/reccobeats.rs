//! The free fallback recommender. No API key and no daily quota -- limits are
//! undisclosed, so this owns its own reactive back-off (Ruling 24, spec §4:
//! "each provider owns its own limit discipline"): a 429 carrying
//! `Retry-After` starts a cooldown that refuses every call, at zero request
//! cost, until it expires. It never hands back a directly playable id either.
//! See [`crate::model::Playable`] for why that difference has its own type
//! instead of being flattened into a single "url" field.

use super::http;
use crate::text::normalize;
use crate::{Error, Playable, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const NAME: &str = "reccobeats";
pub const DEFAULT_BASE_URL: &str = "https://api.reccobeats.com/v1";

/// The most one search page holds: measured 2026-09-13, `size=100` is refused
/// with 400 "must be less than or equal to 50". A common title has many
/// covers, so the seed artist's own track can sit far down the page -- or not
/// be on it at all (Queen's "Bohemian Rhapsody" is not in the first 50).
const SEARCH_PAGE_SIZE: usize = 50;

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
    // No default on `id` or `trackTitle`: a track missing either fails the
    // WHOLE page, not just this track, same as `Page.content` above -- R10's
    // page-level strictness is deliberate here too. Silently dropping one
    // malformed track (or the whole page) would be worse than surfacing the
    // parse error and letting the orchestrator try the next provider.
    // Whether ReccoBeats ever actually sends a malformed track is unmeasured.
    id: String,
    #[serde(rename = "trackTitle")]
    track_title: String,
    // `null` and a missing key both read as "no artist" -- `#[serde(default)]`
    // alone only covers the missing case; an explicit `null` needs
    // `null_as_empty` to not fail the whole page over a field that's
    // genuinely optional per track. Whether ReccoBeats ever sends `null` here
    // is unmeasured.
    #[serde(default, deserialize_with = "null_as_empty")]
    artists: Vec<Artist>,
    #[serde(default)]
    isrc: Option<String>,
}

fn null_as_empty<'de, D>(deserializer: D) -> std::result::Result<Vec<Artist>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<Vec<Artist>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: String,
}

/// Whether a stored cooldown deadline is still in effect, and by how much
/// longer -- pure, so Ruling 24's expiry decision can be tested by handing it
/// arbitrary `(now, until)` pairs instead of waiting out a real or paused
/// clock.
fn remaining_cooldown(now: Instant, until: Option<Instant>) -> Option<Duration> {
    until.filter(|u| now < *u).map(|u| u - now)
}

/// The deadline to store after a new `Retry-After`: whichever ends LATER.
/// Two calls can be in flight when the limit hits; the one answered with the
/// shorter wait must not cut short the cooldown the other already set.
fn later_deadline(current: Option<Instant>, new: Instant) -> Instant {
    current.map_or(new, |c| c.max(new))
}

#[derive(Debug)]
pub struct ReccoBeats {
    base_url: String,
    http: reqwest::Client,
    /// Ruling 24 / spec §4: set when a reply carries
    /// `RateLimited { retry_after: Some(d), .. }`. While `Instant::now()` is
    /// still before this deadline, `recommend` refuses every call outright --
    /// zero requests spent waiting out a provider that already told us to.
    /// No `Retry-After` leaves this untouched: an unqualified back-off would
    /// be invented, not measured.
    cooldown_until: Mutex<Option<Instant>>,
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
        let base_url = base_url.into();
        // 🪤 Ruling 30 (Task 5 review): reqwest defers URL parsing to
        // `send()`, so a malformed base URL used to surface as a transient
        // `Error::Transport` on the FIRST call instead of failing at
        // construction, where it belongs.
        http::validate_base_url(NAME, &base_url)?;
        Ok(Self {
            base_url,
            http: http::client(NAME, http::USER_AGENT)?,
            cooldown_until: Mutex::new(None),
        })
    }

    /// [`Self::get`], plus Ruling 24's cooldown bookkeeping: a reply carrying
    /// `RateLimited { retry_after: Some(d), .. }` starts (or extends) this
    /// instance's cooldown before the error reaches the caller, so even the
    /// very next call on this instance sees it.
    async fn get_tracking_cooldown(&self, url: &str) -> Result<Page> {
        let result = self.get(url).await;
        if let Err(Error::RateLimited {
            retry_after: Some(d),
            ..
        }) = &result
        {
            // `checked_add`: `d` is already clamped to an hour
            // (`http::MAX_RETRY_AFTER`), so this cannot realistically
            // overflow, but a cooldown we cannot represent is one we skip
            // rather than panic over.
            if let Some(until) = Instant::now().checked_add(*d) {
                let mut current = self.cooldown_until.lock().expect("cooldown mutex poisoned");
                *current = Some(later_deadline(*current, until));
            }
        }
        result
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
                message: format!("{status}: {}", http::excerpt(&body)),
            });
        }

        serde_json::from_str(&body).map_err(|e| Error::UnexpectedBody {
            provider: NAME,
            message: format!("{e}: {}", http::excerpt(&body)),
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
        // Ruling 24 / spec §4: checked BEFORE any request. A live cooldown
        // costs zero calls, not one wasted probe against a provider that
        // already told us to wait.
        {
            let until = *self.cooldown_until.lock().expect("cooldown mutex poisoned");
            if let Some(retry_after) = remaining_cooldown(Instant::now(), until) {
                return Err(Error::RateLimited {
                    provider: NAME,
                    retry_after: Some(retry_after),
                });
            }
        }

        let base = self.base_url.trim_end_matches('/');
        // 🪤 TWO calls. Recommendation seeds are ReccoBeats UUIDs; Spotify ids
        // are rejected outright (measured: error code 4002, "Cannot find any
        // track"), so the seed's own artist/title has to be resolved to a
        // ReccoBeats id first.
        //
        // 🪤 The title ALONE. ReccoBeats matches `searchText` against track
        // titles only: measured 2026-09-13, "Queen Bohemian Rhapsody" returns
        // nothing, "Bohemian Rhapsody" a full page. v0.11.0 sent both and so
        // never found a seed for anything.
        let q = http::encode_query(&seed.title);
        let found = self
            .get_tracking_cooldown(&format!(
                "{base}/track/search?searchText={q}&size={SEARCH_PAGE_SIZE}"
            ))
            .await?;
        // A title search finds every song with that name, so the seed is the
        // first track BY THE SEED'S ARTIST -- never a stranger's song that
        // happened to rank first. An empty `id` is as useless as no result at
        // all: `seeds=` with nothing after the `=` cannot succeed, so such a
        // track is passed over rather than spending the second call on it.
        let artist = normalize(&seed.artist);
        let Some(matched) = found
            .content
            .into_iter()
            .find(|t| !t.id.is_empty() && t.artists.iter().any(|a| normalize(&a.name) == artist))
        else {
            return Ok(Vec::new());
        };
        // The id is provider-supplied text interpolated straight into a URL.
        let seed_id = http::encode_query(&matched.id);
        // 🔑 `want` passes through unclamped: ReccoBeats' limits on `size` are
        // undisclosed, so no number here would be measured rather than
        // invented. A caller-supplied `usize::MAX` is passed straight to the
        // wire; clamping belongs at the orchestrator, once a real limit is
        // known.
        let page = self
            .get_tracking_cooldown(&format!(
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

    /// L4 / Ruling 30 (Task 5 review): rejected at construction, not spent as
    /// a request that never leaves the process.
    #[test]
    fn with_base_url_rejects_a_malformed_url() {
        let err = ReccoBeats::with_base_url("not a url")
            .expect_err("must be rejected before any request");
        assert!(matches!(err, Error::Config(_)), "got {err}");
    }

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
        //
        // 🪤 Whole-line equality, not `starts_with`: a prefix match lets
        // `size=50` regress to `size=5`, or `seeds=uuid-1` grow a trailing
        // `&seeds=junk`, without ever failing this assertion.
        //
        // 🔑 The title ALONE. ReccoBeats matches `searchText` against track
        // titles: measured 2026-09-13, "Queen Bohemian Rhapsody" returns
        // nothing at all. v0.11.0 sent artist and title and found no seed.
        let reqs = seen.lock().expect("test mutex");
        assert_eq!(reqs.len(), 2);
        assert_eq!(
            reqs[0].lines().next(),
            Some("GET /track/search?searchText=Bohemian%20Rhapsody&size=50 HTTP/1.1"),
            "request 1: {}",
            reqs[0]
        );
        assert_eq!(
            reqs[1].lines().next(),
            Some("GET /track/recommendation?size=5&seeds=uuid-1 HTTP/1.1"),
            "request 2 must seed from the search response's id, not the query text: {}",
            reqs[1]
        );
        assert!(
            reqs[0].to_lowercase().contains("user-agent:") && reqs[0].contains(USER_AGENT),
            "no explicit User-Agent in: {}",
            reqs[0]
        );
    }

    /// M1: `encode_query` is unit-tested directly in `provider::http`, but
    /// that alone doesn't prove `recommend()` actually routes the seed
    /// through it. A raw `&` would silently merge with the next query param,
    /// and a raw `#` would truncate the URL at a fragment, dropping
    /// `&size=50` entirely -- both survive every other test here, since none
    /// of them puts a reserved character in the seed.
    #[tokio::test]
    async fn a_seed_with_reserved_characters_is_percent_encoded_on_the_wire() {
        let (base, _, seen) = serve(vec![SEARCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let seed = Seed {
            artist: "Simon & Garfunkel".into(),
            title: "Song & Dance #1".into(),
            mbid: None,
            confidence: 100,
        };
        let _ = p.recommend(&seed, 5).await;

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[0].lines().next(),
            Some("GET /track/search?searchText=Song%20%26%20Dance%20%231&size=50 HTTP/1.1"),
            "request 1: {}",
            reqs[0]
        );
    }

    /// M1 / R12: the search-response id is provider-supplied text
    /// interpolated straight into `seeds=`. Nothing previously put a
    /// reserved character in that id, so an unencoded `first.id.clone()`
    /// passed every other test.
    #[tokio::test]
    async fn a_search_result_id_with_reserved_characters_is_percent_encoded_into_seeds() {
        const SEARCH_RESERVED_ID: (u16, &str) = (
            200,
            r#"{"content":[{"id":"a&b c","trackTitle":"x","artists":[{"name":"Queen"}],"isrc":null,"href":"h"}]}"#,
        );
        let (base, _, seen) = serve(vec![SEARCH_RESERVED_ID, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let _ = p.recommend(&seed(), 5).await;

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[1].lines().next(),
            Some("GET /track/recommendation?size=5&seeds=a%26b%20c HTTP/1.1"),
            "request 2: {}",
            reqs[1]
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

    /// Searching by title alone finds every song with that name. Measured
    /// 2026-09-13: "Hit That" returns The Offspring's twice, then BIG SIS.
    /// Seeding from whichever came first would recommend around a stranger's
    /// song, so a page with no track by the seed's artist is no seed at all.
    #[tokio::test]
    async fn a_search_with_no_track_by_the_seed_artist_makes_no_recommendation_call() {
        const OTHER_ARTISTS: (u16, &str) = (
            200,
            r#"{"content":[
            {"id":"uuid-x","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Pentatonix"}],"isrc":null,"href":"h"},
            {"id":"uuid-y","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Rockabye Baby!"}],"isrc":null,"href":"h"}]}"#,
        );
        let (base, hits, _seen) = serve(vec![OTHER_ARTISTS, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        assert!(p.recommend(&seed(), 5).await.unwrap().is_empty());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "must NOT reach the canned recommendation response"
        );
    }

    /// The seed's own track need not be the first result, and a credit can
    /// list the seed's artist second. The id sent is the one that matched.
    #[tokio::test]
    async fn the_first_track_by_the_seed_artist_seeds_the_recommendation() {
        const LATER_MATCH: (u16, &str) = (
            200,
            r#"{"content":[
            {"id":"uuid-cover","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Pentatonix"}],"isrc":null,"href":"h"},
            {"id":"uuid-queen","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Someone"},{"name":"Queen"}],"isrc":null,"href":"h"},
            {"id":"uuid-queen-2","trackTitle":"Bohemian Rhapsody","artists":[{"name":"Queen"}],"isrc":null,"href":"h"}]}"#,
        );
        let (base, _, seen) = serve(vec![LATER_MATCH, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        assert_eq!(p.recommend(&seed(), 5).await.unwrap().len(), 1);

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[1].lines().next(),
            Some("GET /track/recommendation?size=5&seeds=uuid-queen HTTP/1.1"),
            "request 2: {}",
            reqs[1]
        );
    }

    /// The artist is compared the way MusicBrainz confirmations are: case,
    /// spacing and a typographic apostrophe do not make it someone else.
    #[tokio::test]
    async fn the_artist_match_ignores_case_spacing_and_curly_apostrophes() {
        const CURLY: (u16, &str) = (
            200,
            r#"{"content":[{"id":"uuid-gnr","trackTitle":"Sweet Child O' Mine","artists":[{"name":"guns  n’ ROSES"}],"isrc":null,"href":"h"}]}"#,
        );
        let (base, hits, _seen) = serve(vec![CURLY, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let seed = Seed {
            artist: "Guns N' Roses".into(),
            title: "Sweet Child O' Mine".into(),
            mbid: None,
            confidence: 100,
        };
        assert_eq!(p.recommend(&seed, 5).await.unwrap().len(), 1);
        assert_eq!(hits.load(Ordering::SeqCst), 2, "search + recommend");
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

    /// M3: the only non-2xx fixture elsewhere in this file (400 with the 4002
    /// body) has no `content` field, so it fails the parse regardless of
    /// whether the status is even checked -- deleting the "any other non-2xx"
    /// arm would leave it passing anyway. This fixture's body WOULD parse
    /// successfully (an empty `content` array is valid), so it is this test,
    /// not the 400 one, that actually needs the status check to exist.
    #[tokio::test]
    async fn a_404_is_unexpected_body_naming_the_status() {
        let (base, _, _seen) = serve(vec![(404, r#"{"content":[]}"#)]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let err = p.recommend(&seed(), 5).await.expect_err("404 is an error");
        match &err {
            Error::UnexpectedBody { message, .. } => {
                assert!(
                    message.contains("404"),
                    "message should name the status: {message}"
                );
            },
            other => panic!("expected UnexpectedBody, got {other}"),
        }
        assert!(!err.is_transient(), "a 404 is not a flake");
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
        // 🪤 I2: `matches!(UnexpectedBody)` alone doesn't discriminate --
        // when the redirect IS followed, the chain lands on the mock's
        // past-the-end default (missing `content`) and ALSO ends up
        // `UnexpectedBody`. `hits == 1` above is what actually catches a
        // followed redirect; this only additionally checks that the message
        // names the status we refused.
        match &got {
            Err(Error::UnexpectedBody { message, .. }) => {
                assert!(
                    message.contains("302"),
                    "message should name the 3xx status: {message}"
                );
            },
            other => panic!("expected UnexpectedBody naming the 3xx status, got {other:?}"),
        }
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

    /// L6: `#[serde(default)]` alone covers a MISSING `artists` key, not an
    /// explicit `null`. Whether ReccoBeats ever actually sends `null` here is
    /// unmeasured, but the deserializer should not crash the whole page over
    /// a field the doc comment already calls "genuinely optional" if it does.
    #[tokio::test]
    async fn a_null_artists_field_is_treated_as_no_artist() {
        const RECO_NULL_ARTISTS: (u16, &str) = (
            200,
            r#"{"content":[{"id":"uuid-4","trackTitle":"Mulla","artists":null,"isrc":"X","href":"h"}]}"#,
        );
        let (base, _, _seen) = serve(vec![SEARCH, RECO_NULL_ARTISTS]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        let out = p.recommend(&seed(), 5).await.unwrap();
        assert_eq!(out[0].artist, "");
        assert_eq!(out[0].search_text(), "Mulla");
    }

    /// L8: `{"content":[{"id":""}]}"` used to send `seeds=` (nothing after
    /// the `=`), a request that cannot possibly succeed. Treated the same as
    /// no search result at all.
    #[tokio::test]
    async fn an_empty_search_result_id_short_circuits_like_an_empty_search() {
        const SEARCH_EMPTY_ID: (u16, &str) = (
            200,
            r#"{"content":[{"id":"","trackTitle":"x","artists":[{"name":"Queen"}],"isrc":null,"href":"h"}]}"#,
        );
        let (base, hits, _seen) = serve(vec![SEARCH_EMPTY_ID, RECO]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");
        assert!(p.recommend(&seed(), 5).await.unwrap().is_empty());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "must NOT reach the canned recommendation response"
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

    // Ruling 24 / spec §4: "each provider owns its own limit discipline; the
    // orchestrator does not generalize them." ReccoBeats' undisclosed,
    // reactive back-off lives here, not in `MusicReco`.

    #[test]
    fn remaining_cooldown_is_none_with_no_cooldown_set() {
        assert_eq!(remaining_cooldown(Instant::now(), None), None);
    }

    #[test]
    fn remaining_cooldown_is_none_once_the_deadline_has_passed() {
        let now = Instant::now();
        let past = now - Duration::from_secs(1);
        assert_eq!(remaining_cooldown(now, Some(past)), None);
    }

    #[test]
    fn remaining_cooldown_is_some_while_still_active() {
        let now = Instant::now();
        let until = now + Duration::from_secs(10);
        assert_eq!(
            remaining_cooldown(now, Some(until)),
            Some(Duration::from_secs(10))
        );
    }

    #[test]
    fn later_deadline_starts_a_cooldown_when_none_is_set() {
        let new = Instant::now() + Duration::from_secs(5);
        assert_eq!(later_deadline(None, new), new);
    }

    /// Two calls in flight when the limit hits: the one answered `Retry-After: 5`
    /// must not cut short the 42s the other already stored.
    #[test]
    fn later_deadline_never_shortens_a_live_cooldown() {
        let now = Instant::now();
        let long = now + Duration::from_secs(42);
        let short = now + Duration::from_secs(5);
        assert_eq!(later_deadline(Some(long), short), long);
        assert_eq!(later_deadline(Some(short), long), long);
    }

    /// The required shape: a 429 + `Retry-After: 42`, then an immediate
    /// second `recommend` -- hit count stays 1 and the second call is itself
    /// `RateLimited`, spent on nothing.
    #[tokio::test]
    async fn a_rate_limit_with_retry_after_starts_a_cooldown_that_blocks_the_next_call() {
        let (base, hits, _seen) = serve(vec![Canned {
            status: 429,
            body: r#"{"error":"slow down"}"#,
            headers: &[("Retry-After", "42")],
        }])
        .await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");

        let first = p.recommend(&seed(), 5).await.expect_err("429");
        assert!(matches!(first, Error::RateLimited { .. }), "got {first}");

        let second = p
            .recommend(&seed(), 5)
            .await
            .expect_err("still cooling down");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the cooldown must stop the second call before it reaches the network"
        );
        match second {
            Error::RateLimited { retry_after, .. } => {
                let retry_after = retry_after.expect("a live cooldown reports a remaining wait");
                assert!(
                    retry_after <= Duration::from_secs(42),
                    "remaining wait ({retry_after:?}) must be at most the original 42s"
                );
            },
            other => panic!("expected RateLimited, got {other}"),
        }
    }

    /// Sabotage target: cooling down on a `RateLimited` that carried no
    /// `Retry-After` at all would invent a backoff duration nothing measured.
    /// Two 429s with no `Retry-After` header, across two separate `recommend`
    /// calls, must both reach the network. (A 5xx takes the same arm in
    /// `get`, so this covers an outage too.)
    #[tokio::test]
    async fn a_rate_limit_without_retry_after_does_not_start_a_cooldown() {
        let (base, hits, _seen) = serve(vec![(429, "slow down"), (429, "slow down")]).await;
        let p = ReccoBeats::with_base_url(base).expect("client builds");

        let _ = p.recommend(&seed(), 5).await;
        let _ = p.recommend(&seed(), 5).await;

        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "no Retry-After means no cooldown -- both calls must reach the network"
        );
    }
}
