//! Autoplay's half of crack-musicreco: which recommendation plays next.
//!
//! Kept out of `track_end.rs` so each piece is testable without Discord or
//! songbird: the per-guild buffer, the seed read off the track that ended, the
//! query a recommendation becomes, and the one [`MusicReco`] per process.

use crate::db::{CachedRecommender, MUSICATLAS_DAILY_BUDGET};
use ::serenity::model::id::GuildId;
use crack_musicreco::{
    video_id_from_url, Deezer, MusicAtlas, MusicBrainz, MusicReco, Playable, RawTrack,
    Recommendation, Recommender, TitleParseResolver, YouTubeMix,
};
use crack_types::QueryType;
use songbird::input::AuxMetadata;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Recommendations one refill asks for. musicatlas answers 20 per call, so
/// asking for more buys nothing, and asking for fewer spends more of its daily
/// budget on the same number of tracks.
pub const REFILL_SIZE: usize = 20;

/// The environment variable holding the musicatlas API key. Unset, autoplay
/// recommends through YouTube's Mix and Deezer alone.
pub const MUSICATLAS_KEY_ENV: &str = "MUSICATLAS_API_KEY";

/// MusicBrainz requires a contact address in the User-Agent and blocks clients
/// that send none.
const MUSICBRAINZ_CONTACT: &str = "cycle.five@proton.me";

/// yt-dlp's placeholder for a field it does not have. Measured on ordinary
/// music videos: `artist: NA` and `track: NA`.
const YT_DLP_MISSING: &str = "NA";

#[derive(Debug, Default)]
struct Guilds {
    queues: HashMap<GuildId, VecDeque<Recommendation>>,
    /// Bumped by every [`AutoplayBuffer::clear`], so a refill that was already
    /// in flight when the buffer was cleared cannot put its stale rest back.
    generations: HashMap<GuildId, u64>,
}

/// Pending autoplay recommendations, per guild.
///
/// 🔑 One provider call yields up to 20 tracks. Draining them one per track end
/// is what keeps musicatlas inside its daily budget; asking on every track end
/// would be about twenty times the calls.
#[derive(Debug, Default)]
pub struct AutoplayBuffer {
    guilds: Mutex<Guilds>,
}

impl AutoplayBuffer {
    fn lock(&self) -> MutexGuard<'_, Guilds> {
        // Every write here is a single insert or remove, so a poisoned lock
        // cannot hold a half-written map.
        self.guilds.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The guild's next recommendation, calling `refill` only when its buffer
    /// is empty. `None` when it is empty and the refill brought nothing.
    ///
    /// 🔑 Front first. Providers answer most similar first, and the plan's
    /// `Vec::pop` would have served every refill backwards (Ruling 43).
    ///
    /// The lock is not held across `refill`, a network call of up to several
    /// seconds. A [`Self::clear`] that lands meanwhile still wins: the refill's
    /// first result is returned, but the rest are not kept.
    pub async fn next<F, Fut>(&self, guild: GuildId, refill: F) -> Option<Recommendation>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Vec<Recommendation>>,
    {
        let generation = {
            let mut guilds = self.lock();
            if let Some(next) = guilds.queues.get_mut(&guild).and_then(VecDeque::pop_front) {
                return Some(next);
            }
            guilds.generations.get(&guild).copied().unwrap_or_default()
        };

        let mut fresh = VecDeque::from(refill().await);
        let first = fresh.pop_front()?;
        let mut guilds = self.lock();
        let current = guilds.generations.get(&guild).copied().unwrap_or_default();
        if !fresh.is_empty() && current == generation {
            guilds.queues.insert(guild, fresh);
        }
        Some(first)
    }

    /// Drop the guild's buffered recommendations (Ruling 49). Someone queued a
    /// track themselves, or autoplay was turned off, so a list chosen from what
    /// played before no longer applies.
    pub fn clear(&self, guild: GuildId) {
        let mut guilds = self.lock();
        guilds.queues.remove(&guild);
        let generation = guilds.generations.entry(guild).or_default();
        *generation = generation.wrapping_add(1);
    }
}

/// The query a recommendation plays through: a YouTube Mix or musicatlas result
/// is a video to play directly; a Deezer result is a search.
#[must_use]
pub fn to_query(rec: &Recommendation) -> QueryType {
    match &rec.playable {
        Playable::YouTubeId(id) => {
            QueryType::VideoLink(format!("https://www.youtube.com/watch?v={id}"))
        },
        Playable::SearchQuery(query) => QueryType::Keywords(query.clone()),
    }
}

fn present(field: Option<&String>) -> Option<String> {
    field
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != YT_DLP_MISSING)
        .map(str::to_owned)
}

/// What the ended track gives us to seed from. `None` when it has no title.
///
/// 🪤 yt-dlp reports a missing artist as the string `NA`, not as nothing.
/// Passed through, `NA` would read as a caller-supplied artist, which scores
/// 100 and clears the floor for metered providers -- spending musicatlas'
/// budget on "NA - <title>".
///
/// The video id comes from `source_url`. YouTube's Mix needs nothing else, so
/// it answers even for a title with no artist in it.
#[must_use]
pub fn raw_track(meta: &AuxMetadata) -> Option<RawTrack> {
    Some(RawTrack {
        title: present(meta.title.as_ref())?,
        artist: present(meta.artist.as_ref()),
        uploader: present(meta.channel.as_ref()),
        video_id: meta.source_url.as_deref().and_then(video_id_from_url),
    })
}

/// Build the process's one [`MusicReco`] (Ruling 42), or `None` when no
/// recommender could be built.
///
/// - Seeds come from the title parse, then MusicBrainz.
/// - YouTube's Mix first: free, needs no seed, and on-genre where ReccoBeats'
///   audio-feature matching was not (measured 2026-09-13). Uncached: one
///   yt-dlp run per refill, and a Mix changes.
/// - Deezer's artist radio next, behind the cache when there is a database.
/// - musicatlas last, and only with BOTH a key and a database. Its daily
///   budget is counted in the database, and a quota that cannot be counted
///   must not be spent (Ruling 40). Last, so the budget is spent only when
///   both free providers had nothing.
///
/// Every provider constructor can fail (Ruling 41). One that does is logged
/// and left out; startup carries on.
#[must_use]
pub fn build_musicreco(
    pool: Option<sqlx::PgPool>,
    musicatlas_key: Option<String>,
) -> Option<MusicReco> {
    let mut builder = MusicReco::builder().resolver(Box::new(TitleParseResolver::new()));
    match MusicBrainz::new(MUSICBRAINZ_CONTACT) {
        Ok(musicbrainz) => builder = builder.resolver(Box::new(musicbrainz)),
        Err(e) => tracing::warn!("autoplay: MusicBrainz unavailable, seeds stay unconfirmed: {e}"),
    }

    let cached = |inner: Box<dyn Recommender>, budget: Option<u32>| -> Box<dyn Recommender> {
        match &pool {
            Some(pool) => Box::new(CachedRecommender::new(inner, pool.clone(), budget)),
            None => inner,
        }
    };

    let mut recommenders = 0;
    builder = builder.recommender(Box::new(YouTubeMix::new()));
    recommenders += 1;
    match Deezer::new() {
        Ok(deezer) => {
            builder = builder.recommender(cached(Box::new(deezer), None));
            recommenders += 1;
        },
        Err(e) => tracing::warn!("autoplay: Deezer left out: {e}"),
    }
    match (musicatlas_key.filter(|k| !k.trim().is_empty()), &pool) {
        (Some(key), Some(_)) => match MusicAtlas::new(key) {
            Ok(musicatlas) => {
                builder = builder
                    .recommender(cached(Box::new(musicatlas), Some(MUSICATLAS_DAILY_BUDGET)));
                recommenders += 1;
            },
            Err(e) => tracing::warn!("autoplay: musicatlas left out: {e}"),
        },
        (Some(_), None) => tracing::info!(
            "autoplay: musicatlas left out -- no database to count its daily budget in"
        ),
        (None, _) => {
            tracing::info!("autoplay: {MUSICATLAS_KEY_ENV} is unset, so musicatlas is left out")
        },
    }

    if recommenders == 0 {
        tracing::warn!(
            "autoplay: no recommender could be built; autoplay will announce itself off"
        );
        return None;
    }
    let reco = builder.build();
    tracing::info!(
        "autoplay: recommending through {}",
        reco.recommender_names().join(", ")
    );
    Some(reco)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn rec(title: &str) -> Recommendation {
        Recommendation {
            artist: "A".into(),
            title: title.into(),
            playable: Playable::SearchQuery(format!("A - {title}")),
            isrc: None,
            source: "test".into(),
        }
    }

    fn titles(n: usize) -> Vec<Recommendation> {
        (0..n).map(|i| rec(&format!("t{i}"))).collect()
    }

    #[tokio::test]
    async fn a_refill_is_served_most_similar_first() {
        let buffer = AutoplayBuffer::default();
        let guild = GuildId::new(1);
        let mut served = Vec::new();
        for _ in 0..3 {
            let next = buffer
                .next(guild, || async {
                    vec![rec("best"), rec("second"), rec("third")]
                })
                .await;
            served.push(next.expect("buffered").title);
        }
        assert_eq!(served, ["best", "second", "third"]);
    }

    #[tokio::test]
    async fn one_refill_serves_twenty_track_ends() {
        let buffer = AutoplayBuffer::default();
        let guild = GuildId::new(1);
        let refills = AtomicUsize::new(0);
        for _ in 0..REFILL_SIZE {
            let next = buffer
                .next(guild, || async {
                    refills.fetch_add(1, Ordering::SeqCst);
                    titles(REFILL_SIZE)
                })
                .await;
            assert!(next.is_some());
        }
        assert_eq!(refills.load(Ordering::SeqCst), 1);

        buffer
            .next(guild, || async {
                refills.fetch_add(1, Ordering::SeqCst);
                titles(REFILL_SIZE)
            })
            .await;
        assert_eq!(
            refills.load(Ordering::SeqCst),
            2,
            "the 21st track end refills"
        );
    }

    #[tokio::test]
    async fn a_guild_never_plays_another_guilds_recommendations() {
        let buffer = AutoplayBuffer::default();
        let (a, b) = (GuildId::new(1), GuildId::new(2));
        buffer
            .next(a, || async { vec![rec("a1"), rec("a2")] })
            .await;

        let refills = AtomicUsize::new(0);
        let next = buffer
            .next(b, || async {
                refills.fetch_add(1, Ordering::SeqCst);
                vec![rec("b1")]
            })
            .await;
        assert_eq!(next.map(|r| r.title), Some("b1".to_owned()));
        assert_eq!(refills.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_empty_refill_yields_nothing_and_is_asked_again_next_time() {
        let buffer = AutoplayBuffer::default();
        let guild = GuildId::new(1);
        let refills = AtomicUsize::new(0);
        for _ in 0..2 {
            let next = buffer
                .next(guild, || async {
                    refills.fetch_add(1, Ordering::SeqCst);
                    Vec::new()
                })
                .await;
            assert!(next.is_none());
        }
        assert_eq!(refills.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn clearing_drops_what_is_buffered() {
        let buffer = AutoplayBuffer::default();
        let guild = GuildId::new(1);
        buffer
            .next(guild, || async { vec![rec("one"), rec("two")] })
            .await;
        buffer.clear(guild);
        let next = buffer.next(guild, || async { vec![rec("fresh")] }).await;
        assert_eq!(next.map(|r| r.title), Some("fresh".to_owned()));
    }

    /// A user queues something while a refill is on the network: the refill's
    /// first pick still plays, but its other 19 were chosen from the old track.
    #[tokio::test]
    async fn a_clear_during_a_refill_keeps_none_of_its_rest() {
        let buffer = AutoplayBuffer::default();
        let guild = GuildId::new(1);
        let first = buffer
            .next(guild, || async {
                buffer.clear(guild);
                vec![rec("one"), rec("two")]
            })
            .await;
        assert_eq!(first.map(|r| r.title), Some("one".to_owned()));

        let next = buffer.next(guild, || async { vec![rec("fresh")] }).await;
        assert_eq!(next.map(|r| r.title), Some("fresh".to_owned()));
    }

    #[test]
    fn a_youtube_id_plays_directly_and_a_search_query_is_searched() {
        let mut youtube = rec("x");
        youtube.playable = Playable::YouTubeId("dQw4w9WgXcQ".into());
        assert!(matches!(
            to_query(&youtube),
            QueryType::VideoLink(ref url) if url == "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(matches!(
            to_query(&rec("Somebody to Love")),
            QueryType::Keywords(ref q) if q == "A - Somebody to Love"
        ));
    }

    fn meta(title: Option<&str>, artist: Option<&str>, channel: Option<&str>) -> AuxMetadata {
        AuxMetadata {
            title: title.map(str::to_owned),
            artist: artist.map(str::to_owned),
            channel: channel.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn yt_dlp_na_is_not_an_artist() {
        let raw = raw_track(&meta(
            Some("Queen – Bohemian Rhapsody"),
            Some("NA"),
            Some("NA"),
        ))
        .expect("has a title");
        assert_eq!(raw.title, "Queen – Bohemian Rhapsody");
        assert_eq!(
            raw.artist, None,
            "NA would score 100 and spend metered calls"
        );
        assert_eq!(raw.uploader, None);
    }

    #[test]
    fn a_real_artist_and_uploader_are_kept_trimmed() {
        let raw = raw_track(&meta(
            Some(" Bohemian Rhapsody "),
            Some(" Queen "),
            Some("Queen Official"),
        ))
        .expect("has a title");
        assert_eq!(raw.title, "Bohemian Rhapsody");
        assert_eq!(raw.artist.as_deref(), Some("Queen"));
        assert_eq!(raw.uploader.as_deref(), Some("Queen Official"));
    }

    #[test]
    fn no_title_no_seed() {
        assert_eq!(raw_track(&meta(None, Some("Queen"), None)), None);
        assert_eq!(raw_track(&meta(Some("   "), Some("Queen"), None)), None);
    }

    /// The Mix works from this alone. Measured on production: the fan upload
    /// that `/play <url>` resolved with no artist.
    #[test]
    fn the_video_id_comes_from_the_source_url() {
        let mut m = meta(Some("The Offspring ~ Hit That"), None, Some("MrCalienteLP"));
        m.source_url = Some("https://www.youtube.com/watch?v=NJKhbnSGLsQ".into());
        assert_eq!(
            raw_track(&m).expect("has a title").video_id.as_deref(),
            Some("NJKhbnSGLsQ")
        );
        m.source_url = Some("https://open.spotify.com/track/3lfmqF0ULXRHlWxBeaHo3t".into());
        assert_eq!(raw_track(&m).expect("has a title").video_id, None);
        m.source_url = None;
        assert_eq!(raw_track(&m).expect("has a title").video_id, None);
    }

    fn unreachable_pool() -> sqlx::PgPool {
        // Never connected: building providers makes no database call.
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("a lazy pool only parses the url")
    }

    /// Free first, metered last: musicatlas' budget is spent only when both
    /// free providers had nothing.
    #[tokio::test]
    async fn the_mix_goes_first_and_musicatlas_last_given_a_key_and_a_database() {
        let reco = build_musicreco(Some(unreachable_pool()), Some("key".into())).expect("built");
        assert_eq!(
            reco.recommender_names(),
            ["youtube-mix", "deezer", "musicatlas"]
        );
    }

    /// Ruling 40: a quota that cannot be counted must not be spent.
    #[tokio::test]
    async fn musicatlas_is_left_out_without_a_database() {
        let reco = build_musicreco(None, Some("key".into())).expect("built");
        assert_eq!(reco.recommender_names(), ["youtube-mix", "deezer"]);
    }

    #[tokio::test]
    async fn no_key_or_a_blank_one_leaves_musicatlas_out() {
        for key in [None, Some(String::new()), Some("  ".into())] {
            let reco = build_musicreco(Some(unreachable_pool()), key).expect("built");
            assert_eq!(reco.recommender_names(), ["youtube-mix", "deezer"]);
        }
    }
}
