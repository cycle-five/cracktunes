use crate::{
    commands::play_utils::QueryType,
    errors::CrackedError,
    messaging::messages::{SPOTIFY_INVALID_QUERY, SPOTIFY_PLAYLIST_FAILED},
    utils::MUSIC_SEARCH_SUFFIX,
};
use lazy_static::lazy_static;
use regex::Regex;
use rspotify::model::{FullPlaylist, FullTrack, SimplifiedAlbum};
use rspotify::{
    clients::BaseClient,
    model::{
        AlbumId, Country, Market, PlayableItem, PlaylistId, Recommendations, SearchResult,
        SimplifiedArtist, TrackId,
    },
    ClientCredsSpotify, ClientResult, Config, Credentials,
};
use std::{collections::HashMap, env, str::FromStr, time::Duration};
use tokio::sync::Mutex;

lazy_static! {
    pub static ref SPOTIFY: Mutex<Result<ClientCredsSpotify, CrackedError>> =
        Mutex::new(Err(CrackedError::Other("no auth attempts")));
    pub static ref SPOTIFY_QUERY_REGEX: Regex =
        Regex::new(r"spotify.link/.*|spotify.com/(?P<media_type>.+)/(?P<media_id>.*?)(?:\?|$)")
            .unwrap();
}

pub struct CrackClientCredsSpotify(ClientCredsSpotify);

impl CrackClientCredsSpotify {
    pub async fn new(opt_creds: Option<SpotifyCreds>) -> Result<Self, CrackedError> {
        let creds = Spotify::auth(opt_creds).await?;
        Ok(Self(creds))
    }

    pub async fn get(&self) -> Result<&ClientCredsSpotify, CrackedError> {
        Ok(&self.0)
    }

    pub async fn playlist(
        &self,
        playlist_id: PlaylistId<'_>,
        fields: Option<&str>,
        market: Option<rspotify::model::Market>,
    ) -> ClientResult<FullPlaylist> {
        self.0.playlist(playlist_id, fields, market).await
    }
}

/// Media type for Spotify.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MediaType {
    Track,
    Album,
    Playlist,
}

/// Implementation of FromStr for MediaType.
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

#[derive(Debug, Clone)]
pub struct ParsedSpotifyUrl {
    media_type: MediaType,
    media_id: String,
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

    /// Parse a Spotify URL.
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

    /// Extract tracks from a Spotify query.
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

    /// Extract a `QueryType` from a Spotify query.
    pub async fn extract(
        spotify: &ClientCredsSpotify,
        query: &str,
    ) -> Result<QueryType, CrackedError> {
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

    /// Get the info of a Spotify album as a `QueryType`.
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
                },
                PlayableItem::Episode(_) => None,
            })
            .collect();

        Ok(QueryType::KeywordList(query_list))
    }

    /// Get a list of SpotifyTracks from a Spotify playlist.
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

    /// Build a query for searching, from the artist names and the track name.
    fn build_query(artists: &str, track_name: &str) -> String {
        format!("{} {} {}", artists, track_name, MUSIC_SEARCH_SUFFIX)
    }

    /// Join the artist names into a single string.
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
        format!(
            "{} {} {}",
            &self.name(),
            &self.join_artist_names(),
            MUSIC_SEARCH_SUFFIX
        )
    }

    /// Build a query for searching, from the artist names and the track name.
    pub fn build_query_base(&self) -> String {
        format!("{} {}", &self.name(), &self.join_artist_names())
    }
}

/// Implementation of From for SpotifyTrack.
impl From<rspotify::model::FullTrack> for SpotifyTrack {
    fn from(track: rspotify::model::FullTrack) -> Self {
        Self::new(track)
    }
}

/// Implementation of From for SpotifyTrack.
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
    })
}

#[cfg(test)]
mod test {
    use crate::commands::MyAuxMetadata;

    use super::*;
    // use rspotify::model::{FullTrack, SimplifiedAlbum};
    // use std::collections::HashMap;

    // // Mock ClientCredsSpotify
    // struct MockClientCredsSpotify {}

    // /// Mock

    // #[tokio::test]
    // async fn test_auth() {
    //     let creds = Credentials::new("id", "secret");
    //     let spotify = Spotify::auth(Some(creds)).await;
    //     assert!(spotify.is_ok());
    // }

    #[tokio::test]
    async fn test_parse_spotify_url_fail() {
        let url = "https://open.spotify.com/trak/4uLU6hMCjMI75M1A2tKUQC?si=4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await;
        assert!(parsed.is_err());

        let url = "https://open.spoify.com/album/4uLU6hMCjMI75M1A2tKUQC?si=4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await;
        assert!(parsed.is_err());

        let url = "https://open.spotify.com/playlis/";
        let parsed = Spotify::parse_spotify_url(url).await;
        assert!(parsed.is_err());
    }

    #[tokio::test]
    async fn test_parse_spotify_url() {
        let url = "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Track);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");

        let url = "https://open.spotify.com/album/4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Album);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");

        let url = "https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Playlist);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");

        let url = "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Track);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");

        let url = "https://open.spotify.com/album/4uLU6hMCjMI75M1A2tKUQC?si=4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Album);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");

        let url =
            "https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC?si=4uLU6hMCjMI75M1A2tKUQC";
        let parsed = Spotify::parse_spotify_url(url).await.unwrap();
        assert_eq!(parsed.media_type, MediaType::Playlist);
        assert_eq!(parsed.media_id, "4uLU6hMCjMI75M1A2tKUQC");
    }

    // #[tokio::test]
    // async fn test_extract_tracks() {
    //     let spotify = Spotify::auth(None).await.unwrap();
    //     let tracks = Spotify::extract_tracks(
    //         &spotify,
    //         "https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC",
    //     )
    //     .await
    //     .unwrap();
    //     assert_eq!(tracks.len(), 50);
    // }
    #[test]
    fn test_from_spotify_track() {
        let track = build_fake_spotify_track();
        let res = MyAuxMetadata::from_spotify_track(&track);
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
        assert_eq!(query, r#"asdf qwer \"topic\""#);
    }
}
