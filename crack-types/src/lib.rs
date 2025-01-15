#![allow(clippy::no_effect_underscore_binding)]
// ------------------------------------------------------------------
// Modules
// ------------------------------------------------------------------
pub mod http;
pub use http::*;
pub mod metadata;
pub use metadata::*;
pub mod reply_handle;
pub use reply_handle::*;
pub mod messaging;
pub use messaging::*;
pub mod errors;
pub use errors::*;

use rspotify::model::SimplifiedAlbum;
use rspotify::model::SimplifiedArtist;
use rspotify::model::TrackId;
// ------------------------------------------------------------------
// Non-public imports
// ------------------------------------------------------------------
use serenity::small_fixed_array::FixedString;
use serenity::small_fixed_array::ValidLength;
use songbird::Call;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
// ------------------------------------------------------------------
// Public types we use to simplify return and parameter types.
// ------------------------------------------------------------------

pub type ArcTRwLock<T> = Arc<RwLock<T>>;
pub type ArcTMutex<T> = Arc<Mutex<T>>;
pub type ArcRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcTRwMap<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub type ArcMutDMap<K, V> = Arc<Mutex<HashMap<K, V>>>;
pub type CrackedResult<T> = Result<T, CrackedError>;
pub type CrackedHowResult<T> = anyhow::Result<T, CrackedError>;
pub type CommandError = Error;
pub type CommandResult<E = Error> = Result<(), E>;
pub type SongbirdCall = Arc<Mutex<Call>>;

// ------------------------------------------------------------------
// Public Re-exports
// ------------------------------------------------------------------
pub use serenity::all::Attachment;
pub use serenity::all::UserId;
pub use thiserror::Error as ThisError;
pub use typemap_rev::TypeMapKey;

// ------------------------------------------------------------------
// Constants and Enums
// ------------------------------------------------------------------
pub const MUSIC_SEARCH_SUFFIX: &str = r#"\"topic\""#;

pub(crate) const DEFAULT_VALID_TOKEN: &str =
    "XXXXXXXXXXXXXXXXXXXXXXXX.X_XXXX.XXXXXXXXXXXXXXXXXXXXXX_XXXX";

/// Custom error type for track resolve errors.
#[derive(ThisError, Debug)]
pub enum TrackResolveError {
    #[error("No track found")]
    NotFound,
    #[error("Query is empty")]
    EmptyQuery,
    #[error("Error: {0}")]
    Other(String),
    #[error("Unknown resolve error")]
    Unknown,
    #[error("Unknown query type")]
    UnknownQueryType,
}

/// play Mode enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    End,
    Next,
    All,
    Reverse,
    Shuffle,
    Jump,
    DownloadMKV,
    DownloadMP3,
    Search,
}

use serenity::all::Token;
use serenity::model::id::{ChannelId, GuildId};
/// Enum for 64 bit integer Ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiscId {
    U64(u64),
    SBGuildId(GuildId),
    GuildId(GuildId),
    ChannelId(ChannelId),
    UserId(UserId),
}

impl From<GuildId> for DiscId {
    fn from(id: GuildId) -> Self {
        DiscId::GuildId(id)
    }
}

impl From<ChannelId> for DiscId {
    fn from(id: ChannelId) -> Self {
        DiscId::ChannelId(id)
    }
}

impl From<UserId> for DiscId {
    fn from(id: UserId) -> Self {
        DiscId::UserId(id)
    }
}

impl From<u64> for DiscId {
    fn from(id: u64) -> Self {
        DiscId::U64(id)
    }
}

impl From<DiscId> for u64 {
    fn from(id: DiscId) -> Self {
        match id {
            DiscId::U64(id) => id,
            DiscId::GuildId(id) | DiscId::SBGuildId(id) => id.get(),
            DiscId::ChannelId(id) => id.get(),
            DiscId::UserId(id) => id.get(),
        }
    }
}

/// New struct pattern to wrap the spotify track.
#[derive(Clone, Debug)]
pub struct SpotifyTrack {
    pub full_track: FullTrack,
}

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

/// Implementation of our `[SpotifyTrackTrait]`.
impl SpotifyTrackTrait for SpotifyTrack {
    /// Create a new `SpotifyTrack`.
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
        i32::try_from(self.full_track.duration.num_seconds()).unwrap_or(0)
    }

    /// Get the duration of the track as a Duration.
    #[allow(clippy::cast_sign_loss)]
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
            &self.name(),
            &self.join_artist_names(),
            MUSIC_SEARCH_SUFFIX
        )
    }

    /// Build a query for searching, from the artist names and the track name.
    fn build_query(&self) -> String {
        format!("{} {}", &self.name(), &self.join_artist_names())
    }
}

/// Enum for type of possible queries we have to handle.
// QueryType -> Keywords       -> ResolvedTrack
//           -> VideoLink      -> ResolvedTrack
//           -> File           -> ResolvedTrack
//           -> NewYoutubeDl   -> ResolvedTrack        X
//           -> SpotifyTracks  -> Vec<ResolvedTrack>
//           -> PlaylistLink   -> Vec<ResolvedTrack>
//           -> KeywordList    -> Vec<ResolvedTrack>
//           -> YoutubeSearch  -> Vec<ResolvedTrack>
#[derive(Clone, Debug)]
pub enum QueryType {
    Keywords(String),
    KeywordList(Vec<String>),
    VideoLink(String),
    SpotifyTracks(Vec<SpotifyTrack>),
    PlaylistLink(String),
    File(Attachment),
    NewYoutubeDl((YoutubeDl<'static>, AuxMetadata)),
    YoutubeSearch(String),
    None,
}

impl std::str::FromStr for QueryType {
    type Err = TrackResolveError;
    /// Get the query type from a string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("https://") || s.starts_with("http://") {
            Ok(QueryType::VideoLink(s.to_string()))
        } else {
            Ok(QueryType::Keywords(s.to_string()))
        }
    }
}

impl Display for QueryType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryType::Keywords(keywords) => write!(f, "{keywords}"),
            QueryType::KeywordList(keywords_list) => write!(f, "{}", keywords_list.join(" ")),
            QueryType::SpotifyTracks(tracks) => write!(
                f,
                "{}",
                tracks
                    .iter()
                    .map(SpotifyTrackTrait::name)
                    .collect::<Vec<String>>()
                    .join(" ")
            ),
            QueryType::PlaylistLink(url) | QueryType::VideoLink(url) => write!(f, "{url}"),
            QueryType::File(file) => write!(f, "{}", file.url),
            QueryType::NewYoutubeDl((_src, metadata)) => {
                write!(f, "{}", metadata.clone().source_url.unwrap_or_default())
            },
            QueryType::YoutubeSearch(query) => write!(f, "{query}"),
            QueryType::None => write!(f, "None"),
        }
    }
}

impl QueryType {
    /// Build a query string from the query type.
    #[must_use]
    pub fn build_query(&self) -> Option<String> {
        let base = self.build_query_base();
        base.map(|x| format!("{x} {MUSIC_SEARCH_SUFFIX}"))
    }

    /// Build a query string from the query type.
    #[must_use]
    pub fn build_query_base(&self) -> Option<String> {
        match self {
            QueryType::Keywords(keywords) => Some(keywords.clone()),
            QueryType::KeywordList(keywords_list) => Some(keywords_list.join(" ")),
            QueryType::VideoLink(url) => Some(url.clone()),
            QueryType::SpotifyTracks(tracks) => Some(
                tracks
                    .iter()
                    .map(SpotifyTrackTrait::build_query)
                    .collect::<Vec<String>>()
                    .join(" "),
            ),
            QueryType::PlaylistLink(url) => Some(url.to_string()),
            QueryType::File(file) => Some(file.url.to_string()),
            QueryType::NewYoutubeDl((_src, metadata)) => metadata.source_url.clone(),
            QueryType::YoutubeSearch(query) => Some(query.clone()),
            QueryType::None => None,
        }
    }
}

// /// Build a query for searching, from the artist names and the track name.
// fn build_query_spotify(artists: &str, track_name: &str) -> String {
//     format!("{} {}", artists, track_name)
// }

/// [`Default`] implementation for [`QueryType`].
impl Default for QueryType {
    fn default() -> Self {
        QueryType::None
    }
}

/// Enum for the requesting user of a track.
#[derive(Debug, Clone)]
pub enum RequestingUser {
    UserId(UserId),
}

/// Convert [`Option<UserId>`] to [`RequestingUser`].
impl From<Option<UserId>> for RequestingUser {
    fn from(user_id: Option<UserId>) -> Self {
        match user_id {
            Some(user_id) => RequestingUser::UserId(user_id),
            None => RequestingUser::default(),
        }
    }
}

/// Convert [`UserId`] to [`RequestingUser`].
impl From<UserId> for RequestingUser {
    fn from(user_id: UserId) -> Self {
        RequestingUser::UserId(user_id)
    }
}

/// We implement `TypeMapKey` for `RequestingUser`.
impl TypeMapKey for RequestingUser {
    type Value = RequestingUser;
}

/// `Default` implementation for `RequestingUser`.
impl Default for RequestingUser {
    fn default() -> Self {
        let user = UserId::new(1);
        RequestingUser::UserId(user)
    }
}

/// Function to get the full artist name from a [`FullTrack`].
#[must_use]
pub fn full_track_artist_str(track: &FullTrack) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.clone())
        .collect::<Vec<String>>()
        .join(", ")
}

// use humantime::format_duration;
/// Converts a duration into a human readable timestamp
#[must_use]
pub fn get_human_readable_timestamp(duration: Option<Duration>) -> String {
    // let duration = match duration {
    //     Some(duration) if duration == Duration::MAX => return "∞".to_string(),
    //     Some(duration) => duration,
    //     None => return "∞".to_string(),
    // };
    // let formatted = format_duration(duration);
    // formatted.substring(11, formatted.len() - 1).to_string()
    match duration {
        Some(duration) if duration == Duration::MAX => "∞".to_string(),
        Some(duration) => {
            let seconds = duration.as_secs() % 60;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = duration.as_secs() / 3600;

            if hours < 1 {
                format!("{minutes:02}:{seconds:02}")
            } else {
                format!("{hours:02}:{minutes:02}:{seconds:02}")
            }
        },
        None => "∞".to_string(),
    }
}

/// Builds a fake [`rusty_ytdl::Author`] for testing purposes.
#[must_use]
pub fn build_fake_rusty_author() -> rusty_ytdl::Author {
    rusty_ytdl::Author {
        id: "id".to_string(),
        name: "name".to_string(),
        user: "user".to_string(),
        channel_url: "channel_url".to_string(),
        external_channel_url: "external_channel_url".to_string(),
        user_url: "user_url".to_string(),
        thumbnails: vec![],
        verified: false,
        subscriber_count: 0,
    }
}

/// Builds a fake [`rusty_ytdl::Embed`] for testing purposes.
#[must_use]
pub fn build_fake_rusty_embed() -> rusty_ytdl::Embed {
    rusty_ytdl::Embed {
        flash_secure_url: "flash_secure_url".to_string(),
        flash_url: "flash_url".to_string(),
        iframe_url: "iframe_url".to_string(),
        width: 0,
        height: 0,
    }
}

/// Builds a fake [`VideoDetails`] for testing purposes.
#[must_use]
pub fn build_fake_rusty_video_details() -> rusty_ytdl::VideoDetails {
    rusty_ytdl::VideoDetails {
        author: Some(build_fake_rusty_author()),
        likes: 0,
        dislikes: 0,
        age_restricted: false,
        video_url: "video_url".to_string(),
        storyboards: vec![],
        chapters: vec![],
        embed: build_fake_rusty_embed(),
        title: "title".to_string(),
        description: "description".to_string(),
        length_seconds: "60".to_string(),
        owner_profile_url: "owner_profile_url".to_string(),
        external_channel_id: "external_channel_id".to_string(),
        is_family_safe: false,
        available_countries: vec![],
        is_unlisted: false,
        has_ypc_metadata: false,
        view_count: "0".to_string(),
        category: "category".to_string(),
        publish_date: "publish_date".to_string(),
        owner_channel_name: "owner_channel_name".to_string(),
        upload_date: "upload_date".to_string(),
        video_id: "video_id".to_string(),
        keywords: vec![],
        channel_id: "channel_id".to_string(),
        is_owner_viewing: false,
        is_crawlable: false,
        allow_ratings: false,
        is_private: false,
        is_unplugged_corpus: false,
        is_live_content: false,
        thumbnails: vec![rusty_ytdl::Thumbnail {
            url: "thumbnail_url".to_string(),
            width: 0,
            height: 0,
        }],
    }
}

/// Builds a fake but valid [`Token`] for testing purposes.
/// # Panics
/// * If the token is invalid.
#[must_use]
pub fn get_valid_token() -> Token {
    DEFAULT_VALID_TOKEN.parse::<Token>().expect("Invalid token")
}

/// Convert a string to a fixed string.
/// # Panics
/// * If the string is not a valid length.
pub fn to_fixed<T: ValidLength>(s: impl Into<String>) -> FixedString<T> {
    FixedString::from_str(&s.into()).unwrap()
}

#[cfg(test)]
mod tests {
    use serenity::small_fixed_array::FixedString;

    use super::*;

    #[test]
    fn test_get_human_readable_timestamp() {
        assert_eq!(get_human_readable_timestamp(None), "∞");
        assert_eq!(get_human_readable_timestamp(Some(Duration::MAX)), "∞");
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(0)),),
            "00:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(59)),),
            "00:59"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(60)),),
            "01:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(61)),),
            "01:01"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3599)),),
            "59:59"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3600)),),
            "01:00:00"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3601)),),
            "01:00:01"
        );
        assert_eq!(
            get_human_readable_timestamp(Some(Duration::from_secs(3661)),),
            "01:01:01"
        );
    }

    #[test]
    fn test_video_details_to_aux_metadata() {
        let details = build_fake_rusty_video_details();
        let metadata = video_details_to_aux_metadata(&details);
        assert_eq!(metadata.title, Some("title".to_string()));
        assert_eq!(metadata.source_url, Some("video_url".to_string()));
        assert_eq!(metadata.channel, Some("owner_channel_name".to_string()));
        assert_eq!(metadata.duration, Some(Duration::from_secs(60)));
        assert_eq!(metadata.date, Some("publish_date".to_string()));
        assert_eq!(metadata.thumbnail, Some("thumbnail_url".to_string()));
    }

    #[test]
    fn test_video_info_to_aux_metadata() {
        let details = build_fake_rusty_video_details();
        let info = VideoInfo {
            video_details: details,
            dash_manifest_url: Some("dash_manifest_url".to_string()),
            hls_manifest_url: Some("hls_manifest_url".to_string()),
            formats: vec![],
            related_videos: vec![],
        };
        let metadata = video_info_to_aux_metadata(&info);
        assert_eq!(metadata.title, Some("title".to_string()));
        assert_eq!(metadata.source_url, Some("video_url".to_string()));
        assert_eq!(metadata.channel, Some("owner_channel_name".to_string()));
        assert_eq!(metadata.duration, Some(Duration::from_secs(60)));
        assert_eq!(metadata.date, Some("publish_date".to_string()));
        assert_eq!(metadata.thumbnail, Some("thumbnail_url".to_string()));
    }

    #[test]
    fn test_to_fixed() {
        let fixed: FixedString<u8> = to_fixed("12345");
        assert_eq!(fixed, FixedString::from_str("12345").unwrap());
    }
}
