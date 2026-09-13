//! Deezer's artist radio: the fallback when YouTube's Mix has nothing.
//!
//! No key. Measured 2026-09-13: `search/artist?q=The Offspring` finds id 882,
//! and `artist/882/radio` plays The Offspring, Rage Against the Machine,
//! blink-182, Green Day and Foo Fighters. That comes from listening data, not
//! from audio features, which is what makes it on-genre where ReccoBeats was
//! not.
//!
//! 🪤 **Errors arrive as HTTP 200.** Measured:
//! `artist/999999999999/radio` answers
//! `200 {"error":{"type":"DataException","message":"no data","code":800}}`, and
//! `code` is not always there: `artist/0/radio` answers
//! `200 {"error":{"type":"Exception","message":"An error has occured"}}`. A
//! client that trusts the status reads each of those as a page of no tracks.

use super::http;
use crate::text::normalize;
use crate::{Error, Playable, RawTrack, Recommendation, Result, Seed};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::collections::HashSet;

const NAME: &str = "deezer";
pub const DEFAULT_BASE_URL: &str = "https://api.deezer.com";

/// Artists compared per search. The exact name is usually first, but a
/// tribute act can outrank it.
const ARTIST_SEARCH_LIMIT: usize = 10;

/// Deezer's documented quota error, "Quota limit exceeded". Documented, not
/// measured.
const QUOTA_EXCEEDED: u32 = 4;

/// 🔑 Untagged, error first: an error body has `error` and no `data`, a page
/// has `data` and no `error`. `Page.data` is not defaulted, so a body that is
/// neither fails to parse instead of reading as an empty page.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Reply<T> {
    Failed { error: ApiError },
    Page(Page<T>),
}

#[derive(Debug, Deserialize)]
struct Page<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    code: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    id: u64,
    name: String,
    /// Followers. What tells the band apart from the other artists who share
    /// its exact name (see `recommend`).
    #[serde(default)]
    nb_fan: u64,
}

#[derive(Debug, Deserialize)]
struct Track {
    title: String,
    /// The title without a version suffix: "Hit That" rather than "Hit That
    /// (2008 Remaster)". What a YouTube search should look for.
    #[serde(default)]
    title_short: Option<String>,
    artist: TrackArtist,
}

#[derive(Debug, Deserialize)]
struct TrackArtist {
    name: String,
}

#[derive(Debug)]
pub struct Deezer {
    base_url: String,
    http: reqwest::Client,
}

impl Deezer {
    /// # Errors
    /// [`Error::Config`] if the HTTP client cannot be built.
    pub fn new() -> Result<Self> {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// The same, against a different host. Tests point this at a local mock.
    ///
    /// # Errors
    /// [`Error::Config`] if `base_url` is not a valid `http`/`https` URL or the
    /// HTTP client cannot be built.
    pub fn with_base_url(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into();
        http::validate_base_url(NAME, &base_url)?;
        Ok(Self {
            base_url,
            http: http::client(NAME, http::USER_AGENT)?,
        })
    }

    /// One GET, classifying the status and then the body's own error field.
    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<Vec<T>> {
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
        // Read BEFORE the body, which consumes the response.
        let retry_after = http::retry_after(resp.headers());
        let body = resp.text().await.map_err(|source| Error::Transport {
            provider: NAME,
            source,
        })?;

        if status == 429 || status >= 500 {
            return Err(Error::RateLimited {
                provider: NAME,
                retry_after,
            });
        }
        if !(200..300).contains(&status) {
            return Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!("{status}: {}", http::excerpt(&body)),
            });
        }
        match serde_json::from_str::<Reply<T>>(&body) {
            Ok(Reply::Page(page)) => Ok(page.data),
            Ok(Reply::Failed { error }) if error.code == Some(QUOTA_EXCEEDED) => {
                Err(Error::RateLimited {
                    provider: NAME,
                    retry_after: None,
                })
            },
            Ok(Reply::Failed { error }) => Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!(
                    "{} (code {}): {}",
                    error.kind,
                    error
                        .code
                        .map_or_else(|| "none".to_owned(), |c| c.to_string()),
                    error.message
                ),
            }),
            Err(e) => Err(Error::UnexpectedBody {
                provider: NAME,
                message: format!("{e}: {}", http::excerpt(&body)),
            }),
        }
    }
}

#[async_trait]
impl crate::provider::Recommender for Deezer {
    fn name(&self) -> &'static str {
        NAME
    }

    async fn recommend(
        &self,
        _track: &RawTrack,
        seed: Option<&Seed>,
        want: usize,
    ) -> Result<Vec<Recommendation>> {
        let Some(seed) = seed.filter(|s| !s.artist.trim().is_empty()) else {
            return Ok(Vec::new());
        };
        if want == 0 {
            return Ok(Vec::new());
        }
        let base = self.base_url.trim_end_matches('/');

        let q = http::encode_query(seed.artist.trim());
        let artists: Vec<Artist> = self
            .get(&format!(
                "{base}/search/artist?q={q}&limit={ARTIST_SEARCH_LIMIT}"
            ))
            .await?;
        // The seed's artist exactly, never the top hit: a radio for a tribute
        // act, or for whoever shares a word with the name, is a stranger's.
        //
        // 🪤 And of the artists with exactly that name, the most followed, not
        // the first. Measured: `q=Queen` lists four exact "Queen"s ordered by
        // relevance, and the band (12.8M fans) is the fourth of them; the first
        // has 131 fans and a one-track radio. A tie keeps Deezer's order.
        let artist_key = normalize(&seed.artist);
        let Some(artist) = artists
            .into_iter()
            .filter(|a| normalize(&a.name) == artist_key)
            .reduce(|best, a| if a.nb_fan > best.nb_fan { a } else { best })
        else {
            return Ok(Vec::new());
        };

        // One extra, for the seed track should the radio play it.
        let tracks: Vec<Track> = self
            .get(&format!(
                "{base}/artist/{}/radio?limit={}",
                artist.id,
                want + 1
            ))
            .await?;
        let seed_key = (artist_key, normalize(&seed.title));
        let mut seen = HashSet::from([seed_key]);
        Ok(tracks
            .into_iter()
            .map(|t| {
                let title = t
                    .title_short
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(t.title);
                (t.artist.name, title)
            })
            .filter(|(artist, title)| seen.insert((normalize(artist), normalize(title))))
            .map(|(artist, title)| Recommendation {
                playable: Playable::SearchQuery(format!("{artist} - {title}")),
                artist,
                title,
                isrc: None,
                source: NAME.into(),
            })
            .take(want)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Recommender as _;
    use crate::test_support::serve;
    use std::sync::atomic::Ordering;

    fn seed() -> Seed {
        Seed {
            artist: "The Offspring".into(),
            title: "Hit That".into(),
            mbid: None,
            confidence: 50,
        }
    }

    fn ended() -> RawTrack {
        RawTrack {
            title: "The Offspring ~ Hit That".into(),
            artist: None,
            uploader: Some("MrCalienteLP".into()),
            video_id: Some("NJKhbnSGLsQ".into()),
        }
    }

    const SEARCH: (u16, &str) = (
        200,
        r#"{"data":[{"id":1,"name":"The Offspring Tribute Band","type":"artist"},{"id":882,"name":"The Offspring","type":"artist"}],"total":2}"#,
    );

    /// The seed track itself, a repeat, an empty `title_short` and a missing
    /// one, among real radio picks.
    const RADIO: (u16, &str) = (
        200,
        r#"{"data":[
        {"id":1,"title":"Hit That (2008 Remaster)","title_short":"Hit That","readable":true,"artist":{"id":882,"name":"The Offspring"}},
        {"id":2,"title":"Holiday","title_short":"Holiday","readable":true,"artist":{"id":1,"name":"Green Day"}},
        {"id":3,"title":"Mutt","title_short":"","readable":true,"artist":{"id":2,"name":"blink-182"}},
        {"id":4,"title":"Holiday","title_short":"Holiday","readable":true,"artist":{"id":1,"name":"Green Day"}},
        {"id":5,"title":"Freedom","readable":true,"artist":{"id":3,"name":"Rage Against the Machine"}}]}"#,
    );

    #[test]
    fn with_base_url_rejects_a_malformed_url() {
        let err = Deezer::with_base_url("not a url").expect_err("rejected before any request");
        assert!(matches!(err, Error::Config(_)), "got {err}");
    }

    #[test]
    fn it_needs_a_seed() {
        assert!(Deezer::new().expect("client builds").needs_seed());
    }

    #[tokio::test]
    async fn the_seed_artist_is_found_and_its_radio_played_by_search() {
        use crate::provider::http::USER_AGENT;

        let (base, hits, seen) = serve(vec![SEARCH, RADIO]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let out = p.recommend(&ended(), Some(&seed()), 5).await.unwrap();

        assert_eq!(hits.load(Ordering::SeqCst), 2, "search + radio");
        let titles: Vec<_> = out.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Holiday", "Mutt", "Freedom"],
            "no seed track, no repeat, and a blank title_short falls back"
        );
        assert_eq!(out[0].artist, "Green Day");
        assert_eq!(out[0].search_text(), "Green Day - Holiday");
        assert_eq!(
            out[0].youtube_id(),
            None,
            "Deezer never supplies a video id"
        );
        assert_eq!(out[0].source, "deezer");

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[0].lines().next(),
            Some("GET /search/artist?q=The%20Offspring&limit=10 HTTP/1.1"),
            "request 1: {}",
            reqs[0]
        );
        assert_eq!(
            reqs[1].lines().next(),
            Some("GET /artist/882/radio?limit=6 HTTP/1.1"),
            "request 2 must use the EXACT artist's id, not the first hit's: {}",
            reqs[1]
        );
        assert!(
            reqs[0].to_lowercase().contains("user-agent:") && reqs[0].contains(USER_AGENT),
            "no explicit User-Agent in: {}",
            reqs[0]
        );
    }

    /// 🪤 Measured 2026-09-13: `q=Queen` returns four artists named exactly
    /// "Queen", ordered by relevance rather than popularity, with 131, 344, 7
    /// and 12,800,469 fans; the band is fifth overall. The first exact match's
    /// radio is one track, "Cry No More", which is not Queen's.
    #[tokio::test]
    async fn of_several_artists_with_the_seed_name_the_most_followed_is_played() {
        const QUEENS: (u16, &str) = (
            200,
            r#"{"data":[
            {"id":183179807,"name":"Queen","nb_fan":131},
            {"id":61045802,"name":"Queen","nb_fan":344},
            {"id":135041032,"name":"Queen(Ares)","nb_fan":167},
            {"id":268175642,"name":"Queen","nb_fan":7},
            {"id":412,"name":"Queen","nb_fan":12800469},
            {"id":9019,"name":"Ivy Queen","nb_fan":128225}],"total":6}"#,
        );
        let (base, _, seen) = serve(vec![QUEENS, RADIO]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let seed = Seed {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
            mbid: None,
            confidence: 50,
        };
        let _ = p.recommend(&ended(), Some(&seed), 5).await.unwrap();
        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[1].lines().next(),
            Some("GET /artist/412/radio?limit=6 HTTP/1.1"),
            "request 2: {}",
            reqs[1]
        );
    }

    #[tokio::test]
    async fn no_artist_by_that_exact_name_plays_no_radio() {
        let (base, hits, _seen) = serve(vec![
            (
                200,
                r#"{"data":[{"id":1,"name":"The Offspring Tribute Band"}],"total":1}"#,
            ),
            RADIO,
        ])
        .await;
        let p = Deezer::with_base_url(base).expect("client builds");
        assert!(p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 1, "must NOT reach the radio");
    }

    #[tokio::test]
    async fn an_empty_search_plays_no_radio() {
        let (base, hits, _seen) = serve(vec![(200, r#"{"data":[],"total":0}"#), RADIO]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        assert!(p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_artist_match_ignores_case_and_curly_apostrophes() {
        let (base, hits, _seen) = serve(vec![
            (
                200,
                r#"{"data":[{"id":3,"name":"guns n’ ROSES"}],"total":1}"#,
            ),
            RADIO,
        ])
        .await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let seed = Seed {
            artist: "Guns N' Roses".into(),
            title: "Sweet Child O' Mine".into(),
            mbid: None,
            confidence: 100,
        };
        assert!(!p
            .recommend(&ended(), Some(&seed), 5)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn no_seed_or_nothing_wanted_makes_no_request() {
        let (base, hits, _seen) = serve(vec![SEARCH, RADIO]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        assert!(p.recommend(&ended(), None, 5).await.unwrap().is_empty());
        assert!(p
            .recommend(&ended(), Some(&seed()), 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    /// Measured: `artist/999999999999/radio`.
    #[tokio::test]
    async fn an_error_in_a_200_body_is_an_error_not_an_empty_page() {
        let (base, _, _seen) = serve(vec![(
            200,
            r#"{"error":{"type":"DataException","message":"no data","code":800}}"#,
        )])
        .await;
        let p = Deezer::with_base_url(base).expect("client builds");
        match p.recommend(&ended(), Some(&seed()), 5).await {
            Err(Error::UnexpectedBody { message, .. }) => {
                assert!(
                    message.contains("no data") && message.contains("800"),
                    "{message}"
                )
            },
            other => panic!("expected UnexpectedBody, got {other:?}"),
        }
    }

    /// Measured: `artist/0/radio`, whose error carries no `code`.
    #[tokio::test]
    async fn an_error_without_a_code_is_still_an_error() {
        let (base, _, _seen) = serve(vec![(
            200,
            r#"{"error":{"type":"Exception","message":"An error has occured"}}"#,
        )])
        .await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let err = p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .expect_err("an error body");
        assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
    }

    #[tokio::test]
    async fn the_quota_error_is_a_rate_limit() {
        let (base, _, _seen) = serve(vec![(
            200,
            r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#,
        )])
        .await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let err = p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .expect_err("quota");
        assert!(matches!(err, Error::RateLimited { .. }), "got {err}");
    }

    #[tokio::test]
    async fn a_server_fault_is_transient() {
        let (base, _, _seen) = serve(vec![(500, "Internal Server Error")]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let err = p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .expect_err("500");
        assert!(err.is_transient(), "got {err}");
    }

    #[tokio::test]
    async fn a_body_that_is_neither_a_page_nor_an_error_is_an_error() {
        let (base, _, _seen) = serve(vec![(200, r#"{"total":0}"#)]).await;
        let p = Deezer::with_base_url(base).expect("client builds");
        let err = p
            .recommend(&ended(), Some(&seed()), 5)
            .await
            .expect_err("no data, no error");
        assert!(matches!(err, Error::UnexpectedBody { .. }), "got {err}");
    }
}
