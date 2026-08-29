//! Contract tests against real captured sleevenote v0.1.0 responses.
//!
//! See `tests/fixtures/README.md` for where these files came from and why they
//! must never be edited to make a test pass. Nothing here touches the network:
//! the fixtures are compiled in, and the error taxonomy is exercised through
//! [`Error::from_response`], which is the same mapping the live client uses.

use crack_sleevenote::{
    Album, AlbumTag, Artist, Error, ErrorBody, ErrorCode, Playlist, PlaylistTag, Track, TrackAlbum,
    TrackTag, TrackUrlKind,
};
use std::time::Duration;

const TRACK_JSON: &str = include_str!("fixtures/track.json");
const ALBUM_JSON: &str = include_str!("fixtures/album.json");
const PLAYLIST_JSON: &str = include_str!("fixtures/playlist.json");
const NOT_FOUND_JSON: &str = include_str!("fixtures/notfound.json");
const INVALID_JSON: &str = include_str!("fixtures/invalid.json");

// ---------------------------------------------------------------------------
// Success shapes
// ---------------------------------------------------------------------------

#[test]
fn track_fixture_deserializes_field_for_field() {
    let track: Track = serde_json::from_str(TRACK_JSON).expect("track.json must deserialize");

    assert_eq!(track.id, "2h8wlptrZOjSnZQKoNnLge");
    assert_eq!(track.tag, TrackTag::Track);
    assert_eq!(track.name, "King Creole");
    assert_eq!(
        track.artists,
        vec![Artist {
            name: "Elvis Presley".to_owned(),
            id: Some("43ZHCT0cAZBISjO8DG9PnE".to_owned()),
        }]
    );
    assert_eq!(
        track.album,
        Some(TrackAlbum {
            name: "60 Original Hits".to_owned(),
            id: Some("5s5svl5DzlSmEvkjuL8Upw".to_owned()),
            image: Some(
                "https://i.scdn.co/image/ab67616d0000b2730a8e47fff104c12be39d234c".to_owned()
            ),
        })
    );
    // camelCase `durationMs` must land on snake_case `duration_ms`.
    assert_eq!(track.duration_ms, Some(129_880));
    assert_eq!(track.duration(), Some(Duration::from_millis(129_880)));
    assert_eq!(
        track.url,
        "https://open.spotify.com/track/2h8wlptrZOjSnZQKoNnLge"
    );
    assert_eq!(track.url_kind(), TrackUrlKind::Track);
    assert!(!track.is_episode());
    assert_eq!(
        track.primary_artist().map(|a| a.name.as_str()),
        Some("Elvis Presley")
    );
}

#[test]
fn album_fixture_has_sixty_tracks_and_nothing_unresolved() {
    let album: Album = serde_json::from_str(ALBUM_JSON).expect("album.json must deserialize");

    assert_eq!(album.id, "5s5svl5DzlSmEvkjuL8Upw");
    assert_eq!(album.tag, AlbumTag::Album);
    assert_eq!(album.name, "60 Original Hits");
    assert_eq!(album.artists.len(), 1);
    assert_eq!(album.artists[0].name, "Elvis Presley");
    assert_eq!(
        album.image.as_deref(),
        Some("https://i.scdn.co/image/ab67616d0000b2730a8e47fff104c12be39d234c")
    );
    assert_eq!(
        album.url,
        "https://open.spotify.com/album/5s5svl5DzlSmEvkjuL8Upw"
    );

    assert_eq!(album.tracks.len(), 60, "the album has 60 tracks");
    assert_eq!(album.unresolved_items, 0);
    assert_eq!(album.total_items(), 60);
    assert!(album.is_complete());

    assert_eq!(album.tracks[0].name, "King Creole");
    assert_eq!(album.tracks[59].name, "Playing For Keeps");
    assert_eq!(album.tracks[59].duration_ms, Some(170_880));

    // Every track inside an album listing carries `album: null`. That is the
    // nullable-but-present case: if it were modelled with `#[serde(default)]`
    // an omitted key would be indistinguishable from an explicit null.
    for track in &album.tracks {
        assert_eq!(track.tag, TrackTag::Track);
        assert!(track.album.is_none(), "{} carried an album", track.name);
        assert!(!track.artists.is_empty(), "{} had no artists", track.name);
        assert_eq!(track.url_kind(), TrackUrlKind::Track);
    }
}

#[test]
fn playlist_fixture_reports_two_tracks_and_two_unresolved() {
    let playlist: Playlist =
        serde_json::from_str(PLAYLIST_JSON).expect("playlist.json must deserialize");

    assert_eq!(playlist.id, "3tlExkExp1aaYcU91Qhp79");
    assert_eq!(playlist.tag, PlaylistTag::Playlist);
    assert_eq!(playlist.name, "Song, Podcast, Local file");
    assert_eq!(playlist.owner.as_deref(), Some("Lothrop"));
    assert_eq!(
        playlist.url,
        "https://open.spotify.com/playlist/3tlExkExp1aaYcU91Qhp79"
    );

    assert_eq!(playlist.tracks.len(), 2);
    // The whole point of the field: two resolved items out of four seen. A
    // client that drops this reports a half-broken playlist as a complete one.
    assert_eq!(playlist.unresolved_items, 2);
    assert_eq!(playlist.total_items(), 4);
    assert!(!playlist.is_complete());

    assert_eq!(playlist.tracks[0].name, "Oh Shit I'm Feeling It");
    assert_eq!(playlist.tracks[0].duration_ms, Some(215_484));
}

#[test]
fn playlist_fixture_contains_a_podcast_episode_not_a_track() {
    let playlist: Playlist =
        serde_json::from_str(PLAYLIST_JSON).expect("playlist.json must deserialize");

    let song = &playlist.tracks[0];
    let episode = &playlist.tracks[1];

    // Same `type: "track"` discriminator on both -- the shape does not tell
    // them apart, only the URL does.
    assert_eq!(song.tag, TrackTag::Track);
    assert_eq!(episode.tag, TrackTag::Track);

    assert_eq!(song.url_kind(), TrackUrlKind::Track);
    assert!(!song.is_episode());

    assert_eq!(episode.name, "178: Ubiquiti");
    assert_eq!(
        episode.url,
        "https://open.spotify.com/episode/6CQAC1k7sUVk8FQsXABlRU"
    );
    assert_eq!(
        episode.url_kind(),
        TrackUrlKind::Episode,
        "an /episode/ URL must not be classified as a track"
    );
    assert!(episode.is_episode());
    // Its "artist" is the show, not a musician.
    assert_eq!(
        episode.primary_artist().map(|a| a.name.as_str()),
        Some("Darknet Diaries")
    );
}

// ---------------------------------------------------------------------------
// Error taxonomy
// ---------------------------------------------------------------------------

#[test]
fn not_found_fixture_maps_to_the_not_found_variant() {
    let body: ErrorBody =
        serde_json::from_str(NOT_FOUND_JSON).expect("notfound.json must deserialize");
    assert_eq!(body.error, ErrorCode::NotFound);
    assert_eq!(body.id, "0000000000000000000000");
    assert_eq!(body.message, "no track found for id 0000000000000000000000");

    let Error::NotFound(detail) = Error::from_response(404, NOT_FOUND_JSON) else {
        panic!("404 not_found must map to Error::NotFound");
    };
    assert_eq!(detail.status, 404);
    assert_eq!(detail.id, "0000000000000000000000");
    assert_eq!(
        detail.message,
        "no track found for id 0000000000000000000000"
    );
}

#[test]
fn invalid_id_fixture_maps_to_the_invalid_id_variant() {
    let body: ErrorBody =
        serde_json::from_str(INVALID_JSON).expect("invalid.json must deserialize");
    assert_eq!(body.error, ErrorCode::InvalidId);
    assert_eq!(body.id, "not!valid");
    assert_eq!(body.message, "id must match ^[A-Za-z0-9]{1,64}$");

    let Error::InvalidId(detail) = Error::from_response(400, INVALID_JSON) else {
        panic!("400 invalid_id must map to Error::InvalidId");
    };
    assert_eq!(detail.status, 400);
    assert_eq!(detail.id, "not!valid");
    assert_eq!(detail.message, "id must match ^[A-Za-z0-9]{1,64}$");
}

/// Build a wire-shaped error body from typed values. Nothing here constructs
/// JSON by hand; the body is a real `ErrorBody` serialized the same way the
/// service's own types would emit it.
fn error_response(code: ErrorCode, status: u16) -> Error {
    let body = ErrorBody {
        error: code,
        id: "3tlExkExp1aaYcU91Qhp79".to_owned(),
        message: "synthesized for the taxonomy test".to_owned(),
    };
    let json = serde_json::to_string(&body).expect("ErrorBody must serialize");
    Error::from_response(status, &json)
}

#[test]
fn the_three_bad_gateways_and_the_gateway_timeout_stay_distinct() {
    // This is the assertion the service exists to make possible. Three of the
    // four share HTTP 502; if any pair of them collapsed into one variant a
    // caller could no longer tell a broken scraper from a slow one.
    let empty = error_response(ErrorCode::ExtractionEmpty, 502);
    let incomplete = error_response(ErrorCode::ExtractionIncomplete, 502);
    let internal = error_response(ErrorCode::Internal, 502);
    let timeout = error_response(ErrorCode::Timeout, 504);

    assert!(matches!(empty, Error::ExtractionEmpty(_)), "{empty}");
    assert!(
        matches!(incomplete, Error::ExtractionIncomplete(_)),
        "{incomplete}"
    );
    assert!(matches!(internal, Error::Internal(_)), "{internal}");
    assert!(matches!(timeout, Error::Timeout(_)), "{timeout}");

    let codes: Vec<_> = [&empty, &incomplete, &internal, &timeout]
        .iter()
        .map(|e| e.code().expect("service errors carry a code"))
        .collect();
    assert_eq!(
        codes,
        vec![
            ErrorCode::ExtractionEmpty,
            ErrorCode::ExtractionIncomplete,
            ErrorCode::Internal,
            ErrorCode::Timeout,
        ]
    );

    // Every documented code round-trips to a distinct variant, and each keeps
    // the status it arrived with.
    for (code, status) in [
        (ErrorCode::InvalidId, 400),
        (ErrorCode::NotFound, 404),
        (ErrorCode::ExtractionEmpty, 502),
        (ErrorCode::ExtractionIncomplete, 502),
        (ErrorCode::Timeout, 504),
        (ErrorCode::Internal, 502),
    ] {
        let err = error_response(code.clone(), status);
        assert_eq!(err.code(), Some(code));
        assert_eq!(err.detail().map(|d| d.status), Some(status));
    }
}

#[test]
fn an_unknown_code_is_preserved_rather_than_collapsed() {
    let err = error_response(ErrorCode::Unrecognized("rate_limited".to_owned()), 429);
    let Error::Unrecognized { code, detail } = &err else {
        panic!("an unknown code must not be folded into a known variant: {err}");
    };
    assert_eq!(code, "rate_limited");
    assert_eq!(detail.status, 429);
}

#[test]
fn a_non_error_body_does_not_pretend_to_be_the_taxonomy() {
    // A proxy or ingress answering instead of sleevenote.
    let err = Error::from_response(502, "<html>502 Bad Gateway</html>");
    let Error::UnexpectedStatus { status, body } = &err else {
        panic!("a non-conforming body must not be coerced into a code: {err}");
    };
    assert_eq!(*status, 502);
    assert_eq!(body, "<html>502 Bad Gateway</html>");
    assert_eq!(err.code(), None);
    assert!(err.detail().is_none());
}

// ---------------------------------------------------------------------------
// Drift detection
// ---------------------------------------------------------------------------

#[test]
fn every_fixture_round_trips_byte_for_byte() {
    // A value-by-value assertion catches a field we model going wrong. This
    // catches the other direction: a field the service sends that we do not
    // model, or one we emit under the wrong name. Field order in each struct
    // matches the service's own declaration order, so the re-serialized bytes
    // must equal the captured bytes exactly.
    fn round_trip<T>(label: &str, raw: &str)
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        let parsed: T = serde_json::from_str(raw).unwrap_or_else(|e| panic!("{label}: {e}"));
        let reserialized = serde_json::to_string(&parsed).expect("must serialize");
        assert_eq!(
            reserialized, raw,
            "{label} did not round trip; the wire shape and our types disagree"
        );
    }

    round_trip::<Track>("track.json", TRACK_JSON);
    round_trip::<Album>("album.json", ALBUM_JSON);
    round_trip::<Playlist>("playlist.json", PLAYLIST_JSON);
    round_trip::<ErrorBody>("notfound.json", NOT_FOUND_JSON);
    round_trip::<ErrorBody>("invalid.json", INVALID_JSON);
}

#[test]
fn a_changed_type_discriminator_fails_to_deserialize() {
    let drifted = TRACK_JSON.replace(r#""type":"track""#, r#""type":"song""#);
    assert_ne!(drifted, TRACK_JSON, "the substitution must have applied");
    assert!(
        serde_json::from_str::<Track>(&drifted).is_err(),
        "a discriminator change must be a hard failure, not a value nobody reads"
    );
}

#[test]
fn an_omitted_nullable_key_fails_to_deserialize() {
    // `durationMs` is documented `number | null`: always present, sometimes
    // null. Modelling it `Option<u64>` *without* `#[serde(default)]` is what
    // makes an omitted key a contract violation instead of a silent None.
    let drifted = TRACK_JSON.replace(r#""durationMs":129880,"#, "");
    assert_ne!(drifted, TRACK_JSON, "the substitution must have applied");
    assert!(
        serde_json::from_str::<Track>(&drifted).is_err(),
        "an omitted nullable key must fail, not default to None"
    );

    // An explicit null, by contrast, is legal and means None.
    let nulled = TRACK_JSON.replace(r#""durationMs":129880"#, r#""durationMs":null"#);
    let track: Track = serde_json::from_str(&nulled).expect("explicit null is legal");
    assert_eq!(track.duration_ms, None);
    assert_eq!(track.duration(), None);
}

#[test]
fn a_renamed_field_fails_to_deserialize() {
    // If the service ever went snake_case, we must find out from a red test.
    let drifted = PLAYLIST_JSON.replace(r#""unresolvedItems""#, r#""unresolved_items""#);
    assert_ne!(drifted, PLAYLIST_JSON, "the substitution must have applied");
    assert!(
        serde_json::from_str::<Playlist>(&drifted).is_err(),
        "a camelCase -> snake_case rename must fail loudly"
    );
}
