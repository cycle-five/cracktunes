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
use std::{env, str::FromStr};
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

    pub async fn extract(
        spotify: &ClientCredsSpotify,
        query: &str,
    ) -> Result<QueryType, CrackedError> {
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

    fn _search_result_to_track_ids(search_result: SearchResult) -> Vec<TrackId<'static>> {
        match search_result {
            SearchResult::Tracks(tracks) => {
                tracks.items.iter().flat_map(|x| x.id.clone()).collect()
            }
            _ => Vec::new(),
        }
    }

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

    fn build_query(artists: &str, track_name: &str) -> String {
        format!("{} - {}", artists, track_name)
    }

    fn join_artist_names(artists: &[SimplifiedArtist]) -> String {
        let artist_names: Vec<String> = artists.iter().map(|artist| artist.name.clone()).collect();
        artist_names.join(" ")
    }
}
