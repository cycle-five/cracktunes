//! The recommendation cache and daily call budget for `crack-musicreco`.
//!
//! Lives here rather than in `crack-musicreco`, so the client crate stays free
//! of a database dependency -- the separation `crack-sleevenote` keeps.
//!
//! 🔑 Nothing in the client crate calls this. [`CachedRecommender`] is what
//! puts a provider behind the cache and the budget; without it musicatlas
//! would run with no quota accounting at all (Ruling 39).

use async_trait::async_trait;
use crack_musicreco::{Error as RecoError, Recommendation, Recommender, Seed};
use sqlx::PgPool;

/// musicatlas' free tier is 100 calls a day, and no response header reports
/// the remainder (spec §4). 90 leaves headroom for calls this process did not
/// make: a second deployment, a manual test.
pub const MUSICATLAS_DAILY_BUDGET: u32 = 90;

/// How long a positive entry is served before the provider is asked again.
/// Our policy, not a provider limit.
const FOUND_TTL_DAYS: i32 = 30;

/// A negative entry expires sooner: a track a provider does not know today can
/// be added tomorrow.
const NOT_FOUND_TTL_DAYS: i32 = 7;

/// Cache keys only; the provider always receives the un-normalized values.
///
/// Trim, collapse internal whitespace, lowercase, and fold `’` to `'`, the
/// same folding the MusicBrainz resolver applies -- otherwise "Don’t Stop" and
/// "Don't Stop" are two entries and two metered calls.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .replace('\u{2019}', "'")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub results: Vec<Recommendation>,
    pub found: bool,
}

pub struct MusicRecoCache;

impl MusicRecoCache {
    /// The fresh entry for this provider and seed, or `None` for a miss.
    ///
    /// An entry older than its TTL is a miss, and the next [`Self::put`]
    /// replaces it. So is an entry whose stored results no longer decode -- a
    /// shape written by an older build -- which is logged, not reported as
    /// "found, nothing to play".
    ///
    /// # Errors
    /// Any sqlx failure.
    pub async fn get(
        pool: &PgPool,
        provider: &str,
        seed: &Seed,
    ) -> Result<Option<CacheEntry>, sqlx::Error> {
        let row = sqlx::query!(
            r#"SELECT results::text AS "results!", found
               FROM musicreco_cache
               WHERE provider = $1 AND artist = $2 AND track = $3
                 AND fetched_at > now() - make_interval(days => CASE WHEN found THEN $4::int4 ELSE $5::int4 END)"#,
            provider,
            normalize(&seed.artist),
            normalize(&seed.title),
            FOUND_TTL_DAYS,
            NOT_FOUND_TTL_DAYS,
        )
        .fetch_optional(pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };
        match serde_json::from_str::<Vec<Recommendation>>(&row.results) {
            Ok(results) => Ok(Some(CacheEntry {
                results,
                found: row.found,
            })),
            Err(e) => {
                tracing::warn!(
                    "musicreco cache: the {provider} entry for `{} - {}` no longer decodes, \
                     treating it as a miss: {e}",
                    seed.artist,
                    seed.title
                );
                Ok(None)
            },
        }
    }

    /// Store (or replace) the entry for this provider and seed, stamped now.
    ///
    /// # Errors
    /// Any sqlx failure. Results that fail to serialize are
    /// `sqlx::Error::Encode`, never a silently stored `[]` -- that would
    /// poison the entry until its TTL ran out.
    pub async fn put(
        pool: &PgPool,
        provider: &str,
        seed: &Seed,
        results: &[Recommendation],
        found: bool,
    ) -> Result<(), sqlx::Error> {
        let blob = serde_json::to_string(results).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        sqlx::query!(
            "INSERT INTO musicreco_cache (provider, artist, track, results, found)
             VALUES ($1, $2, $3, $4::text::jsonb, $5)
             ON CONFLICT (provider, artist, track)
             DO UPDATE SET results = EXCLUDED.results, found = EXCLUDED.found, fetched_at = now()",
            provider,
            normalize(&seed.artist),
            normalize(&seed.title),
            blob,
            found,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Reserve one call against today's budget. `Ok(false)` means refuse.
    ///
    /// 🔑 Increments BEFORE the call, in one statement, so a crash between the
    /// call and the bookkeeping cannot under-count a quota no header reports.
    ///
    /// 🪤 The statement's `WHERE calls < $2` guards only the conflict branch:
    /// the day's first INSERT is unconditional. A budget of 0 is refused here,
    /// before the statement, or it would still grant one call.
    ///
    /// The day is the UTC date, not `CURRENT_DATE`, which follows the database
    /// server's time zone. When the provider's own day resets is unmeasured.
    ///
    /// # Errors
    /// Any sqlx failure.
    pub async fn try_spend(
        pool: &PgPool,
        provider: &str,
        budget: u32,
    ) -> Result<bool, sqlx::Error> {
        if budget == 0 {
            return Ok(false);
        }
        let budget = i32::try_from(budget).unwrap_or(i32::MAX);
        let granted = sqlx::query_scalar!(
            "INSERT INTO musicreco_budget (provider, day, calls)
             VALUES ($1, (now() AT TIME ZONE 'UTC')::date, 1)
             ON CONFLICT (provider, day) DO UPDATE
               SET calls = musicreco_budget.calls + 1
               WHERE musicreco_budget.calls < $2
             RETURNING calls",
            provider,
            budget,
        )
        .fetch_optional(pool)
        .await?;
        Ok(granted.is_some())
    }
}

/// A recommender behind the cache and, when `budget` is set, the daily budget
/// (Ruling 39). The inner provider is called only on a miss, and only after the
/// budget has granted the call.
///
/// A database failure is handled by what it would cost:
/// - the cache cannot be read or written: carry on uncached. That costs calls,
///   not autoplay.
/// - the budget cannot be counted: refuse the call. A quota that cannot be
///   counted must not be spent.
pub struct CachedRecommender {
    inner: Box<dyn Recommender>,
    pool: PgPool,
    budget: Option<u32>,
}

impl CachedRecommender {
    #[must_use]
    pub fn new(inner: Box<dyn Recommender>, pool: PgPool, budget: Option<u32>) -> Self {
        Self {
            inner,
            pool,
            budget,
        }
    }

    async fn remember(&self, seed: &Seed, results: &[Recommendation], found: bool) {
        let name = self.inner.name();
        if let Err(e) = MusicRecoCache::put(&self.pool, name, seed, results, found).await {
            tracing::warn!("musicreco cache: could not store the {name} result: {e}");
        }
    }
}

#[async_trait]
impl Recommender for CachedRecommender {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn is_metered(&self) -> bool {
        self.inner.is_metered()
    }

    /// 🪤 A hit is served truncated to `want`, but a miss caches only what the
    /// call returned for THIS `want`. A later, larger `want` for the same seed
    /// gets the shorter list until the entry expires. The orchestrator always
    /// asks for the same amount, so this does not arise today.
    async fn recommend(
        &self,
        seed: &Seed,
        want: usize,
    ) -> crack_musicreco::Result<Vec<Recommendation>> {
        if want == 0 {
            // Before the budget, which would otherwise spend a call on nothing.
            return Ok(Vec::new());
        }
        let name = self.inner.name();

        match MusicRecoCache::get(&self.pool, name, seed).await {
            // A negative entry's results are `[]`, so this also answers "we
            // already know there is nothing" without a call.
            Ok(Some(entry)) => return Ok(entry.results.into_iter().take(want).collect()),
            Ok(None) => {},
            Err(e) => {
                tracing::warn!("musicreco cache: could not read for {name}, calling uncached: {e}");
            },
        }

        if let Some(budget) = self.budget {
            match MusicRecoCache::try_spend(&self.pool, name, budget).await {
                Ok(true) => {},
                Ok(false) => {
                    return Err(RecoError::BudgetExhausted {
                        provider: name,
                        budget,
                    })
                },
                Err(e) => {
                    tracing::warn!(
                        "musicreco budget: could not count a {name} call, refusing it: {e}"
                    );
                    return Err(RecoError::BudgetExhausted {
                        provider: name,
                        budget,
                    });
                },
            }
        }

        match self.inner.recommend(seed, want).await {
            Ok(results) => {
                self.remember(seed, &results, !results.is_empty()).await;
                Ok(results)
            },
            Err(e @ RecoError::NotATrack { .. }) => {
                // Permanent for this seed: the next track end of it must not
                // spend another call to hear the same answer.
                self.remember(seed, &[], false).await;
                Err(e)
            },
            // Transient or operator-facing (rate limit, transport, bad key):
            // caching it would hide a recovery.
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_case_padding_whitespace_and_curly_apostrophes() {
        assert_eq!(normalize("  QUEEN "), "queen");
        assert_eq!(normalize("bohemian   rhapsody"), "bohemian rhapsody");
        assert_eq!(normalize("Don\u{2019}t Stop"), normalize("don't stop"));
    }

    #[cfg(test)]
    mod db {
        use super::super::*;
        use crack_musicreco::Playable;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

        fn seed(artist: &str, title: &str) -> Seed {
            Seed {
                artist: artist.into(),
                title: title.into(),
                mbid: None,
                confidence: 100,
            }
        }

        fn rec(title: &str) -> Recommendation {
            Recommendation {
                artist: "A".into(),
                title: title.into(),
                playable: Playable::YouTubeId(format!("yt-{title}")),
                isrc: None,
                source: "fake".into(),
            }
        }

        /// Backdate an entry, so TTL tests need no clock.
        async fn age(pool: &PgPool, days: i32) {
            sqlx::query(
                "UPDATE musicreco_cache SET fetched_at = now() - make_interval(days => $1)",
            )
            .bind(days)
            .execute(pool)
            .await
            .expect("backdate");
        }

        async fn budget_rows(pool: &PgPool) -> i64 {
            sqlx::query_scalar("SELECT count(*) FROM musicreco_budget")
                .fetch_one(pool)
                .await
                .expect("count")
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_negative_result_is_remembered(pool: PgPool) {
            let s = seed("Lofi Radio", "Beats");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[], false)
                .await
                .unwrap();
            let entry = MusicRecoCache::get(&pool, "musicatlas", &s)
                .await
                .unwrap()
                .expect("a negative entry is still an entry");
            assert!(
                !entry.found,
                "the caller must be able to skip the call entirely"
            );
            assert!(entry.results.is_empty());
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_positive_result_round_trips(pool: PgPool) {
            let s = seed("Queen", "Bohemian Rhapsody");
            let results = vec![rec("one"), rec("two")];
            MusicRecoCache::put(&pool, "musicatlas", &s, &results, true)
                .await
                .unwrap();
            let entry = MusicRecoCache::get(&pool, "musicatlas", &s).await.unwrap();
            assert_eq!(
                entry,
                Some(CacheEntry {
                    results,
                    found: true
                })
            );
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn keys_are_normalized_so_case_and_padding_do_not_double_spend(pool: PgPool) {
            let a = seed("Queen", "Don't Stop Me Now");
            let b = seed("  QUEEN ", "don\u{2019}t   stop me now");
            MusicRecoCache::put(&pool, "musicatlas", &a, &[], true)
                .await
                .unwrap();
            assert!(MusicRecoCache::get(&pool, "musicatlas", &b)
                .await
                .unwrap()
                .is_some());
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn entries_are_per_provider(pool: PgPool) {
            let s = seed("Queen", "Bohemian Rhapsody");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[], false)
                .await
                .unwrap();
            assert!(MusicRecoCache::get(&pool, "reccobeats", &s)
                .await
                .unwrap()
                .is_none());
        }

        /// Ruling 37: `fetched_at` is read, not only written.
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_positive_entry_expires_after_30_days(pool: PgPool) {
            let s = seed("Queen", "Bohemian Rhapsody");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[rec("one")], true)
                .await
                .unwrap();
            age(&pool, 29).await;
            assert!(MusicRecoCache::get(&pool, "musicatlas", &s)
                .await
                .unwrap()
                .is_some());
            age(&pool, 31).await;
            assert!(MusicRecoCache::get(&pool, "musicatlas", &s)
                .await
                .unwrap()
                .is_none());
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_negative_entry_expires_after_7_days(pool: PgPool) {
            let s = seed("Lofi Radio", "Beats");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[], false)
                .await
                .unwrap();
            age(&pool, 6).await;
            assert!(MusicRecoCache::get(&pool, "musicatlas", &s)
                .await
                .unwrap()
                .is_some());
            age(&pool, 8).await;
            assert!(MusicRecoCache::get(&pool, "musicatlas", &s)
                .await
                .unwrap()
                .is_none());
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_put_refreshes_a_stale_entry(pool: PgPool) {
            let s = seed("Queen", "Bohemian Rhapsody");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[], false)
                .await
                .unwrap();
            age(&pool, 8).await;
            MusicRecoCache::put(&pool, "musicatlas", &s, &[rec("one")], true)
                .await
                .unwrap();
            let entry = MusicRecoCache::get(&pool, "musicatlas", &s).await.unwrap();
            assert_eq!(entry.map(|e| e.found), Some(true));
        }

        /// Ruling 35: an undecodable blob is a MISS, not "found, nothing to play".
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_entry_that_no_longer_decodes_is_a_miss(pool: PgPool) {
            let s = seed("Queen", "Bohemian Rhapsody");
            MusicRecoCache::put(&pool, "musicatlas", &s, &[rec("one")], true)
                .await
                .unwrap();
            sqlx::query(
                r#"UPDATE musicreco_cache SET results = '[{"shape":"from an older build"}]'"#,
            )
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(
                MusicRecoCache::get(&pool, "musicatlas", &s).await.unwrap(),
                None
            );
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn the_budget_stops_at_the_limit(pool: PgPool) {
            for i in 0..3 {
                assert!(
                    MusicRecoCache::try_spend(&pool, "musicatlas", 3)
                        .await
                        .unwrap(),
                    "call {i}"
                );
            }
            assert!(
                !MusicRecoCache::try_spend(&pool, "musicatlas", 3)
                    .await
                    .unwrap(),
                "the fourth call must be refused, not merely logged"
            );
        }

        /// 🪤 Ruling 36: the day's first INSERT bypasses `WHERE calls < $2`.
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_budget_of_zero_grants_nothing(pool: PgPool) {
            assert!(!MusicRecoCache::try_spend(&pool, "musicatlas", 0)
                .await
                .unwrap());
            assert_eq!(budget_rows(&pool).await, 0);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn concurrent_spends_against_a_budget_of_one_grant_exactly_one(pool: PgPool) {
            let spends = (0..8).map(|_| {
                let pool = pool.clone();
                tokio::spawn(async move { MusicRecoCache::try_spend(&pool, "musicatlas", 1).await })
            });
            let mut granted = 0;
            for s in spends {
                if s.await.expect("task").expect("spend") {
                    granted += 1;
                }
            }
            assert_eq!(granted, 1);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn budgets_are_counted_per_provider(pool: PgPool) {
            assert!(MusicRecoCache::try_spend(&pool, "musicatlas", 1)
                .await
                .unwrap());
            assert!(MusicRecoCache::try_spend(&pool, "other", 1).await.unwrap());
        }

        struct Fake {
            calls: Arc<AtomicUsize>,
            result: fn() -> crack_musicreco::Result<Vec<Recommendation>>,
        }

        #[async_trait]
        impl Recommender for Fake {
            fn name(&self) -> &'static str {
                "fake"
            }
            fn is_metered(&self) -> bool {
                true
            }
            async fn recommend(
                &self,
                _seed: &Seed,
                _want: usize,
            ) -> crack_musicreco::Result<Vec<Recommendation>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                (self.result)()
            }
        }

        fn three() -> crack_musicreco::Result<Vec<Recommendation>> {
            Ok(vec![rec("one"), rec("two"), rec("three")])
        }
        fn nothing() -> crack_musicreco::Result<Vec<Recommendation>> {
            Ok(vec![])
        }
        fn not_a_track() -> crack_musicreco::Result<Vec<Recommendation>> {
            Err(RecoError::NotATrack {
                provider: "fake",
                artist: "Lofi Radio".into(),
                title: "Beats".into(),
                message: "That doesn't appear to be a released track.".into(),
            })
        }
        fn rate_limited() -> crack_musicreco::Result<Vec<Recommendation>> {
            Err(RecoError::RateLimited {
                provider: "fake",
                retry_after: None,
            })
        }

        fn cached(
            pool: &PgPool,
            budget: Option<u32>,
            result: fn() -> crack_musicreco::Result<Vec<Recommendation>>,
        ) -> (CachedRecommender, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            let inner = Fake {
                calls: Arc::clone(&calls),
                result,
            };
            (
                CachedRecommender::new(Box::new(inner), pool.clone(), budget),
                calls,
            )
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_repeat_seed_is_served_from_the_cache_without_a_call(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), three);
            let s = seed("Queen", "Bohemian Rhapsody");
            let first = r.recommend(&s, 20).await.unwrap();
            let second = r.recommend(&s, 20).await.unwrap();
            assert_eq!(first, second);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(budget_rows(&pool).await, 1, "the hit spent no budget");
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_hit_is_truncated_to_want(pool: PgPool) {
            let (r, calls) = cached(&pool, None, three);
            let s = seed("Queen", "Bohemian Rhapsody");
            r.recommend(&s, 20).await.unwrap();
            assert_eq!(r.recommend(&s, 2).await.unwrap().len(), 2);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_empty_answer_is_negative_cached(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), nothing);
            let s = seed("Lofi Radio", "Beats");
            assert!(r.recommend(&s, 20).await.unwrap().is_empty());
            assert!(r.recommend(&s, 20).await.unwrap().is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn not_a_track_is_negative_cached(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), not_a_track);
            let s = seed("Lofi Radio", "Beats");
            let first = r.recommend(&s, 20).await;
            assert!(matches!(first, Err(RecoError::NotATrack { .. })));
            assert!(r.recommend(&s, 20).await.unwrap().is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_transient_error_is_not_cached(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), rate_limited);
            let s = seed("Queen", "Bohemian Rhapsody");
            let _ = r.recommend(&s, 20).await;
            let _ = r.recommend(&s, 20).await;
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_exhausted_budget_refuses_without_calling(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(1), three);
            r.recommend(&seed("Queen", "Bohemian Rhapsody"), 20)
                .await
                .unwrap();
            let refused = r.recommend(&seed("Queen", "Somebody to Love"), 20).await;
            assert!(
                matches!(refused, Err(RecoError::BudgetExhausted { budget: 1, .. })),
                "got {refused:?}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_stale_entry_calls_the_provider_again(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), three);
            let s = seed("Queen", "Bohemian Rhapsody");
            r.recommend(&s, 20).await.unwrap();
            age(&pool, 31).await;
            r.recommend(&s, 20).await.unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn an_unbudgeted_provider_never_touches_the_budget(pool: PgPool) {
            let (r, calls) = cached(&pool, None, three);
            r.recommend(&seed("Queen", "Bohemian Rhapsody"), 20)
                .await
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(budget_rows(&pool).await, 0);
        }

        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn want_zero_spends_neither_budget_nor_call(pool: PgPool) {
            let (r, calls) = cached(&pool, Some(90), three);
            assert!(r
                .recommend(&seed("Queen", "Bohemian Rhapsody"), 0)
                .await
                .unwrap()
                .is_empty());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(budget_rows(&pool).await, 0);
        }

        /// Fail closed: with the budget table gone, a metered call is refused.
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_budget_that_cannot_be_counted_is_not_spent(pool: PgPool) {
            sqlx::query("DROP TABLE musicreco_budget")
                .execute(&pool)
                .await
                .unwrap();
            let (r, calls) = cached(&pool, Some(90), three);
            let refused = r.recommend(&seed("Queen", "Bohemian Rhapsody"), 20).await;
            assert!(
                matches!(refused, Err(RecoError::BudgetExhausted { .. })),
                "got {refused:?}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }

        /// Fail open: with the cache table gone, the call still happens.
        #[sqlx::test(migrator = "MIGRATOR")]
        #[cfg_attr(
            not(feature = "db-tests"),
            ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
        )]
        async fn a_cache_that_cannot_be_read_still_calls(pool: PgPool) {
            sqlx::query("DROP TABLE musicreco_cache")
                .execute(&pool)
                .await
                .unwrap();
            let (r, calls) = cached(&pool, None, three);
            assert_eq!(
                r.recommend(&seed("Queen", "Bohemian Rhapsody"), 20)
                    .await
                    .unwrap()
                    .len(),
                3
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
}
