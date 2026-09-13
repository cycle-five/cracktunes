//! The orchestrator: turns "the track that just ended" into recommendations by
//! consulting resolvers for a seed, then trying recommenders in order until
//! one answers.
//!
//! 🔑 Fallback is per-provider, not global (spec §3.2): a musicatlas quota
//! exhaustion falls through to ReccoBeats instead of ending autoplay. That is
//! the entire point of this crate, so [`MusicReco::next_tracks`] never
//! returns `Err` for a provider failure -- an empty `Vec` means every avenue
//! was tried.

use crate::provider::Recommender;
use crate::resolver::SeedResolver;
use crate::{Error, RawTrack, Recommendation, Result, Seed};
use std::sync::atomic::{AtomicBool, Ordering};

/// Our own ceiling on how many recommendations to ask for, not a measured
/// provider limit (Task 4 review L4): musicatlas returns 20 matches per call,
/// so asking any provider for more than that is asking for something no
/// provider in this crate can supply anyway.
const MAX_WANT: usize = 20;

/// Tunables that are policy rather than protocol.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Below this, METERED providers are skipped instead of guessed at. A
    /// bare title parse scores 50; a caller-supplied artist or a
    /// MusicBrainz-confirmed seed both score 100 (Ruling 31 measured the
    /// latter as binary -- exact match or nothing) -- so the default floor of
    /// 80 reads as "only spend metered calls on a supplied or confirmed
    /// seed".
    pub min_seed_confidence: u8,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            min_seed_confidence: 80,
        }
    }
}

#[derive(Default)]
pub struct MusicRecoBuilder {
    resolvers: Vec<Box<dyn SeedResolver>>,
    recommenders: Vec<Box<dyn Recommender>>,
    policy: Policy,
}

impl MusicRecoBuilder {
    #[must_use]
    pub fn resolver(mut self, r: Box<dyn SeedResolver>) -> Self {
        self.resolvers.push(r);
        self
    }

    #[must_use]
    pub fn recommender(mut self, r: Box<dyn Recommender>) -> Self {
        self.recommenders.push(r);
        self
    }

    #[must_use]
    pub fn policy(mut self, p: Policy) -> Self {
        self.policy = p;
        self
    }

    #[must_use]
    pub fn build(self) -> MusicReco {
        // Ruling 23: one flag PER recommender, built here (rather than taken
        // as a builder argument) so `.recommender()`'s public signature stays
        // just a `Box<dyn Recommender>`.
        let disabled = self
            .recommenders
            .iter()
            .map(|_| AtomicBool::new(false))
            .collect();
        MusicReco {
            resolvers: self.resolvers,
            recommenders: self.recommenders,
            disabled,
            policy: self.policy,
        }
    }
}

/// The orchestrator. Build one per process (spec §9, Ruling 18/23/24 carried
/// from Task 5): MusicBrainz's 1/sec gate, a recommender's disabled flag, and
/// ReccoBeats' own cooldown are all per-instance state that a fresh
/// `MusicReco` per call or per guild would silently reset.
pub struct MusicReco {
    resolvers: Vec<Box<dyn SeedResolver>>,
    recommenders: Vec<Box<dyn Recommender>>,
    /// Parallel to `recommenders`, by index. Ruling 23 / spec §9: an
    /// `Error::InvalidKey` disables that recommender for the life of the
    /// process -- an operator has to rotate the credential, and nothing here
    /// will make a bad key good again by retrying it on every track end.
    disabled: Vec<AtomicBool>,
    policy: Policy,
}

impl MusicReco {
    #[must_use]
    pub fn builder() -> MusicRecoBuilder {
        MusicRecoBuilder::default()
    }

    /// The recommenders, in the order they are tried. For the startup log line
    /// and for tests of which providers a deployment actually wired up.
    #[must_use]
    pub fn recommender_names(&self) -> Vec<&'static str> {
        self.recommenders.iter().map(|r| r.name()).collect()
    }

    /// The best seed any resolver produced.
    ///
    /// Ruling 22: resolvers may only RAISE confidence -- MusicBrainz confirms
    /// a title parse but never replaces a caller-supplied artist with a worse
    /// guess -- so once the running best reaches 100 nothing later could
    /// possibly change the outcome. Stopping there keeps a supplied-artist
    /// seed from spending MusicBrainz's 1/sec slot on every single track end.
    async fn best_seed(&self, raw: &RawTrack) -> Option<Seed> {
        let mut best: Option<Seed> = None;
        for r in &self.resolvers {
            if best.as_ref().is_some_and(|b| b.confidence >= 100) {
                break;
            }
            match r.resolve(raw).await {
                Ok(Some(s)) => {
                    if best.as_ref().is_none_or(|b| s.confidence > b.confidence) {
                        best = Some(s);
                    }
                },
                Ok(None) => {},
                // Ruling 27 / spec §9 "MusicBrainz any failure": a resolver
                // failing is not fatal. Whatever `best` already holds (an
                // offline parse, or nothing) still stands.
                Err(e) => log_at(r.name(), &e),
            }
        }
        best
    }

    /// Candidate next-tracks, or an empty vec when nothing could be produced.
    ///
    /// 🔑 Never returns `Err` for a provider failure: a provider is allowed to
    /// fail, and falling through to the next is the reason this crate exists.
    /// An empty result means every avenue was tried.
    ///
    /// # Errors
    /// Currently infallible; the signature keeps room for a future fatal case.
    pub async fn next_tracks(&self, raw: &RawTrack, want: usize) -> Result<Vec<Recommendation>> {
        // Ruling 25: `want == 0` spends NOTHING -- not even seed resolution,
        // so not even a MusicBrainz slot.
        if want == 0 {
            return Ok(Vec::new());
        }
        // Our own ceiling, not a provider one (see `MAX_WANT`'s doc).
        let want = want.min(MAX_WANT);

        let Some(seed) = self.best_seed(raw).await else {
            tracing::debug!("no seed derivable from {:?}; spending nothing", raw.title);
            return Ok(Vec::new());
        };
        let shaky = seed.confidence < self.policy.min_seed_confidence;

        for (r, disabled) in self.recommenders.iter().zip(&self.disabled) {
            // Ruling 23: a disabled recommender is skipped before `recommend`
            // is even called.
            if disabled.load(Ordering::SeqCst) {
                continue;
            }
            // 🔑 The floor gates METERED providers only (see
            // `Recommender::is_metered`'s doc). Skipping free ones too would
            // make a MusicBrainz outage fatal to autoplay.
            if shaky && r.is_metered() {
                tracing::debug!(
                    "seed `{} - {}` scored {} (< {}); skipping metered {}",
                    seed.artist,
                    seed.title,
                    seed.confidence,
                    self.policy.min_seed_confidence,
                    r.name()
                );
                continue;
            }
            match r.recommend(&seed, want).await {
                Ok(v) if !v.is_empty() => return Ok(v),
                Ok(_) => tracing::debug!("{} had nothing for `{}`", r.name(), seed.title),
                Err(Error::InvalidKey { message, .. }) => {
                    // Ruling 23 / spec §9: log ERROR once, at the moment it
                    // trips, then never call this recommender again for the
                    // life of the process. `swap`, not `store`: two guilds'
                    // track ends can both be mid-call when the key goes bad,
                    // and only the one that flips the flag logs.
                    if !disabled.swap(true, Ordering::SeqCst) {
                        tracing::error!(
                            "{} rejected our credential, disabling it for the process: {message}",
                            r.name()
                        );
                    }
                },
                Err(e) => log_at(r.name(), &e),
            }
        }
        Ok(Vec::new())
    }
}

/// Ruling 26 (the #516 rule: don't log expected, budgeted outcomes as
/// failures). `BudgetExhausted`/`RateLimited` are the daily and expected
/// shape of "try the next provider"; anything else is unexpected enough to
/// warrant a `warn!`. `InvalidKey` from a recommender has its own arm in
/// `next_tracks` (it also disables the provider) and never reaches here; a
/// resolver cannot produce it at all today.
fn log_at(name: &str, e: &Error) {
    match e {
        Error::BudgetExhausted { .. } | Error::RateLimited { .. } => {
            tracing::debug!("{name}: {e}");
        },
        _ => tracing::warn!("{name}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use crate::provider::Recommender;
    use crate::resolver::SeedResolver;
    use crate::TitleParseResolver;
    use crate::{Error, MusicReco, Playable, Policy, RawTrack, Recommendation, Result, Seed};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct FakeReco {
        name: &'static str,
        calls: Arc<AtomicUsize>,
        result: fn() -> Result<Vec<Recommendation>>,
    }

    #[async_trait::async_trait]
    impl Recommender for FakeReco {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn recommend(&self, _s: &Seed, _w: usize) -> Result<Vec<Recommendation>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.result)()
        }
    }

    fn one(src: &'static str) -> Result<Vec<Recommendation>> {
        Ok(vec![Recommendation {
            artist: "A".into(),
            title: "T".into(),
            playable: Playable::YouTubeId("v".into()),
            isrc: None,
            source: src.into(),
        }])
    }
    fn boom() -> Result<Vec<Recommendation>> {
        Err(Error::BudgetExhausted {
            provider: "first",
            budget: 90,
        })
    }
    fn empty() -> Result<Vec<Recommendation>> {
        Ok(vec![])
    }
    fn invalid_key() -> Result<Vec<Recommendation>> {
        Err(Error::InvalidKey {
            provider: "first",
            message: "bad key".into(),
        })
    }

    fn raw() -> RawTrack {
        RawTrack {
            title: "Queen - Bohemian Rhapsody".into(),
            artist: None,
            uploader: None,
        }
    }

    fn seed100() -> Seed {
        Seed {
            artist: "A".into(),
            title: "T".into(),
            mbid: None,
            confidence: 100,
        }
    }

    /// A counting fake resolver, so tests can prove HOW MANY times a resolver
    /// was consulted, not just what the final seed was.
    struct FakeResolver {
        name: &'static str,
        calls: Arc<AtomicUsize>,
        result: fn() -> Result<Option<Seed>>,
    }

    #[async_trait::async_trait]
    impl SeedResolver for FakeResolver {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn resolve(&self, _raw: &RawTrack) -> Result<Option<Seed>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.result)()
        }
    }

    fn confirmed() -> Result<Option<Seed>> {
        Ok(Some(seed100()))
    }
    fn resolver_boom() -> Result<Option<Seed>> {
        Err(Error::UnexpectedBody {
            provider: "musicbrainz",
            message: "boom".into(),
        })
    }

    #[tokio::test]
    async fn the_first_recommender_that_answers_wins_and_the_rest_are_not_called() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: || one("first"),
            }))
            .recommender(Box::new(FakeReco {
                name: "second",
                calls: Arc::clone(&b),
                result: || one("second"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "first");
        assert_eq!((a.load(Ordering::SeqCst), b.load(Ordering::SeqCst)), (1, 0));
    }

    /// 🔑 The entire point of the crate: one provider failing must not end
    /// autoplay.
    #[tokio::test]
    async fn a_failing_first_recommender_falls_through_to_the_second() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: boom,
            }))
            .recommender(Box::new(FakeReco {
                name: "second",
                calls: Arc::clone(&b),
                result: || one("second"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "second");
        assert_eq!((a.load(Ordering::SeqCst), b.load(Ordering::SeqCst)), (1, 1));
    }

    #[tokio::test]
    async fn an_empty_answer_also_falls_through() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: empty,
            }))
            .recommender(Box::new(FakeReco {
                name: "second",
                calls: Arc::clone(&b),
                result: || one("second"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        assert_eq!(r.next_tracks(&raw(), 5).await.unwrap()[0].source, "second");
        assert_eq!((a.load(Ordering::SeqCst), b.load(Ordering::SeqCst)), (1, 1));
    }

    #[tokio::test]
    async fn no_derivable_seed_calls_nothing() {
        let a = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: || one("first"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let raw = RawTrack {
            title: "Never Gonna Give You Up".into(),
            artist: None,
            uploader: None,
        };
        assert!(r.next_tracks(&raw, 5).await.unwrap().is_empty());
        assert_eq!(a.load(Ordering::SeqCst), 0, "no seed, no calls, no quota");
    }

    struct MeteredFake {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Recommender for MeteredFake {
        fn name(&self) -> &'static str {
            "metered"
        }
        fn is_metered(&self) -> bool {
            true
        }
        async fn recommend(&self, _s: &Seed, _w: usize) -> Result<Vec<Recommendation>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            one("metered")
        }
    }

    /// A bare parse scores 50. With a floor of 80 the METERED provider is
    /// skipped -- but the free one must still run, or a MusicBrainz outage
    /// would kill autoplay outright.
    #[tokio::test]
    async fn a_low_confidence_seed_skips_metered_providers_but_not_free_ones() {
        let (m, f) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(MeteredFake {
                calls: Arc::clone(&m),
            }))
            .recommender(Box::new(FakeReco {
                name: "free",
                calls: Arc::clone(&f),
                result: || one("free"),
            }))
            .policy(Policy {
                min_seed_confidence: 80,
            })
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "free");
        assert_eq!(m.load(Ordering::SeqCst), 0, "no quota spent on a guess");
        assert_eq!(f.load(Ordering::SeqCst), 1, "the free provider still ran");
    }

    /// The same shape, but against the actual `Policy::default()` rather than
    /// a hand-picked 80 -- proves the shipped default, not just that SOME
    /// floor works.
    #[tokio::test]
    async fn a_bare_parse_seed_skips_metered_under_the_default_floor() {
        let (m, f) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(MeteredFake {
                calls: Arc::clone(&m),
            }))
            .recommender(Box::new(FakeReco {
                name: "free",
                calls: Arc::clone(&f),
                result: || one("free"),
            }))
            .policy(Policy::default())
            .build();
        let out = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out[0].source, "free");
        assert_eq!(m.load(Ordering::SeqCst), 0);
        assert_eq!(f.load(Ordering::SeqCst), 1);
    }

    /// The real value a caller-supplied artist scores (Ruling 31 / Task 6
    /// dispatch): 100, which clears the default floor of 80 outright.
    #[tokio::test]
    async fn a_caller_supplied_artist_seed_clears_the_default_floor() {
        let m = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(MeteredFake {
                calls: Arc::clone(&m),
            }))
            .policy(Policy::default())
            .build();
        let raw = RawTrack {
            title: "Bohemian Rhapsody".into(),
            artist: Some("Queen".into()),
            uploader: None,
        };
        let out = r.next_tracks(&raw, 5).await.unwrap();
        assert_eq!(out[0].source, "metered");
        assert_eq!(m.load(Ordering::SeqCst), 1);
    }

    /// Ruling 22: resolvers can only RAISE confidence, so once one reaches
    /// 100 nothing later could possibly change the outcome -- consulting it
    /// anyway would just spend MusicBrainz's 1/sec slot for nothing.
    #[tokio::test]
    async fn once_a_resolver_reaches_full_confidence_no_later_resolver_runs() {
        let never = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(FakeResolver {
                name: "confirmed",
                calls: Arc::new(AtomicUsize::new(0)),
                result: confirmed,
            }))
            .resolver(Box::new(FakeResolver {
                name: "never-called",
                calls: Arc::clone(&never),
                result: confirmed,
            }))
            .recommender(Box::new(FakeReco {
                name: "r",
                calls: Arc::new(AtomicUsize::new(0)),
                result: || one("r"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let _ = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(
            never.load(Ordering::SeqCst),
            0,
            "confidence is already 100; nothing could raise it further"
        );
    }

    /// Ruling 23 / spec §9: an `InvalidKey` disables that recommender for the
    /// life of the process. A rotated key needing a restart to take effect is
    /// the intended behaviour, not a bug.
    #[tokio::test]
    async fn an_invalid_key_disables_the_recommender_for_the_process() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: invalid_key,
            }))
            .recommender(Box::new(FakeReco {
                name: "second",
                calls: Arc::clone(&b),
                result: || one("second"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let out1 = r.next_tracks(&raw(), 5).await.unwrap();
        let out2 = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(out1[0].source, "second");
        assert_eq!(out2[0].source, "second");
        assert_eq!(
            a.load(Ordering::SeqCst),
            1,
            "the first call trips InvalidKey; the second next_tracks must not call it again"
        );
        assert_eq!(
            b.load(Ordering::SeqCst),
            2,
            "the second recommender must answer both track ends"
        );
    }

    /// The mirror of the previous test: only `InvalidKey` earns the
    /// process-lifetime disable (spec §9 names that credential specifically).
    /// `BudgetExhausted` is the DAILY, expected shape of exhaustion -- it must
    /// not permanently silence a provider that a new day (or the current
    /// process's next call) might see recover.
    #[tokio::test]
    async fn a_budget_exhausted_error_does_not_disable_the_recommender() {
        let (a, b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(FakeReco {
                name: "first",
                calls: Arc::clone(&a),
                result: boom,
            }))
            .recommender(Box::new(FakeReco {
                name: "second",
                calls: Arc::clone(&b),
                result: || one("second"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let _ = r.next_tracks(&raw(), 5).await.unwrap();
        let _ = r.next_tracks(&raw(), 5).await.unwrap();
        assert_eq!(
            a.load(Ordering::SeqCst),
            2,
            "a non-InvalidKey error must not disable the recommender for later calls"
        );
        assert_eq!(b.load(Ordering::SeqCst), 2);
    }

    struct WantCapture {
        calls: Arc<AtomicUsize>,
        want_seen: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Recommender for WantCapture {
        fn name(&self) -> &'static str {
            "capture"
        }
        async fn recommend(&self, _s: &Seed, w: usize) -> Result<Vec<Recommendation>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.want_seen.store(w, Ordering::SeqCst);
            Ok(vec![])
        }
    }

    /// Ruling 25: `want` is clamped at the orchestrator to a ceiling that is
    /// OUR policy (musicatlas returns 20 matches/call), not a provider limit.
    #[tokio::test]
    async fn want_is_clamped_to_the_orchestrators_ceiling() {
        let (calls, want_seen) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .recommender(Box::new(WantCapture {
                calls: Arc::clone(&calls),
                want_seen: Arc::clone(&want_seen),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let _ = r.next_tracks(&raw(), usize::MAX).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            want_seen.load(Ordering::SeqCst),
            20,
            "clamped to the ceiling, not passed through raw"
        );
    }

    /// Ruling 25: `want == 0` spends NOTHING -- not even seed resolution, so
    /// not even a MusicBrainz slot.
    #[tokio::test]
    async fn want_zero_spends_nothing_not_even_seed_resolution() {
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let reco_calls = Arc::new(AtomicUsize::new(0));
        let r = MusicReco::builder()
            .resolver(Box::new(FakeResolver {
                name: "r",
                calls: Arc::clone(&resolver_calls),
                result: confirmed,
            }))
            .recommender(Box::new(FakeReco {
                name: "x",
                calls: Arc::clone(&reco_calls),
                result: || one("x"),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let out = r.next_tracks(&raw(), 0).await.unwrap();
        assert!(out.is_empty());
        assert_eq!(
            resolver_calls.load(Ordering::SeqCst),
            0,
            "no seed resolution for zero wanted"
        );
        assert_eq!(reco_calls.load(Ordering::SeqCst), 0);
    }

    struct SeedCapturingFake {
        seen: Arc<Mutex<Option<Seed>>>,
    }

    #[async_trait::async_trait]
    impl Recommender for SeedCapturingFake {
        fn name(&self) -> &'static str {
            "capture"
        }
        async fn recommend(&self, s: &Seed, _w: usize) -> Result<Vec<Recommendation>> {
            *self.seen.lock().expect("test mutex") = Some(s.clone());
            Ok(vec![])
        }
    }

    /// Ruling 27 / spec §9 "MusicBrainz any failure": a resolver erroring
    /// falls back to whatever the parse already produced, rather than
    /// aborting `next_tracks` outright.
    #[tokio::test]
    async fn a_failing_resolver_falls_back_to_the_parsed_seed() {
        let seen: Arc<Mutex<Option<Seed>>> = Arc::new(Mutex::new(None));
        let r = MusicReco::builder()
            .resolver(Box::new(TitleParseResolver::new()))
            .resolver(Box::new(FakeResolver {
                name: "boom",
                calls: Arc::new(AtomicUsize::new(0)),
                result: resolver_boom,
            }))
            .recommender(Box::new(SeedCapturingFake {
                seen: Arc::clone(&seen),
            }))
            .policy(Policy {
                min_seed_confidence: 0,
            })
            .build();
        let _ = r.next_tracks(&raw(), 5).await.unwrap();
        let got = seen
            .lock()
            .expect("test mutex")
            .clone()
            .expect("the recommender must still be called with the parsed seed");
        assert_eq!(got.artist, "Queen");
        assert_eq!(got.title, "Bohemian Rhapsody");
        assert_eq!(
            got.confidence, 50,
            "the parse's own confidence, unmodified by the failed resolver"
        );
    }
}
