//! The success shapes of the sleevenote wire contract, v0.1.0.
//!
//! Every type here mirrors a shape defined by `src/types.ts` in
//! cycle-five/sleevenote and confirmed against the captured responses vendored
//! in `tests/fixtures/`. Two rules govern this module and neither is cosmetic:
//!
//! * **The wire is camelCase, Rust is snake_case.** Structs carry
//!   `#[serde(rename_all = "camelCase")]` so `durationMs` and `unresolvedItems`
//!   land on `duration_ms` and `unresolved_items`.
//! * **`| null` means present-and-nullable, never absent.** Such a field is
//!   `Option<T>` and never `#[serde(default)]`, plus [`required_nullable`] to
//!   defeat serde's implicit missing-field default. A response that omits the
//!   key entirely is a contract violation and must fail to deserialize rather
//!   than quietly turn into `None` -- "Spotify says this track has no album"
//!   and "sleevenote stopped sending us albums" are different facts.
//!
//! The `type` discriminator on each entity is modelled as a single-variant enum
//! rather than a `String`. That is what makes a drift in the discriminator a
//! deserialization failure instead of a value nobody checks.

use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

/// Deserialize a field that is nullable but must be present.
///
/// A bare `Option<T>` field is *absent-or-null* to serde: a missing key
/// silently becomes `None`, with no `#[serde(default)]` needed to make it so.
/// The sleevenote contract says these keys are always present, so an omitted
/// one is wire drift and has to fail. Naming any `deserialize_with` is what
/// suppresses serde's implicit missing-field fallback; this function is
/// otherwise exactly `Option::<T>::deserialize`, so an explicit `null` still
/// means `None`.
///
/// # Errors
///
/// Whatever the underlying `Option<T>` deserialization reports.
pub fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// The literal `"track"` discriminator that every [`Track`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackTag {
    /// The only value the service ever sends for a track.
    #[default]
    Track,
}

/// The literal `"album"` discriminator that every [`Album`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumTag {
    /// The only value the service ever sends for an album.
    #[default]
    Album,
}

/// The literal `"playlist"` discriminator that every [`Playlist`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistTag {
    /// The only value the service ever sends for a playlist.
    #[default]
    Playlist,
}

/// A credited artist.
///
/// For a podcast episode returned inside a [`Playlist`] this is the show, not a
/// musician -- see [`Track::url_kind`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    /// Display name.
    pub name: String,
    /// Spotify artist id, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub id: Option<String>,
}

/// The abbreviated album a [`Track`] belongs to.
///
/// This is *not* an [`Album`]: it has no track listing. It is `null` on tracks
/// returned as part of an album listing, where repeating the parent on all 60
/// entries would be noise.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAlbum {
    /// Album title.
    pub name: String,
    /// Spotify album id, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub id: Option<String>,
    /// Cover art URL, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub image: Option<String>,
}

/// What kind of Spotify page a [`Track`]'s `url` points at.
///
/// A playlist can contain podcast episodes. They arrive in the same `Track`
/// shape, but their `url` is `/episode/<id>` rather than `/track/<id>` and
/// their sole "artist" is the show name. Callers that build a Spotify link, or
/// that assume a track id can be looked up via `GET /v1/track/:id`, must branch
/// on this rather than assuming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackUrlKind {
    /// `https://open.spotify.com/track/<id>` -- an ordinary song.
    Track,
    /// `https://open.spotify.com/episode/<id>` -- a podcast episode.
    Episode,
    /// Anything else, including a URL that does not parse.
    Other,
}

/// One playable item: a song, or a podcast episode inside a playlist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// Spotify id.
    pub id: String,
    /// Always [`TrackTag::Track`]; present so a discriminator change fails loudly.
    #[serde(rename = "type")]
    pub tag: TrackTag,
    /// Track title.
    pub name: String,
    /// Credited artists. Never empty for a track the service returned.
    pub artists: Vec<Artist>,
    /// Parent album, or `null` -- notably on every track inside an [`Album`].
    #[serde(deserialize_with = "required_nullable")]
    pub album: Option<TrackAlbum>,
    /// Duration in milliseconds, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub duration_ms: Option<u64>,
    /// Canonical Spotify URL. See [`Track::url_kind`] before parsing it.
    pub url: String,
}

impl Track {
    /// Duration as a [`Duration`], or `None` when the service reported none.
    #[must_use]
    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(Duration::from_millis)
    }

    /// The first credited artist, if there is one.
    ///
    /// The contract says `artists` is never empty for a returned track, but
    /// this returns an `Option` rather than indexing: a client should not panic
    /// because a service regressed.
    #[must_use]
    pub fn primary_artist(&self) -> Option<&Artist> {
        self.artists.first()
    }

    /// Classify [`Track::url`] by its first path segment.
    #[must_use]
    pub fn url_kind(&self) -> TrackUrlKind {
        let Ok(parsed) = url::Url::parse(&self.url) else {
            return TrackUrlKind::Other;
        };
        match parsed.path_segments().and_then(|mut it| it.next()) {
            Some("track") => TrackUrlKind::Track,
            Some("episode") => TrackUrlKind::Episode,
            _ => TrackUrlKind::Other,
        }
    }

    /// Whether this item is a podcast episode rather than a song.
    #[must_use]
    pub fn is_episode(&self) -> bool {
        self.url_kind() == TrackUrlKind::Episode
    }
}

/// An album and its track listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    /// Spotify id.
    pub id: String,
    /// Always [`AlbumTag::Album`]; present so a discriminator change fails loudly.
    #[serde(rename = "type")]
    pub tag: AlbumTag,
    /// Album title.
    pub name: String,
    /// Credited artists.
    pub artists: Vec<Artist>,
    /// Cover art URL, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub image: Option<String>,
    /// Canonical Spotify URL.
    pub url: String,
    /// The tracks that were successfully resolved.
    pub tracks: Vec<Track>,
    /// How many listed items could **not** be turned into a [`Track`].
    ///
    /// See [`Album::total_items`]. Ignoring this is how a caller mistakes a
    /// half-resolved listing for a complete one.
    pub unresolved_items: u32,
}

impl Album {
    /// How many items the service saw: resolved tracks plus unresolved ones.
    #[must_use]
    pub fn total_items(&self) -> u64 {
        self.tracks.len() as u64 + u64::from(self.unresolved_items)
    }

    /// Whether every item the service saw made it into [`Album::tracks`].
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unresolved_items == 0
    }
}

/// A playlist and its resolved contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    /// Spotify id.
    pub id: String,
    /// Always [`PlaylistTag::Playlist`]; present so a discriminator change fails loudly.
    #[serde(rename = "type")]
    pub tag: PlaylistTag,
    /// Playlist title.
    pub name: String,
    /// Owner display name, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub owner: Option<String>,
    /// Cover art URL, or `null` when the page did not expose one.
    #[serde(deserialize_with = "required_nullable")]
    pub image: Option<String>,
    /// Canonical Spotify URL.
    pub url: String,
    /// The items that were successfully resolved. May include podcast episodes.
    pub tracks: Vec<Track>,
    /// How many listed items could **not** be turned into a [`Track`].
    ///
    /// Local files are the usual cause. See [`Playlist::total_items`]: without
    /// this field a two-track playlist and a four-item playlist half of which
    /// failed to resolve are indistinguishable.
    pub unresolved_items: u32,
}

impl Playlist {
    /// How many items the service saw: resolved tracks plus unresolved ones.
    #[must_use]
    pub fn total_items(&self) -> u64 {
        self.tracks.len() as u64 + u64::from(self.unresolved_items)
    }

    /// Whether every item the service saw made it into [`Playlist::tracks`].
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unresolved_items == 0
    }
}
