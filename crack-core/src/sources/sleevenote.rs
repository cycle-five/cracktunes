//! Spotify resolution, backed by the [sleevenote] service.
//!
//! Spotify stopped issuing Web API credentials in ~2025-12, so the rspotify
//! path in [`crate::sources::spotify`] cannot authenticate and has not been
//! able to for some time. sleevenote resolves a track / album / playlist id to
//! metadata without an API key, and this module is the one place playback
//! reaches it.
//!
//! What lives here is deliberately *resolution only*: a Spotify link in, a
//! list of YouTube search queries out. Spotify is never the audio source --
//! the tracks are found on YouTube by searching for "title artists", exactly
//! as the old rspotify path did.
//!
//! # What sleevenote cannot do
//!
//! It resolves ids to metadata. It has no recommendations endpoint, so
//! autoplay (`handlers::track_end`) cannot be moved off rspotify and stays
//! dead until Spotify issues credentials again. That is a capability gap, not
//! an oversight -- see [`AUTOPLAY_DISABLED_SPOTIFY`].
//!
//! [sleevenote]: https://github.com/cycle-five/sleevenote

use crate::errors::CrackedError;
use crate::http_utils;
use crate::messaging::messages::{
    SPOTIFY_INVALID_QUERY, SPOTIFY_LOOKUP_BROKEN, SPOTIFY_LOOKUP_FAILED, SPOTIFY_NOTHING_PLAYABLE,
    SPOTIFY_NOT_CONFIGURED, SPOTIFY_NOT_FOUND, SPOTIFY_TIMEOUT, SPOTIFY_UNREACHABLE,
};
use crack_sleevenote::{Client as Sleevenote, Error as SleevenoteError, Track, BASE_URL_ENV};
use crack_types::NewAuxMetadata;
use songbird::input::AuxMetadata;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

/// Which kind of Spotify entity a link points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaType {
    /// A single song.
    Track,
    /// An album: many songs.
    Album,
    /// A playlist: many songs, possibly including podcast episodes.
    Playlist,
}

impl MediaType {
    /// The word to use for this entity in a message to a user.
    #[must_use]
    pub fn noun(self) -> &'static str {
        match self {
            MediaType::Track => "track",
            MediaType::Album => "album",
            MediaType::Playlist => "playlist",
        }
    }

    /// Whether this entity is a collection of songs rather than one song.
    #[must_use]
    pub fn is_collection(self) -> bool {
        !matches!(self, MediaType::Track)
    }
}

impl FromStr for MediaType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "track" => Ok(Self::Track),
            "album" => Ok(Self::Album),
            "playlist" => Ok(Self::Playlist),
            _ => Err(()),
        }
    }
}

/// A Spotify link broken into the two things a lookup needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpotifyUrl {
    media_type: MediaType,
    media_id: String,
}

impl ParsedSpotifyUrl {
    /// Which kind of entity the URL pointed at.
    #[must_use]
    pub fn media_type(&self) -> MediaType {
        self.media_type
    }

    /// The bare Spotify id, with no `?si=` tracking suffix.
    #[must_use]
    pub fn media_id(&self) -> &str {
        &self.media_id
    }
}

/// Split a Spotify URL or `spotify:` URI into an entity kind and an id.
///
/// Path segments are walked looking for the entity word rather than being
/// indexed, because Spotify's own web player emits locale-prefixed links --
/// `/intl-de/track/<id>` -- and an entity is not always the first segment.
/// The previous regex took everything before the last slash as the entity
/// kind, so every localized link a European user pasted failed to parse.
///
/// Returns `None` for anything that is not a recognisable Spotify entity,
/// including `spotify.link` shortlinks, which carry no id until followed.
#[must_use]
pub fn parse_spotify_url(input: &str) -> Option<ParsedSpotifyUrl> {
    // `spotify:track:<id>`, the desktop client's URI form.
    if let Some(rest) = input.strip_prefix("spotify:") {
        let mut parts = rest.split(':');
        let media_type = MediaType::from_str(parts.next()?).ok()?;
        let media_id = parts.next()?;
        return non_empty(media_id).map(|id| ParsedSpotifyUrl {
            media_type,
            media_id: id,
        });
    }

    let parsed = url::Url::parse(input).ok()?;
    let segments: Vec<&str> = parsed.path_segments()?.collect();
    // `windows(2)` rather than `position` + index: the pair is what matters,
    // and this cannot run off the end.
    segments.windows(2).find_map(|pair| {
        let media_type = MediaType::from_str(pair[0]).ok()?;
        non_empty(pair[1]).map(|media_id| ParsedSpotifyUrl {
            media_type,
            media_id,
        })
    })
}

/// `Some(owned)` when the segment has content, `None` when it is empty.
fn non_empty(segment: &str) -> Option<String> {
    (!segment.is_empty()).then(|| segment.to_string())
}

/// Parse a Spotify link, following a `spotify.link` shortlink first.
///
/// A shortlink carries no entity id of its own, so it has to be followed
/// before it means anything. Failing to follow it is not fatal here: parsing
/// the original then fails in the ordinary way and the caller reports "not a
/// usable Spotify link", which is true either way.
pub async fn parse_link(url: &str) -> Option<ParsedSpotifyUrl> {
    let target = if url.contains("spotify.link") {
        http_utils::resolve_final_url(url)
            .await
            .unwrap_or_else(|_| url.to_string())
    } else {
        url.to_string()
    };
    parse_spotify_url(&target)
}

/// One client for the process.
///
/// `from_env` builds a reqwest client, and reqwest pools connections
/// internally -- building one per invocation would discard the pool every
/// time. The init is fallible (a bad `SLEEVENOTE_URL`), so the result is what
/// gets cached; retrying it per call would just re-fail identically.
static CLIENT: OnceLock<Result<Sleevenote, String>> = OnceLock::new();

/// Whether an operator pointed this bot at a sleevenote deployment.
///
/// [`Sleevenote::from_env`] falls back to a localhost default when the
/// variable is unset, which is right for someone running the service beside
/// the bot and wrong for everyone else: without this check an unconfigured
/// bot reports "I can't reach Spotify lookup right now", which reads as a
/// transient outage of something that was never set up. The distinction is
/// only observable here, at the environment, because by the time the request
/// fails both cases are the same refused connection.
#[must_use]
pub fn is_configured() -> bool {
    std::env::var(BASE_URL_ENV).is_ok_and(|value| !value.trim().is_empty())
}

/// The shared sleevenote client, or why there isn't one.
///
/// # Errors
///
/// Returns the stringified init failure when the configured base URL is not
/// usable. Absent configuration is not an error here -- the client falls back
/// to its own default base URL.
pub fn client() -> Result<&'static Sleevenote, String> {
    CLIENT
        .get_or_init(|| Sleevenote::from_env().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// What a Spotify link resolved to: everything playback and messaging need.
///
/// The tracks are carried rather than pre-rendered into queries because the
/// two callers want different things out of them -- playback wants searches,
/// the playlist loader wants metadata to store -- and deriving both from one
/// list is what keeps them describing the same songs.
#[derive(Debug, Clone)]
pub struct SpotifyResolution {
    /// Which kind of entity the link pointed at.
    pub media_type: MediaType,
    /// Display name of the entity: track title, album title, playlist name.
    pub name: String,
    /// The playable songs, in listing order, with podcast episodes removed.
    pub tracks: Vec<Track>,
    /// Items sleevenote itself could not recover -- typically local files.
    ///
    /// Carried rather than discarded so that "this playlist has 3 songs" and
    /// "this playlist has 3 of 40 songs" stay distinguishable.
    pub unresolved: u32,
    /// Podcast episodes dropped from a playlist: listed, but not songs.
    pub episodes_skipped: usize,
}

impl SpotifyResolution {
    /// One YouTube search query per song, in listing order.
    #[must_use]
    pub fn queries(&self) -> Vec<String> {
        self.tracks.iter().map(track_query).collect()
    }

    /// Storable metadata for each song, in listing order.
    #[must_use]
    pub fn metadata(&self) -> Vec<NewAuxMetadata> {
        self.tracks.iter().map(track_metadata).collect()
    }

    /// How many songs came back.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether nothing playable came back.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

/// Metadata for one Spotify track, for storing or displaying.
///
/// `source_url` stays `None`: a Spotify URL is not something this bot can play
/// from, and putting one here would make a stored entry look playable when the
/// track still has to be found on YouTube first.
#[must_use]
pub fn track_metadata(track: &Track) -> NewAuxMetadata {
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    NewAuxMetadata::new(AuxMetadata {
        track: Some(track.name.clone()),
        artist: (!artists.is_empty()).then_some(artists),
        album: track.album.as_ref().map(|album| album.name.clone()),
        date: None,
        start_time: Some(Duration::ZERO),
        duration: track.duration(),
        channels: Some(2),
        channel: None,
        sample_rate: None,
        source_url: None,
        // The cover art, when Spotify's page exposed one. The rspotify path
        // put the track *title* in this field, which is not a thumbnail.
        thumbnail: track.album.as_ref().and_then(|album| album.image.clone()),
        title: Some(track.name.clone()),
    })
}

/// The YouTube search query for one Spotify track.
///
/// Title first, then artists, matching what the rspotify path built -- the
/// search results this feeds have been tuned against that ordering.
fn track_query(track: &Track) -> String {
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if artists.is_empty() {
        track.name.clone()
    } else {
        format!("{} {}", track.name, artists)
    }
}

/// Keep the songs, drop the podcast episodes, and count what was dropped.
///
/// A playlist can hold podcast episodes. Searching YouTube for an episode
/// title returns something, and that something is not the episode -- so they
/// are dropped rather than silently turned into wrong audio, and counted
/// rather than dropped silently.
fn songs_only(tracks: Vec<Track>) -> (Vec<Track>, usize) {
    let total = tracks.len();
    let songs: Vec<Track> = tracks.into_iter().filter(|t| !t.is_episode()).collect();
    let episodes = total - songs.len();
    (songs, episodes)
}

/// Resolve a Spotify link to the searches that will play it.
///
/// Handles the three link shapes users actually paste: a canonical
/// `open.spotify.com` URL, a locale-prefixed one, and a `spotify.link`
/// shortlink (followed first, since it carries no id of its own).
///
/// # Errors
///
/// [`CrackedError::Other`] carrying a message written for the user. Each
/// sleevenote failure keeps its own message: "this does not exist" and "our
/// scraper broke" call for opposite reactions from whoever reads it, and
/// collapsing them here would throw that away at the last moment.
pub async fn resolve_spotify(url: &str) -> Result<SpotifyResolution, CrackedError> {
    let parsed = parse_link(url)
        .await
        .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?;

    let client = client().map_err(|why| {
        tracing::error!("sleevenote client unavailable: {why}");
        CrackedError::Other(SPOTIFY_NOT_CONFIGURED)
    })?;

    let id = parsed.media_id();
    let resolution = match parsed.media_type() {
        MediaType::Track => client.track(id).await.map(|track| {
            let name = track.name.clone();
            let (tracks, episodes_skipped) = songs_only(vec![track]);
            SpotifyResolution {
                media_type: MediaType::Track,
                name,
                tracks,
                unresolved: 0,
                episodes_skipped,
            }
        }),
        MediaType::Album => client.album(id).await.map(|album| {
            let (tracks, episodes_skipped) = songs_only(album.tracks);
            SpotifyResolution {
                media_type: MediaType::Album,
                name: album.name,
                tracks,
                unresolved: album.unresolved_items,
                episodes_skipped,
            }
        }),
        MediaType::Playlist => client.playlist(id).await.map(|playlist| {
            let (tracks, episodes_skipped) = songs_only(playlist.tracks);
            SpotifyResolution {
                media_type: MediaType::Playlist,
                name: playlist.name,
                tracks,
                unresolved: playlist.unresolved_items,
                episodes_skipped,
            }
        }),
    };

    let resolution = resolution.map_err(|err| CrackedError::Other(user_message(&err)))?;

    // Resolving to nothing is a success on the wire and a failure to the user:
    // a podcast-only playlist, or one that is nothing but local files.
    if resolution.is_empty() {
        return Err(CrackedError::Other(SPOTIFY_NOTHING_PLAYABLE));
    }
    Ok(resolution)
}

/// The message to show a user for a sleevenote failure.
///
/// Each arm is a different diagnosis, which is the entire reason the client
/// keeps these variants apart. The ones that are our fault rather than the
/// caller's are logged on the way past: the user cannot act on them, but we
/// can.
#[must_use]
pub fn user_message(err: &SleevenoteError) -> &'static str {
    match err {
        SleevenoteError::NotFound(_) => SPOTIFY_NOT_FOUND,
        SleevenoteError::InvalidId(_) => SPOTIFY_INVALID_QUERY,
        SleevenoteError::Timeout(_) => SPOTIFY_TIMEOUT,
        // Not the caller's fault and not retryable by them: the service
        // stopped matching Spotify's page. Offering a retry would be a lie.
        SleevenoteError::ExtractionEmpty(_) | SleevenoteError::ExtractionIncomplete(_) => {
            tracing::error!("sleevenote extraction failed: {err}");
            SPOTIFY_LOOKUP_BROKEN
        },
        // The service is unreachable, which is different again from the
        // service answering with a failure -- and "never configured" is
        // different again from that. All three arrive as a refused
        // connection; only the environment can tell them apart.
        SleevenoteError::Transport(_) if !is_configured() => {
            tracing::error!("{BASE_URL_ENV} is not set, so Spotify links cannot be resolved");
            SPOTIFY_NOT_CONFIGURED
        },
        SleevenoteError::Transport(_) => {
            tracing::error!("sleevenote unreachable: {err}");
            SPOTIFY_UNREACHABLE
        },
        other => {
            tracing::error!("sleevenote lookup failed: {other}");
            SPOTIFY_LOOKUP_FAILED
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_links() {
        let parsed = parse_spotify_url("https://open.spotify.com/track/3Vr5jdQHibI2q0A0KW4RWk")
            .expect("canonical track link");
        assert_eq!(parsed.media_type(), MediaType::Track);
        assert_eq!(parsed.media_id(), "3Vr5jdQHibI2q0A0KW4RWk");
    }

    #[test]
    fn drops_the_si_tracking_suffix() {
        let parsed =
            parse_spotify_url("https://open.spotify.com/album/1XkGORuUX2QGOEIL4EbJKm?si=x")
                .expect("album link with tracking suffix");
        assert_eq!(parsed.media_type(), MediaType::Album);
        assert_eq!(parsed.media_id(), "1XkGORuUX2QGOEIL4EbJKm");
    }

    #[test]
    fn parses_locale_prefixed_links() {
        // The regression this parser exists for: Spotify's web player hands
        // out `/intl-xx/` links, and the old regex read "intl-de/track" as the
        // entity kind and rejected the whole link.
        for input in [
            "https://open.spotify.com/intl-de/track/3Vr5jdQHibI2q0A0KW4RWk",
            "https://open.spotify.com/intl-pt/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc",
        ] {
            let parsed = parse_spotify_url(input).unwrap_or_else(|| panic!("should parse {input}"));
            assert!(!parsed.media_id().is_empty());
            assert!(!parsed.media_id().contains('/'));
        }
        assert_eq!(
            parse_spotify_url("https://open.spotify.com/intl-de/track/3Vr5jdQHibI2q0A0KW4RWk")
                .unwrap()
                .media_type(),
            MediaType::Track
        );
    }

    #[test]
    fn parses_the_uri_form() {
        let parsed =
            parse_spotify_url("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").expect("uri form");
        assert_eq!(parsed.media_type(), MediaType::Playlist);
        assert_eq!(parsed.media_id(), "37i9dQZF1DXcBWIGoYBM5M");
    }

    #[test]
    fn rejects_what_it_cannot_resolve() {
        // A shortlink has to be followed before it means anything; an
        // unknown entity kind is not something we can look up; an artist or
        // episode page is a Spotify link we deliberately do not play.
        for input in [
            "https://spotify.link/abc123",
            "https://open.spotify.com/artist/0OdUWJ0sBjDrqHygGUXeCF",
            "https://open.spotify.com/track/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "spotify:track:",
            "not a url at all",
        ] {
            assert!(
                parse_spotify_url(input).is_none(),
                "should not parse: {input}"
            );
        }
    }

    /// The captured responses the client crate models its contract against.
    /// Deriving queries and metadata from real service output, rather than
    /// from JSON written to match the code, is what makes these tests say
    /// anything about the wire.
    const TRACK_JSON: &str = include_str!("../../../crack-sleevenote/tests/fixtures/track.json");
    const ALBUM_JSON: &str = include_str!("../../../crack-sleevenote/tests/fixtures/album.json");
    const PLAYLIST_JSON: &str =
        include_str!("../../../crack-sleevenote/tests/fixtures/playlist.json");

    #[test]
    fn builds_a_search_query_from_a_real_track() {
        let track: Track = serde_json::from_str(TRACK_JSON).expect("fixture deserializes");
        // Title first, then artists -- the ordering the rspotify path used and
        // that the YouTube search results were tuned against.
        assert_eq!(track_query(&track), "King Creole Elvis Presley");
    }

    #[test]
    fn carries_a_real_track_into_metadata() {
        let track: Track = serde_json::from_str(TRACK_JSON).expect("fixture deserializes");
        let metadata = track_metadata(&track);
        let aux = metadata.metadata();
        assert_eq!(aux.title.as_deref(), Some("King Creole"));
        assert_eq!(aux.artist.as_deref(), Some("Elvis Presley"));
        assert_eq!(aux.album.as_deref(), Some("60 Original Hits"));
        assert_eq!(aux.duration, Some(Duration::from_millis(129_880)));
        // A Spotify URL is not something this bot can play from, so an entry
        // stored from one must not look playable.
        assert_eq!(aux.source_url, None);
        // Cover art, not the track title, which is what the rspotify path put
        // in this field.
        let thumbnail = aux.thumbnail.as_deref().expect("album art");
        assert!(thumbnail.starts_with("http"), "thumbnail: {thumbnail}");
    }

    #[test]
    fn every_album_track_becomes_a_query() {
        let album: crack_sleevenote::Album =
            serde_json::from_str(ALBUM_JSON).expect("fixture deserializes");
        assert_eq!(album.unresolved_items, 0);
        let (songs, episodes) = songs_only(album.tracks);
        assert_eq!(songs.len(), 60);
        assert_eq!(episodes, 0);
        assert!(songs.iter().map(track_query).all(|q| !q.trim().is_empty()));
    }

    #[test]
    fn a_playlist_drops_episodes_and_keeps_the_count() {
        // This fixture is literally named "Song, Podcast, Local file": two
        // items resolved (one of them a podcast episode) and two the service
        // could not recover at all.
        let playlist: crack_sleevenote::Playlist =
            serde_json::from_str(PLAYLIST_JSON).expect("fixture deserializes");
        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.unresolved_items, 2);

        let (songs, episodes) = songs_only(playlist.tracks);
        // The episode is dropped rather than searched for on YouTube, where a
        // title search would return something that is not the episode.
        assert_eq!(episodes, 1);
        assert_eq!(songs.len(), 1);
        assert_eq!(track_query(&songs[0]), "Oh Shit I'm Feeling It DjCorny");
        // Four items in, one song out: the counts are what keep that legible
        // instead of looking like a one-song playlist.
        assert_eq!(
            songs.len() + episodes + playlist.unresolved_items as usize,
            4
        );
    }

    #[test]
    fn media_type_nouns_read_in_a_sentence() {
        assert_eq!(MediaType::Track.noun(), "track");
        assert!(!MediaType::Track.is_collection());
        assert!(MediaType::Album.is_collection());
        assert!(MediaType::Playlist.is_collection());
    }
}
