use crate::{
    errors::CrackedError,
    messaging::messages::{SPOTIFY_INVALID_QUERY, SPOTIFY_PLAYLIST_FAILED},
};
use lazy_static::lazy_static;
use regex::Regex;
use rspotify::{
    clients::BaseClient,
    model::{AlbumId, PlayableItem, PlaylistId, SimplifiedArtist, TrackId},
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

#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    PlaylistLink(String),
    File(serenity::all::Attachment),
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
