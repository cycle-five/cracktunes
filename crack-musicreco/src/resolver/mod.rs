use crate::{RawTrack, Result, Seed};
use async_trait::async_trait;

pub mod musicbrainz;
pub mod title_parse;
pub use musicbrainz::MusicBrainz;
pub use title_parse::TitleParseResolver;

/// Turn what the bot knows about a finished track into a seed worth spending a
/// metered call on. `Ok(None)` means "no usable seed" and is NOT an error —
/// it is the correct, free outcome for a title with no artist in it.
#[async_trait]
pub trait SeedResolver: Send + Sync {
    fn name(&self) -> &'static str;
    async fn resolve(&self, raw: &RawTrack) -> Result<Option<Seed>>;
}
