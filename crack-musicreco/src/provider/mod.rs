use crate::{Recommendation, Result, Seed};
use async_trait::async_trait;

pub mod musicatlas;
pub mod reccobeats;
pub use musicatlas::MusicAtlas;
pub use reccobeats::ReccoBeats;

/// Turn a seed into candidate next-tracks. An empty `Ok(vec![])` means "this
/// provider had nothing", which the orchestrator treats as "try the next one" —
/// distinct from an `Err`, which may disable the provider.
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

    async fn recommend(&self, seed: &Seed, want: usize) -> Result<Vec<Recommendation>>;
}
