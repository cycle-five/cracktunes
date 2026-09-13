use crate::{RawTrack, Recommendation, Result, Seed};
use async_trait::async_trait;

pub mod deezer;
// `pub(crate)`, not private: Task 5's MusicBrainz resolver (outside this
// module's descendants) also needs `http::encode_query`.
pub(crate) mod http;
pub mod musicatlas;
pub mod youtube_mix;
pub use deezer::Deezer;
pub use musicatlas::MusicAtlas;
pub use youtube_mix::YouTubeMix;

/// Turn the track that just ended into candidate next-tracks. An empty
/// `Ok(vec![])` means "this provider had nothing", which the orchestrator
/// treats as "try the next one" — distinct from an `Err`, which may disable
/// the provider.
#[async_trait]
pub trait Recommender: Send + Sync {
    fn name(&self) -> &'static str;

    /// Whether calls cost a finite, purchased quota.
    ///
    /// 🔑 Only metered providers are gated by the seed-confidence floor. A free
    /// provider must still be tried on a shaky seed -- otherwise a MusicBrainz
    /// outage leaves every seed at the bare-parse score of 50 and autoplay is
    /// dead, which is exactly the single-point-of-failure this crate exists to
    /// remove.
    fn is_metered(&self) -> bool {
        false
    }

    /// Whether this provider works from a [`Seed`] (an artist and a title).
    ///
    /// YouTube's Mix does not: it works from the video id alone, so it answers
    /// even for a title nothing can be parsed out of. The orchestrator resolves
    /// a seed only when it first reaches a provider that needs one.
    fn needs_seed(&self) -> bool {
        true
    }

    /// `seed` is `None` only for a provider whose [`Self::needs_seed`] is
    /// `false`: the orchestrator never calls one that needs a seed without it.
    /// A provider that needs one and gets `None` anyway returns nothing rather
    /// than panicking.
    async fn recommend(
        &self,
        track: &RawTrack,
        seed: Option<&Seed>,
        want: usize,
    ) -> Result<Vec<Recommendation>>;
}
