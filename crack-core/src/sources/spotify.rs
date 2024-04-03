use crate::{
    commands::QueryType,
    errors::CrackedError,
    messaging::messages::{SPOTIFY_INVALID_QUERY, SPOTIFY_PLAYLIST_FAILED},
};
use lazy_static::lazy_static;
use regex::Regex;
use rspotify::{
    clients::BaseClient,
    model::{
        AlbumId, Country, Market, PlayableItem, PlaylistId, Recommendations, SearchResult,
        SimplifiedArtist, TrackId,
    },
    ClientCredsSpotify, Config, Credentials,
};
use std::{env, str::FromStr, time::Duration};
use tokio::sync::Mutex;

lazy_static! {
    pub static ref SPOTIFY: Mutex<Result<ClientCredsSpotify, CrackedError>> =
        Mutex::new(Err(CrackedError::Other("no auth attempts")));
    pub static ref SPOTIFY_QUERY_REGEX: Regex =
        Regex::new(r"spotify.link/.*|spotify.com/(?P<media_type>.+)/(?P<media_id>.*?)(?:\?|$)")
            .unwrap();
}

#[derive(Clone, Copy)]
pub enum MediaType {
    Track,
    Album,
    Playlist,
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

pub struct ParsedSpotifyUrl {
    media_type: MediaType,
    media_id: String,
}

type SpotifyCreds = Credentials;
pub struct Spotify {}

impl Spotify {
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

    pub async fn parse_spotify_url(query: &str) -> Result<ParsedSpotifyUrl, CrackedError> {
        let captures = SPOTIFY_QUERY_REGEX
            .captures(query)
            .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?;

        let media_type = captures
            .name("media_type")
            .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?
            .as_str();

        let media_type = MediaType::from_str(media_type)
            .map_err(|_| CrackedError::Other(SPOTIFY_INVALID_QUERY))?;

        let media_id = captures
            .name("media_id")
            .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?
            .as_str();

        Ok(ParsedSpotifyUrl {
            media_type,
            media_id: media_id.to_string(),
        })
    }

    pub async fn extract_tracks(
        spotify: &ClientCredsSpotify,
        query: &str,
    ) -> Result<Vec<SpotifyTrack>, CrackedError> {
        let ParsedSpotifyUrl {
            media_type,
            media_id,
        } = Self::parse_spotify_url(query).await?;

        let media_id = media_id.as_str();

        match media_type {
            MediaType::Playlist => Self::get_playlist_tracks(spotify, media_id).await,
            _ => Err(CrackedError::Other(SPOTIFY_INVALID_QUERY)),
        }
    }

    pub async fn extract(
        spotify: &ClientCredsSpotify,
        query: &str,
    ) -> Result<QueryType, CrackedError> {
        // let captures = SPOTIFY_QUERY_REGEX
        //     .captures(query)
        //     .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?;

        // let media_type = captures
        //     .name("media_type")
        //     .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?
        //     .as_str();

        // let media_type = MediaType::from_str(media_type)
        //     .map_err(|_| CrackedError::Other(SPOTIFY_INVALID_QUERY))?;

        // let media_id = captures
        //     .name("media_id")
        //     .ok_or(CrackedError::Other(SPOTIFY_INVALID_QUERY))?
        //     .as_str();

        let ParsedSpotifyUrl {
            media_type,
            media_id,
        } = Self::parse_spotify_url(query).await?;

        let media_id = media_id.as_str();

        match media_type {
            MediaType::Track => Self::get_track_info(spotify, media_id).await,
            MediaType::Album => Self::get_album_info(spotify, media_id).await,
            MediaType::Playlist => Self::get_playlist_info(spotify, media_id).await,
        }
    }

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
            }
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
            }
            SearchResult::Artists(artists) => {
                let artist = artists.items[0].clone();
                let query = artist.name;
                Ok(QueryType::Keywords(query))
            }
            SearchResult::Playlists(playlists) => {
                let playlist = playlists.items[0].clone();
                let query = playlist.name;
                Ok(QueryType::Keywords(query))
            }
            SearchResult::Tracks(tracks) => {
                let track = tracks.items[0].clone();
                let artist_names = Self::join_artist_names(&track.artists);
                let query = Self::build_query(&artist_names, &track.name);
                Ok(QueryType::Keywords(query))
            }
            SearchResult::Shows(shows) => {
                let show = shows.items[0].clone();
                let query = show.name;
                Ok(QueryType::Keywords(query))
            }
            SearchResult::Episodes(episodes) => {
                let episode = episodes.items[0].clone();
                let query = episode.name;
                Ok(QueryType::Keywords(query))
            }
        }
    }

    /// Get a search query as a QueryType from a spotify track id.
    async fn get_track_info(
        spotify: &ClientCredsSpotify,
        id: &str,
    ) -> Result<QueryType, CrackedError> {
        let track_id = TrackId::from_id(id)
            .map_err(|_| CrackedError::Other("track ID contains invalid characters"))?;

        let track = spotify
            .track(track_id, None)
            .await
            .map_err(CrackedError::RSpotify)?;

        let artist_names = Self::join_artist_names(&track.artists);

        let query = Self::build_query(&artist_names, &track.name);
        Ok(QueryType::Keywords(query))
    }

    async fn get_album_info(
        spotify: &ClientCredsSpotify,
        id: &str,
    ) -> Result<QueryType, CrackedError> {
        let album_id = AlbumId::from_id(id)
            .map_err(|_| CrackedError::Other("album ID contains invalid characters"))?;

        let album = spotify
            .album(album_id, None)
            .await
            .map_err(|_| CrackedError::Other("failed to fetch album"))?;

        let artist_names = Self::join_artist_names(&album.artists);

        let query_list: Vec<String> = album
            .tracks
            .items
            .iter()
            .map(|track| Self::build_query(&artist_names, &track.name))
            .collect();

        Ok(QueryType::KeywordList(query_list))
    }

    /// Returns a list of queries from a Spotify playlist.
    async fn get_playlist_info(
        spotify: &ClientCredsSpotify,
        id: &str,
    ) -> Result<QueryType, CrackedError> {
        let playlist_id = PlaylistId::from_id(id)
            .map_err(|_| CrackedError::Other("playlist ID contains invalid characters"))?;

        let playlist = spotify
            .playlist(playlist_id, None, None)
            .await
            .map_err(|_| CrackedError::Other(SPOTIFY_PLAYLIST_FAILED))?;

        let query_list: Vec<String> = playlist
            .tracks
            .items
            .iter()
            .filter_map(|item| match item.track.as_ref().unwrap() {
                PlayableItem::Track(track) => {
                    let artist_names = Self::join_artist_names(&track.album.artists);
                    Some(Self::build_query(&artist_names, &track.name))
                }
                PlayableItem::Episode(_) => None,
            })
            .collect();

        Ok(QueryType::KeywordList(query_list))
    }

    pub async fn get_playlist_tracks(
        spotify: &ClientCredsSpotify,
        id: &str,
    ) -> Result<Vec<SpotifyTrack>, CrackedError> {
        let playlist_id = PlaylistId::from_id(id)
            .map_err(|_| CrackedError::Other("playlist ID contains invalid characters"))?;

        let playlist = spotify
            .playlist(playlist_id, None, None)
            .await
            .map_err(|_| CrackedError::Other(SPOTIFY_PLAYLIST_FAILED))?;

        let query_list: Vec<SpotifyTrack> = playlist
            .tracks
            .items
            .iter()
            .filter_map(|item| match item.track.as_ref().unwrap() {
                PlayableItem::Track(track) => Some(SpotifyTrack::new(track.clone())),
                PlayableItem::Episode(_) => None,
            })
            .collect();

        Ok(query_list)
    }

    fn build_query(artists: &str, track_name: &str) -> String {
        format!("{} - {}", artists, track_name)
    }

    fn join_artist_names(artists: &[SimplifiedArtist]) -> String {
        let artist_names: Vec<String> = artists.iter().map(|artist| artist.name.clone()).collect();
        artist_names.join(" ")
    }
}

/// Wrapper for a Spotify track.
#[derive(Debug, Clone)]
pub struct SpotifyTrack {
    pub full_track: rspotify::model::FullTrack,
}

/// Implementation of SpotifyTrack.
impl SpotifyTrack {
    /// Create a new SpotifyTrack.
    pub fn new(full_track: rspotify::model::FullTrack) -> Self {
        Self { full_track }
    }

    /// Get the ID of the track
    pub fn id(&self) -> TrackId<'static> {
        self.full_track.id.clone().unwrap()
    }

    /// Get the name of the track.
    pub fn name(&self) -> String {
        self.full_track.name.clone()
    }

    /// Get the artists of the track.
    pub fn artists(&self) -> Vec<SimplifiedArtist> {
        self.full_track.artists.clone()
    }

    /// Get the artists of the track as a string.
    pub fn artists_str(&self) -> String {
        self.full_track
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect::<Vec<String>>()
            .join(", ")
    }

    /// Get the album of the track.
    pub fn album(&self) -> rspotify::model::SimplifiedAlbum {
        self.full_track.album.clone()
    }

    /// Get the album name of the track.
    pub fn album_name(&self) -> String {
        self.full_track.album.name.clone()
    }

    /// Get the duration of the track.
    pub fn duration_seconds(&self) -> i32 {
        self.full_track.duration.num_seconds() as i32
    }

    /// Get the duration of the track as a Duration.
    pub fn duration(&self) -> Duration {
        let track_secs = self.full_track.duration.num_seconds();
        let nanos = self.full_track.duration.subsec_nanos();
        let secs = if track_secs < 0 { 0 } else { track_secs };
        Duration::new(secs as u64, nanos as u32)
    }

    /// Join the artist names into a single string.
    pub fn join_artist_names(&self) -> String {
        let artist_names: Vec<String> = self
            .full_track
            .artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect();
        artist_names.join(" ")
    }

    /// Build a query for searching, from the artist names and the track name.
    pub fn build_query(&self) -> String {
        format!("{} - {}", &self.join_artist_names(), &self.name())
    }
}

impl From<rspotify::model::FullTrack> for SpotifyTrack {
    fn from(track: rspotify::model::FullTrack) -> Self {
        Self::new(track)
    }
}
