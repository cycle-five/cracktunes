//! What is left of the rspotify client: recommendations, and nothing else.
//!
//! Every path that turns a Spotify *link* into something playable now goes
//! through [`crate::sources::sleevenote`], which needs no credentials. This
//! module survives only because sleevenote resolves ids to metadata and has no
//! recommendations endpoint, so autoplay
//! ([`crate::handlers::track_end`]) has nowhere else to go.
//!
//! It is dead in practice: Spotify stopped issuing Web API credentials in
//! roughly December 2025, so [`Spotify::auth`] fails on every deployment that
//! did not already hold a client id and secret. Autoplay says so
//! ([`AUTOPLAY_DISABLED_SPOTIFY`](crate::messaging::messages::AUTOPLAY_DISABLED_SPOTIFY))
//! rather than failing silently. Keep this until Spotify issues credentials
//! again, or until autoplay picks its next track some other way.

use crate::{errors::CrackedError, utils::MUSIC_SEARCH_SUFFIX};
use crack_types::{QueryType, SpotifyTrack};
use lazy_static::lazy_static;
use rspotify::model::{FullTrack, SimplifiedAlbum};
use rspotify::{
    clients::BaseClient,
    model::{Country, Market, Recommendations, SearchResult, SimplifiedArtist, TrackId},
    ClientCredsSpotify, Config, Credentials,
};
use std::{collections::HashMap, env, time::Duration};
use tokio::sync::Mutex;

lazy_static! {
    pub static ref SPOTIFY: Mutex<Result<ClientCredsSpotify, CrackedError>> =
        Mutex::new(Err(CrackedError::Other("no auth attempts")));
}

type SpotifyCreds = Credentials;

// #[derive(Debug, Clone)]
// pub struct SpotifyPlaylist(FullPlaylist);

// impl Deref for SpotifyPlaylist {
//     type Target = FullPlaylist;

//     fn deref(&self) -> &Self::Target {
//         &self.0
//     }
// }

// impl DerefMut for SpotifyPlaylist {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.0
//     }
// }

/// Spotify source.
#[derive(Debug, Clone)]
pub struct Spotify {}

/// Implementation of Spotify source.
impl Spotify {
    /// Authenticate with Spotify.
    pub async fn auth(opt_creds: Option<SpotifyCreds>) -> Result<ClientCredsSpotify, CrackedError> {
        let spotify_client_id = match opt_creds.clone() {
            Some(creds) => creds.id,
            None => env::var("SPOTIFY_CLIENT_ID")
                .map_err(|_| CrackedError::Other("missing spotify client ID"))?,
        };
        let spotify_client_secret = match opt_creds {
            Some(creds) => creds.secret.unwrap_or("".to_string()),
            None => env::var("SPOTIFY_CLIENT_SECRET")
                .map_err(|_| CrackedError::Other("missing spotify client secret"))?,
        };

        let creds = Credentials::new(&spotify_client_id, &spotify_client_secret);
        let config = Config {
            token_refreshing: true,
            ..Default::default()
        };

        let spotify = ClientCredsSpotify::with_config(creds, config);
        spotify.request_token().await?;

        Ok(spotify)
    }

    /// Search Spotify for a query.
    pub async fn search(
        spotify: &ClientCredsSpotify,
        query: &str,
    ) -> Result<QueryType, CrackedError> {
        let search_result = spotify
            .search(
                query,
                rspotify::model::SearchType::Track,
                None,
                None,
                None,
                None,
            )
            .await?;

        Self::extract_search_results(search_result)
    }

    /// Get recommendations based on a list of tracks.
    pub async fn get_recommendations(
        spotify: &ClientCredsSpotify,
        tracks: Vec<String>,
    ) -> Result<Vec<String>, CrackedError> {
        let mut track_ids = Vec::new();
        for track in &tracks {
            let search_result = spotify
                .search(
                    track,
                    rspotify::model::SearchType::Track,
                    None,
                    None,
                    None,
                    None,
                )
                .await?;
            tracing::trace!("search_result: {:?}", search_result);
            let tracks = Self::search_result_to_track_id(search_result);
            tracing::warn!("tracks len: {:?}", tracks.len());
            track_ids.append(&mut tracks.clone());
        }
        let recommendations: Recommendations = spotify
            .recommendations(
                Vec::new(),
                None::<Vec<_>>,
                None::<Vec<_>>,
                Some(track_ids),
                Some(Market::Country(Country::UnitedStates)),
                Some(5),
            )
            .await
            .map_err(CrackedError::RSpotify)?;

        let query_list: Vec<String> = recommendations
            .tracks
            .iter()
            .map(|track| Self::build_query(&track.artists[0].name, &track.name))
            .collect();

        Ok(query_list)
    }

    /// Get track ids based on a search result.
    fn _search_result_to_track_ids(search_result: SearchResult) -> Vec<TrackId<'static>> {
        match search_result {
            SearchResult::Tracks(tracks) => {
                tracks.items.iter().flat_map(|x| x.id.clone()).collect()
            },
            _ => Vec::new(),
        }
    }

    /// Search results to a single track id.
    fn search_result_to_track_id(search_result: SearchResult) -> Vec<TrackId<'static>> {
        match search_result {
            SearchResult::Tracks(tracks) => tracks
                .items
                .iter()
                .flat_map(|x| x.id.clone())
                .take(1)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// SearchResult to a QueryType.
    fn extract_search_results(search_result: SearchResult) -> Result<QueryType, CrackedError> {
        match search_result {
            SearchResult::Albums(albums) => {
                let album = albums.items[0].clone();
                let artist_names = Self::join_artist_names(&album.artists);
                let query = Self::build_query(&artist_names, &album.name);
                Ok(QueryType::Keywords(query))
            },
            SearchResult::Artists(artists) => {
                let artist = artists.items[0].clone();
                let query = artist.name;
                Ok(QueryType::Keywords(query))
            },
            SearchResult::Playlists(playlists) => {
                let playlist = playlists.items[0].clone();
                let query = playlist.name;
                Ok(QueryType::Keywords(query))
            },
            SearchResult::Tracks(tracks) => {
                let track = tracks.items[0].clone();
                let artist_names = Self::join_artist_names(&track.artists);
                let query = Self::build_query(&artist_names, &track.name);
                Ok(QueryType::Keywords(query))
            },
            SearchResult::Shows(shows) => {
                let show = shows.items[0].clone();
                let query = show.name;
                Ok(QueryType::Keywords(query))
            },
            SearchResult::Episodes(episodes) => {
                let episode = episodes.items[0].clone();
                let query = episode.name;
                Ok(QueryType::Keywords(query))
            },
        }
    }

    /// Build a query for searching, from the artist names and the track name.
    fn build_query(artists: &str, track_name: &str) -> String {
        format!("{} {}", artists, track_name)
    }

    /// Build a query for searching, from the artist names and the track name.
    fn _build_query_lyric(artists: &str, track_name: &str) -> String {
        format!("{} {} {}", artists, track_name, MUSIC_SEARCH_SUFFIX)
    }

    /// Join the artist names into a single string.
    fn join_artist_names(artists: &[SimplifiedArtist]) -> String {
        let artist_names: Vec<String> = artists.iter().map(|artist| artist.name.clone()).collect();
        artist_names.join(" ")
    }
}

// /// Wrapper for a Spotify track.
// #[derive(Debug, Clone)]
// pub struct SpotifyTrack {
//     pub full_track: rspotify::model::FullTrack,
// }

pub trait SpotifyTrackTrait {
    fn new(full_track: rspotify::model::FullTrack) -> Self;
    fn id(&self) -> TrackId<'static>;
    fn name(&self) -> String;
    fn artists(&self) -> Vec<SimplifiedArtist>;
    fn artists_str(&self) -> String;
    fn album(&self) -> SimplifiedAlbum;
    fn album_name(&self) -> String;
    fn duration_seconds(&self) -> i32;
    fn duration(&self) -> Duration;
    fn join_artist_names(&self) -> String;
    fn build_query_lyric(&self) -> String;
    fn build_query(&self) -> String;
}

/// Implementation of our SpotifyTrackTrait.
impl SpotifyTrackTrait for SpotifyTrack {
    /// Create a new SpotifyTrack.
    fn new(full_track: rspotify::model::FullTrack) -> Self {
        Self { full_track }
    }

    /// Get the ID of the track
    fn id(&self) -> TrackId<'static> {
        self.full_track.id.clone().unwrap()
    }

    /// Get the name of the track.
    fn name(&self) -> String {
        self.full_track.name.clone()
    }

    /// Get the artists of the track.
    fn artists(&self) -> Vec<SimplifiedArtist> {
        self.full_track.artists.clone()
    }

    /// Get the artists of the track as a string.
    fn artists_str(&self) -> String {
        self.full_track
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect::<Vec<String>>()
            .join(", ")
    }

    /// Get the album of the track.
    fn album(&self) -> rspotify::model::SimplifiedAlbum {
        self.full_track.album.clone()
    }

    /// Get the album name of the track.
    fn album_name(&self) -> String {
        self.full_track.album.name.clone()
    }

    /// Get the duration of the track.
    fn duration_seconds(&self) -> i32 {
        self.full_track.duration.num_seconds() as i32
    }

    /// Get the duration of the track as a Duration.
    fn duration(&self) -> Duration {
        let track_secs = self.full_track.duration.num_seconds();
        let nanos = self.full_track.duration.subsec_nanos();
        let secs = if track_secs < 0 { 0 } else { track_secs };
        Duration::new(secs as u64, nanos as u32)
    }

    /// Join the artist names into a single string.
    fn join_artist_names(&self) -> String {
        let artist_names: Vec<String> = self
            .full_track
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect();
        artist_names.join(" ")
    }

    /// Build a query for searching, from the artist names and the track name.
    fn build_query_lyric(&self) -> String {
        format!(
            "{} {} {}",
            self.name(),
            self.join_artist_names(),
            MUSIC_SEARCH_SUFFIX
        )
    }

    /// Build a query for searching, from the artist names and the track name.
    fn build_query(&self) -> String {
        format!("{} {}", self.name(), self.join_artist_names())
    }
}

/// Implementation of From for SpotifyTrack.
///
/// `#[allow(deprecated)]`: rspotify 0.16 deprecated `popularity`,
/// `linked_from` and `available_markets` on `FullTrack`, and `album_group` /
/// `available_markets` on `SimplifiedAlbum`, because Spotify removed those
/// fields from its API. They are still struct fields, so a literal must
/// initialize them -- and CI runs `-D warnings`. We never READ any of them;
/// they exist here only to satisfy the constructor. Drop the attribute once
/// rspotify removes the fields outright.
#[allow(deprecated)]
pub fn build_fake_spotify_track() -> SpotifyTrack {
    SpotifyTrack::new(FullTrack {
        id: None,
        name: "asdf".to_string(),
        artists: vec![SimplifiedArtist {
            external_urls: HashMap::new(),
            href: None,
            id: None,
            name: "qwer".to_string(),
        }],
        album: SimplifiedAlbum {
            album_type: None,
            album_group: None,
            artists: vec![],
            available_markets: vec![],
            external_urls: HashMap::new(),
            href: None,
            id: None,
            images: vec![],
            name: "zxcv".to_string(),
            release_date: Some("2012".to_string()),
            release_date_precision: None,
            restrictions: None,
        },
        track_number: 0,
        disc_number: 0,
        explicit: false,
        external_urls: HashMap::new(),
        href: None,
        preview_url: None,
        popularity: 0,
        is_playable: None,
        linked_from: None,
        restrictions: None,
        external_ids: HashMap::new(),
        is_local: false,
        available_markets: vec![],
        duration: chrono::TimeDelta::new(60, 0).unwrap(),
        r#type: rspotify::model::Type::Track,
    })
}

#[cfg(test)]
mod test {
    use crack_types::NewAuxMetadata;

    use super::*;

    // The Spotify URL parser and its tests moved to
    // `crate::sources::sleevenote`, which is where URL parsing now lives. The
    // replacements also cover the locale-prefixed links (`/intl-de/track/...`)
    // that the regex these tested silently rejected.

    #[test]
    fn test_from_spotify_track() {
        let track = build_fake_spotify_track();
        let res = NewAuxMetadata::from_spotify_track(&track);
        let metadata = res.metadata();
        assert_eq!(metadata.title, Some("asdf".to_string()));
        assert_eq!(metadata.artist, Some("qwer".to_string()));
        assert_eq!(metadata.album, Some("zxcv".to_string()));
        assert_eq!(metadata.duration.unwrap().as_secs(), 60);
    }

    #[test]
    fn test_track_build_query() {
        let track = build_fake_spotify_track();
        let query = track.build_query();
        assert_eq!(query, r#"asdf qwer"#);
    }

    #[test]
    fn test_track_build_query_lyric() {
        let track = build_fake_spotify_track();
        let query = track.build_query_lyric();
        assert_eq!(query, r#"asdf qwer \"topic\""#);
    }
}
