//! MusicBrainz as a seed canonicalizer, not a recommender: it has no
//! similar-track capability at all, but it CONFIRMS a title-parsed guess so a
//! metered call (musicatlas, ReccoBeats) is never spent on a bad one.
//!
//! 🪤 Ruling 31 (measured against the live API): MusicBrainz's `score` is NOT
//! a confidence signal. `artist:"Queen" AND recording:"Love"` returned 967
//! hits -- "Mother Love", "Love Kills" and everything else with "Love"
//! anywhere in the title -- ALL scoring 100. A phrase matching inside a
//! longer title scores identically to an exact match. Treating `score` as
//! confidence would "confirm" the wrong song and spend a metered call on it.
//! Confirmation here is therefore an EXACT match (title and first-credit
//! artist, both normalized) against one of up to 5 candidates, not a score
//! threshold. Anything short of an exact match is `Ok(None)`, so the caller
//! falls back to the unverified parse (spec §9: "any failure falls back").

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

/// 🪤 HARD limit, enforced against the IP: exceeding it blocks every service
/// on this host, not just one guild's autoplay.
///
/// 🔑 Ruling 29 (Task 5 review): the gate spaces the START of each call (see
/// `throttle`'s doc), not arrivals on the wire -- connection setup (DNS, TCP,
/// TLS) happens inside `send()`, after a slot has already been granted. The
/// measured floor is 1s; the extra 100ms is a margin so ordinary
/// connection-setup variance (a cold connection vs. a pooled one) cannot
/// compress two callers' actual wire arrivals below 1s. Holding the gate
/// across `send()` itself was rejected: a hung call would then stall every
/// other resolve for up to the client's 10s timeout, on the track-end path.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1_100);

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

/// 🪤 No `score` field. Ruling 31: MusicBrainz's relevance score is not
/// trustworthy as a confidence signal (see the module doc), so it is never
/// read -- and an out-of-range or malformed value in it can no longer fail
/// this parse either.
#[derive(Debug, Deserialize)]
struct Recording {
    id: String,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<Credit>,
}

#[derive(Debug, Deserialize)]
struct Credit {
    name: String,
}

/// A MusicBrainz seed canonicalizer: CONFIRMS a [`TitleParseResolver`] guess
/// against MusicBrainz's recording search by exact (normalized) match, rather
/// than trusting the search's own relevance score (see the module doc).
///
/// 🔑 The 1 req/sec gate ([`MIN_INTERVAL`]) lives in `self.gate`, an
/// instance-local mutex. It spaces the START of each call -- when a caller is
/// GRANTED a slot -- not when the request actually reaches MusicBrainz on the
/// wire (see [`MIN_INTERVAL`]'s doc). The limit is enforced by MusicBrainz
/// against the calling IP, so the process must hold exactly ONE shared
/// `MusicBrainz` (behind an `Arc`, owned by Task 6's orchestrator): two
/// independent instances would each obey 1/sec internally while jointly
/// sending 2/sec from the same address.
#[derive(Debug)]
pub struct MusicBrainz {
    base_url: String,
    http: reqwest::Client,
    gate: Mutex<Option<Instant>>,
}

impl MusicBrainz {
    /// # Errors
    /// [`Error::Config`] if `contact` is empty or whitespace-only, `base_url`
    /// is not a valid `http`/`https` URL, or the HTTP client cannot be built.
    pub fn new(contact: &str) -> Result<Self> {
        Self::with_base_url(contact, DEFAULT_BASE_URL)
    }

    /// The same, against a different host. Tests point this at a local mock.
    ///
    /// # Errors
    /// [`Error::Config`] if `contact` is empty or whitespace-only, `base_url`
    /// is not a valid `http`/`https` URL, or the HTTP client cannot be built.
    pub fn with_base_url(contact: &str, base_url: impl Into<String>) -> Result<Self> {
        // 🪤 A contact-less UA is not merely impolite here: MusicBrainz's
        // policy is to IP-ban clients that omit one, which would take down
        // every provider on the host, not just this resolver.
        let contact = contact.trim();
        if contact.is_empty() {
            return Err(Error::Config(
                "MusicBrainz requires a contact address in the User-Agent (an empty one risks an IP ban for the whole host)".into(),
            ));
        }
        let base_url = base_url.into();
        // 🪤 Ruling 30 (Task 5 review): reqwest defers URL parsing to
        // `send()`, so a malformed base URL used to surface as a transient
        // `Error::Transport` -- and spend a throttled slot -- on the FIRST
        // resolve, instead of failing at construction where it belongs.
        http::validate_base_url(NAME, &base_url)?;
        let ua = format!("cracktunes/{} ( {contact} )", env!("CARGO_PKG_VERSION"));
        Ok(Self {
            base_url,
            http: http::client(NAME, &ua)?,
            gate: Mutex::new(None),
        })
    }

    /// Block until at least [`MIN_INTERVAL`] has elapsed since the previous
    /// call to this method returned, on this instance -- i.e. spaces slot
    /// GRANTS, not the requests those grants lead to (see the struct doc).
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

    /// Normalize a title or artist name for exact-match confirmation:
    /// Unicode-lowercase, collapse/trim whitespace, and treat a typographic
    /// right single quote the same as an ASCII apostrophe -- MusicBrainz's
    /// canonical text favors the former ("Guns N’ Roses"); YouTube titles
    /// almost always use the latter.
    fn normalize(s: &str) -> String {
        s.replace('\u{2019}', "'")
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
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
        // 🪤 Ruling 31: `limit=5`, not `1`. MusicBrainz can rank a
        // same-scoring, wrong recording (module doc) ahead of the real one,
        // so the first result alone is not enough to search for an exact
        // match among.
        let url = format!(
            "{}/recording?query={}&fmt=json&limit=5",
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

        let want_artist = Self::normalize(&guess.artist);
        let want_title = Self::normalize(&guess.title);
        // 🪤 Ruling 31: the FIRST EXACT match, not `recordings[0]`. Anything
        // short of an exact match on BOTH fields is not confirmation, so
        // `find_map` yielding `None` here is deliberately the same `Ok(None)`
        // as "no recordings at all" -- the caller falls back to the parse
        // either way.
        Ok(parsed.recordings.into_iter().find_map(|r| {
            // Cloning immediately ends the borrow of `r.artist_credit` so
            // `r.title`/`r.id` can be moved out below without a conflict.
            let artist = r.artist_credit.first()?.name.clone();
            if Self::normalize(&artist) != want_artist || Self::normalize(&r.title) != want_title {
                return None;
            }
            Some(Seed {
                artist,
                title: r.title,
                mbid: Some(r.id),
                confidence: 100,
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{serve, Canned};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    /// An exact match for `track("Queen - Bohemian Rhapsody")`.
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

    /// `normalize` in isolation, per the review's request for its own unit
    /// test -- every other test here only exercises it indirectly through
    /// `resolve()`.
    #[test]
    fn normalize_folds_case_whitespace_and_curly_apostrophes() {
        assert_eq!(MusicBrainz::normalize("Queen"), "queen");
        assert_eq!(
            MusicBrainz::normalize("  Sweet   Child O\u{2019} Mine  "),
            "sweet child o' mine"
        );
        assert_eq!(
            MusicBrainz::normalize("queen  -  BOHEMIAN Rhapsody"),
            "queen - bohemian rhapsody"
        );
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

    /// Ruling 31's trap fixture, reproducing the measurement verbatim:
    /// against the live API, `artist:"Queen" AND recording:"Love"` returned
    /// "Mother Love" and "Love Kills" (and 965 others), ALL scoring 100 --
    /// a phrase match anywhere in the title scores exactly like an exact
    /// one. Treating `score` as confidence would confirm one of these and
    /// spend a metered musicatlas call on the wrong track. `hits == 1`:
    /// this must cost exactly the one search request, not a retry.
    #[tokio::test]
    async fn a_substring_match_at_score_100_does_not_confirm() {
        let (base, hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[
                {"id":"mbid-wrong-1","score":100,"title":"Mother Love","artist-credit":[{"name":"Queen"}]},
                {"id":"mbid-wrong-2","score":100,"title":"Love Kills","artist-credit":[{"name":"Queen"}]}
            ]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let got = mb.resolve(&track("Queen - Love")).await.unwrap();
        assert!(
            got.is_none(),
            "neither candidate is an exact title match, however high their score"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// Ruling 31: an exact match need not be first. Same guess as the trap
    /// fixture above, but this time a real "Love" IS among the candidates,
    /// behind the non-matching "Mother Love" -- it must still be found, with
    /// its own mbid and confidence 100.
    #[tokio::test]
    async fn an_exact_match_behind_a_non_matching_candidate_is_still_found() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[
                {"id":"mbid-wrong-1","score":100,"title":"Mother Love","artist-credit":[{"name":"Queen"}]},
                {"id":"mbid-love","score":100,"title":"Love","artist-credit":[{"name":"Queen"}]}
            ]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Queen - Love"))
            .await
            .unwrap()
            .expect("the real match, even though it is not first");
        assert_eq!(seed.mbid.as_deref(), Some("mbid-love"));
        assert_eq!(seed.confidence, 100);
    }

    /// Ruling 31: an artist-credit that names a DIFFERENT (even overlapping)
    /// artist must not confirm, even though the title matches exactly.
    #[tokio::test]
    async fn a_title_match_with_a_mismatched_artist_credit_does_not_confirm() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[{"id":"mbid-collab","score":100,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen + Adam Lambert"}]}]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let got = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap();
        assert!(
            got.is_none(),
            "the title matches but the credited artist does not"
        );
    }

    /// Ruling 31: the messy side is usually the GUESS (a YouTube title), not
    /// MusicBrainz's canonical text -- lowercase and doubled internal
    /// whitespace here must still confirm, and the returned seed carries
    /// MusicBrainz's own (clean) casing, not the guess's.
    #[tokio::test]
    async fn a_lowercase_double_spaced_guess_still_confirms_and_the_seed_carries_musicbrainzs_casing(
    ) {
        let (base, _hits, _seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("queen - bohemian  rhapsody"))
            .await
            .unwrap()
            .expect("case and whitespace differences must not block confirmation");
        assert_eq!(seed.artist, "Queen");
        assert_eq!(seed.title, "Bohemian Rhapsody");
    }

    /// Ruling 31: confidence is always 100 on an exact match, never the
    /// recording's own `score` -- a low score (42, chosen to be neither the
    /// parse's 50 nor a hardcoded 100) on an otherwise exact match must not
    /// leak through.
    #[tokio::test]
    async fn confidence_is_100_on_an_exact_match_regardless_of_the_recordings_own_score() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[{"id":"mbid-2","score":42,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"}]}]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap()
            .expect("an exact title+artist match");
        assert_eq!(seed.confidence, 100);
    }

    /// Ruling 31: the first EXACT match among several candidates wins, not
    /// `recordings[0]` -- a non-matching candidate ranked ahead of the real
    /// one (as MusicBrainz does for "Love", see the module doc) must be
    /// skipped rather than returned or treated as a failure.
    #[tokio::test]
    async fn the_first_exact_match_among_several_candidates_wins() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[
                {"id":"mbid-a","score":100,"title":"Mother Love","artist-credit":[{"name":"Queen"}]},
                {"id":"mbid-b","score":100,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"}]},
                {"id":"mbid-c","score":100,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"}]}
            ]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap()
            .expect("an exact match exists among the candidates");
        assert_eq!(
            seed.mbid.as_deref(),
            Some("mbid-b"),
            "the first exact match, skipping the non-matching one ahead of it"
        );
    }

    /// Normalization must absorb the differences actually observed between a
    /// YouTube title and MusicBrainz's canonical text: case, extra internal
    /// whitespace, and a typographic apostrophe.
    #[tokio::test]
    async fn normalization_ignores_case_whitespace_and_curly_apostrophes() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            "{\"recordings\":[{\"id\":\"mbid-n\",\"score\":100,\"title\":\"  Sweet   Child O\\u2019 Mine\",\"artist-credit\":[{\"name\":\"guns n' roses\"}]}]}",
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Guns N' Roses - Sweet Child O' Mine"))
            .await
            .unwrap()
            .expect("normalization should still confirm this as an exact match");
        assert_eq!(seed.mbid.as_deref(), Some("mbid-n"));
        assert_eq!(seed.confidence, 100);
    }

    /// L7 (Task 5 review), Ruling 31 shape: with no credit at all there is
    /// nothing to confirm the artist against, so the candidate cannot match
    /// -- this is not "fall back to the guess's artist", it is "not a
    /// confirmation".
    #[tokio::test]
    async fn an_empty_artist_credit_cannot_confirm_a_match() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[{"id":"mbid-4","score":90,"title":"Bohemian Rhapsody","artist-credit":[]}]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let got = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap();
        assert!(
            got.is_none(),
            "no credit to compare against the guess's artist means no confirmation"
        );
    }

    /// L7 (Task 5 review): only the first credit counts, both for matching
    /// and for the seed's artist field -- a join-phrase or a second artist
    /// must not leak in.
    #[tokio::test]
    async fn two_artist_credits_use_only_the_first() {
        let (base, _hits, _seen) = serve(vec![(
            200,
            r#"{"recordings":[{"id":"mbid-5","score":90,"title":"Bohemian Rhapsody","artist-credit":[{"name":"Queen"},{"name":"David Bowie"}]}]}"#,
        )])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let seed = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .unwrap()
            .expect("the first credit matches the guess's artist");
        assert_eq!(seed.artist, "Queen");
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

    /// L6 (Task 5 review): fixed at the source in `title_parse.rs`, but
    /// re-asserted here at the MusicBrainz boundary -- a supplied artist with
    /// a title that cleans to empty must not spend a slot on a query that
    /// cannot possibly match.
    #[tokio::test]
    async fn a_supplied_artist_with_a_title_that_cleans_to_empty_makes_no_request() {
        let (base, hits, _seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let raw = RawTrack {
            title: "(Official Video)".into(),
            artist: Some("Queen".into()),
            uploader: None,
        };
        let got = mb.resolve(&raw).await.unwrap();
        assert!(got.is_none());
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "an empty-after-cleaning title must not spend a slot"
        );
    }

    /// R16 / I3 (Task 5 review): the WHOLE request line, compared against a
    /// LITERAL expected string rather than one built with `encode_query`
    /// itself -- self-referential assertions can pass a broken encoder that
    /// is merely internally consistent. Covers an artist that needs quoting
    /// to stay one term, and a title whose quote and `&` need
    /// escaping/encoding.
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
        assert_eq!(
            reqs[0].lines().next(),
            Some(
                "GET /recording?query=artist%3A%22Guns%20N%27%20Roses%22%20AND%20recording%3A%22Song%20%5C%22Live%5C%22%20%26%20Loud%22&fmt=json&limit=5 HTTP/1.1"
            ),
            "request: {}",
            reqs[0]
        );
    }

    /// L1 (Task 5 review): the `\` -> `\\` escape had no test containing an
    /// actual backslash, so dropping that pass survived undetected -- the
    /// other wire test's `"` exercises the SECOND replace only.
    #[tokio::test]
    async fn a_title_ending_in_a_backslash_is_escaped_on_the_wire() {
        let (base, _hits, seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let _ = mb.resolve(&track("Queen - Song\\")).await;

        let reqs = seen.lock().expect("test mutex");
        assert_eq!(
            reqs[0].lines().next(),
            Some(
                "GET /recording?query=artist%3A%22Queen%22%20AND%20recording%3A%22Song%5C%5C%22&fmt=json&limit=5 HTTP/1.1"
            ),
            "request: {}",
            reqs[0]
        );
    }

    /// L8 (Task 5 review): the whole `user-agent:` line, not just "a header
    /// exists and the contact appears somewhere" -- `format!("( {contact}
    /// )")` alone (no app name or version) used to survive.
    #[tokio::test]
    async fn the_user_agent_carries_the_app_name_version_and_contact_address() {
        let (base, _hits, seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("ops@cracktun.es", base).expect("client builds");
        let _ = mb.resolve(&track("Queen - Bohemian Rhapsody")).await;

        let reqs = seen.lock().expect("test mutex");
        let ua_line = reqs[0]
            .lines()
            .find(|l| l.to_lowercase().starts_with("user-agent:"))
            .expect("a user-agent header");
        assert_eq!(
            ua_line,
            format!(
                "user-agent: cracktunes/{} ( ops@cracktun.es )",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    /// I2 (Task 5 review): the contact is trimmed before being interpolated
    /// into the UA, not just checked for blankness and then used raw.
    #[tokio::test]
    async fn the_contact_is_trimmed_in_the_user_agent() {
        let (base, _hits, seen) = serve(vec![HIT]).await;
        let mb = MusicBrainz::with_base_url("  ops@cracktun.es  ", base).expect("client builds");
        let _ = mb.resolve(&track("Queen - Bohemian Rhapsody")).await;

        let reqs = seen.lock().expect("test mutex");
        let ua_line = reqs[0]
            .lines()
            .find(|l| l.to_lowercase().starts_with("user-agent:"))
            .expect("a user-agent header");
        assert_eq!(
            ua_line,
            format!(
                "user-agent: cracktunes/{} ( ops@cracktun.es )",
                env!("CARGO_PKG_VERSION")
            )
        );
    }

    #[tokio::test]
    async fn an_empty_contact_is_a_config_error() {
        let err = MusicBrainz::new("   ").expect_err("blank contact must be rejected");
        assert!(matches!(err, Error::Config(_)), "got {err}");
    }

    /// L4 / Ruling 30 (Task 5 review): rejected at construction, not spent
    /// as a throttled slot on a request that never leaves the process.
    #[test]
    fn with_base_url_rejects_a_malformed_url() {
        let err = MusicBrainz::with_base_url("a@b.c", "not a url")
            .expect_err("must be rejected before any request");
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

    /// L2 (Task 5 review): the only 5xx fixture used to be 503, so narrowing
    /// `status >= 500` to `status == 503` survived undetected.
    #[tokio::test]
    async fn a_non_503_5xx_is_still_rate_limited() {
        let (base, _hits, _seen) = serve(vec![(500, "<html>oops</html>")]).await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("500");
        assert!(matches!(err, Error::RateLimited { .. }), "got {err}");
        assert!(err.is_transient(), "any 5xx must be retried, not just 503");
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

    /// M2 (Task 5 review): nothing previously pinned MusicBrainz to the
    /// shared client's redirect policy from MusicBrainz's own test suite. A
    /// followed redirect would bypass the 1/sec gate entirely (the second
    /// request happens inside the same `send()`, after `throttle()` already
    /// granted the slot).
    #[tokio::test]
    async fn a_redirect_is_refused_rather_than_followed() {
        let (base, hits, _seen) = serve(vec![
            Canned {
                status: 302,
                body: "",
                headers: &[("Location", "http://{addr}/again")],
            },
            HIT.into(),
        ])
        .await;
        let mb = MusicBrainz::with_base_url("a@b.c", base).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("a redirect must not be followed");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a followed redirect would be a second, unthrottled request"
        );
        match &err {
            Error::UnexpectedBody { message, .. } => {
                assert!(message.contains("302"), "message: {message}");
            },
            other => panic!("expected UnexpectedBody naming 302, got {other:?}"),
        }
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

    /// No test in the crate asserted the `Transport` mapping until Task 5 --
    /// every prior transport-shaped test used a hung peer (a timeout), not a
    /// refused connection. Through `resolve`, not reqwest directly.
    ///
    /// 🪤 Speculation (Task 5 review), P16: a bound-but-never-`listen()`ed
    /// `TcpSocket`, kept alive for the whole test, rather than binding then
    /// DROPPING a listener. A dropped listener frees its port immediately,
    /// which an unrelated, concurrently running mock server in this same
    /// suite could re-bind before `resolve` connects -- making the refusal
    /// flaky instead of deterministic. This socket is never dropped until
    /// the assertions are done, so the port cannot be stolen out from under
    /// the test.
    #[tokio::test]
    async fn a_refused_connection_is_a_transient_transport_error() {
        let socket = tokio::net::TcpSocket::new_v4().expect("socket");
        socket
            .bind("127.0.0.1:0".parse().expect("addr"))
            .expect("bind");
        let addr = socket.local_addr().expect("local_addr");

        let mb =
            MusicBrainz::with_base_url("a@b.c", format!("http://{addr}")).expect("client builds");
        let err = mb
            .resolve(&track("Queen - Bohemian Rhapsody"))
            .await
            .expect_err("nothing is listening");
        assert!(matches!(err, Error::Transport { .. }), "got {err}");
        assert!(err.is_transient(), "a refused connection should be retried");
        drop(socket); // kept alive until here so the port stays ours throughout
    }

    /// R18: the 1/sec floor is a property of ONE instance's gate, pinned
    /// without touching the network -- real TCP under paused time is
    /// meaningless (auto-advance during IO waits hides the floor entirely).
    /// 🪤 Pinned against a LITERAL, not `MIN_INTERVAL` itself -- comparing
    /// elapsed time to `2 * MIN_INTERVAL` would make the sabotage
    /// "`MIN_INTERVAL` -> 0" invisible: the threshold collapses to 0 right
    /// alongside the thing being measured, and `elapsed() >= 0` always holds.
    #[test]
    fn min_interval_is_the_measured_floor_plus_its_margin() {
        assert_eq!(MIN_INTERVAL, Duration::from_millis(1_100));
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

    /// H1 (Task 5 review): the test above measures TOTAL elapsed time, which
    /// a throttle where both callers sleep in PARALLEL and return together
    /// also satisfies -- a gate that releases its lock before sleeping, or
    /// one with no lock at all, left it green. This measures the GAP between
    /// each pair of SORTED return times instead, which only a genuinely
    /// serializing gate can satisfy. Pre-warming the gate with one call
    /// first also matters: on a fresh gate, tokio's current-thread runtime
    /// lets the first of two callers finish an uncontended lock before the
    /// second even reads `last`, hiding the exact race this test exists to
    /// catch.
    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_are_spaced_from_each_other_not_just_from_the_start() {
        let mb = Arc::new(
            MusicBrainz::with_base_url("a@b.c", "http://unused.invalid").expect("client builds"),
        );
        mb.throttle().await; // pre-warm: a request just went out
        let mut times = vec![Instant::now()];
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let mb = Arc::clone(&mb);
                tokio::spawn(async move {
                    mb.throttle().await;
                    Instant::now()
                })
            })
            .collect();
        for h in handles {
            times.push(h.await.expect("task"));
        }
        times.sort();
        for w in times.windows(2) {
            assert!(
                w[1] - w[0] >= Duration::from_secs(1),
                "gap too small: {times:?}"
            );
        }
    }

    /// L5 (Task 5 review), P09: the no-separator short-circuit is asserted
    /// by request count elsewhere, but that alone would not catch
    /// `throttle()` moved ABOVE it -- that mutation keeps `hits == 0` while
    /// still burning a slot. Paused time makes a burned slot visible as
    /// nonzero elapsed time on the very next call.
    #[tokio::test(start_paused = true)]
    async fn a_no_separator_resolve_spends_no_slot_on_the_gate() {
        let mb =
            MusicBrainz::with_base_url("a@b.c", "http://unused.invalid").expect("client builds");
        let _ = mb.resolve(&track("Never Gonna Give You Up")).await;
        let start = Instant::now();
        mb.throttle().await;
        assert_eq!(
            start.elapsed(),
            Duration::ZERO,
            "no slot was spent by the short-circuited resolve"
        );
    }

    /// L5 (Task 5 review), P17: the first-ever call on a fresh gate must not
    /// wait -- a regression that makes it wait a full `MIN_INTERVAL` adds
    /// latency to the first autoplay after every process start, and nothing
    /// else in this file isolates that case from ordinary spacing.
    #[tokio::test(start_paused = true)]
    async fn a_fresh_gates_first_throttle_call_is_free() {
        let mb =
            MusicBrainz::with_base_url("a@b.c", "http://unused.invalid").expect("client builds");
        let start = Instant::now();
        mb.throttle().await;
        assert_eq!(
            start.elapsed(),
            Duration::ZERO,
            "the first-ever call must not wait"
        );
    }
}
