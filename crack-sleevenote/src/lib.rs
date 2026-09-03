//! A thin, typed async client for [sleevenote][repo], the HTTP service that
//! resolves Spotify track / album / playlist ids to metadata now that the
//! Spotify Web API is unobtainable.
//!
//! This crate is the client only. It models the v0.1.0 wire contract and
//! nothing else: no caching, no retries, no fallback to another source, no
//! opinion about what a caller should do with a failure.
//!
//! # Shape of the thing
//!
//! * [`Client`] -- one deployment, three entity calls plus `/health`.
//! * [`model`] -- the success shapes, mirroring the service's `src/types.ts`.
//! * [`error`] -- the failure taxonomy, one variant per documented code.
//!
//! ```no_run
//! # async fn demo() -> Result<(), crack_sleevenote::Error> {
//! use crack_sleevenote::{Client, Error};
//!
//! let client = Client::from_env()?;
//! match client.playlist("3tlExkExp1aaYcU91Qhp79").await {
//!     Ok(playlist) => {
//!         // `unresolved_items` is not decoration: without it a two-track
//!         // playlist and a four-item one half of which failed to resolve
//!         // look identical.
//!         println!(
//!             "{}: {} of {} items resolved",
//!             playlist.name,
//!             playlist.tracks.len(),
//!             playlist.total_items(),
//!         );
//!     },
//!     // The distinctions below are the reason the service exists. Three of
//!     // these arms are HTTP 502 and they mean different things.
//!     Err(Error::NotFound(d)) => println!("no such playlist: {}", d.id),
//!     Err(Error::ExtractionEmpty(d)) => println!("our scraper broke: {}", d.message),
//!     Err(Error::ExtractionIncomplete(d)) => println!("partial: {}", d.message),
//!     Err(Error::Timeout(d)) => println!("retry later: {}", d.message),
//!     Err(other) => return Err(other),
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Timeouts
//!
//! [`DEFAULT_TIMEOUT`] is three minutes and that is not a mistake -- see its
//! docs. A short HTTP timeout here turns normal cold-cache operation into
//! transport errors.
//!
//! [repo]: https://github.com/cycle-five/sleevenote

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod client;
pub mod error;
pub mod model;

pub use client::{
    CacheStatus, Client, ClientBuilder, Health, BASE_URL_ENV, CACHE_HEADER, DEFAULT_BASE_URL,
    DEFAULT_TIMEOUT, ID_PATTERN, TIMEOUT_SECS_ENV,
};
pub use error::{Error, ErrorBody, ErrorCode, ErrorDetail, Result};
pub use model::{
    Album, AlbumTag, Artist, Playlist, PlaylistTag, Track, TrackAlbum, TrackTag, TrackUrlKind,
};
