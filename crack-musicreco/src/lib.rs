//! Provider-agnostic music recommendation.
//!
//! Autoplay needs "given the track that just ended, what plays next". No single
//! service answers that reliably, so this crate holds several behind two traits
//! and falls back between them.
//!
//! * [`SeedResolver`] — a messy YouTube title becomes a checked `(artist, title)`.
//! * [`Recommender`] — a seed becomes candidate tracks.
//!
//! 🔑 The providers are not interchangeable. Only musicatlas returns a directly
//! playable YouTube id, so [`model::Playable`] makes that difference explicit
//! instead of hiding it behind a lowest common denominator.

pub mod error;
pub mod model;
pub mod provider;
pub mod resolver;
#[cfg(test)]
pub(crate) mod test_support;

pub use error::{Error, Result};
pub use model::{Playable, RawTrack, Recommendation, Seed};
pub use provider::{MusicAtlas, ReccoBeats, Recommender};
pub use resolver::{SeedResolver, TitleParseResolver};
