use crate::messaging::messages::{
    EMPTY_SEARCH_RESULT, FAIL_ANOTHER_CHANNEL, FAIL_AUTHOR_DISCONNECTED, FAIL_AUTHOR_NOT_FOUND,
    FAIL_EMPTY_VECTOR, FAIL_INVALID_TOPGG_TOKEN, FAIL_NOTHING_PLAYING, FAIL_NOT_IMPLEMENTED,
    FAIL_NO_SONGBIRD, FAIL_NO_VIRUSTOTAL_API_KEY, FAIL_NO_VOICE_CONNECTION, FAIL_PARSE_TIME,
    FAIL_PLAYLIST_FETCH, FAIL_WRONG_CHANNEL, GUILD_ONLY, NO_DATABASE_POOL, NO_GUILD_CACHED,
    NO_GUILD_ID, NO_GUILD_SETTINGS, QUEUE_IS_EMPTY, SPOTIFY_AUTH_FAILED, UNAUTHORIZED_USER,
};
use crate::Error;
use audiopus::error::Error as AudiopusError;
use poise::serenity_prelude::{self as serenity, ChannelId, GuildId};
use rspotify::ClientError as RSpotifyClientError;
use rusty_ytdl::VideoError;
use serenity::model::mention::Mention;
use serenity::Error as SerenityError;
use songbird::error::JoinError;
use songbird::input::AudioStreamError;
use std::fmt::{self};
use std::fmt::{Debug, Display};
use std::process::ExitStatus;

/// A common error enum returned by most of the crate's functions within a [`Result`].
#[derive(Debug)]
pub enum CrackedError {
    AlreadyConnected(Mention),
    AudioStream(AudioStreamError),
    AuthorDisconnected(Mention),
    AuthorNotFound,
    Anyhow(anyhow::Error),
    #[cfg(feature = "crack-gpt")]
    CrackGPT(Error),
    CommandFailed(String, ExitStatus, String),
    DurationParseError(String, String),
    EmptySearchResult,
    EmptyVector(&'static str),
    GuildOnly,
    JoinChannelError(JoinError),
    Json(serde_json::Error),
    LogChannelWarning(&'static str, GuildId),
    NotInRange(&'static str, isize, isize, isize),
    NotConnected,
    NotImplemented,
    NoTrackName,
    NoDatabasePool,
    NoGuildCached,
    NoGuildId,
    NoGuildForChannelId(ChannelId),
    NoGuildSettings,
    NoLogChannel,
    NoUserAutoplay,
    NothingPlaying,
    NoSongbird,
    NoVirusTotalApiKey,
    Other(&'static str),
    InvalidIP(String),
    InvalidTopGGToken,
    PlayListFail,
    ParseTimeFail,
    PoisonError(Error),
    IO(std::io::Error),
    QueueEmpty,
    Reqwest(reqwest::Error),
    RSpotify(RSpotifyClientError),
    RSpotifyLockError(String),
    SQLX(sqlx::Error),
    Serde(serde_json::Error),
    SerdeStream(serde_stream::Error),
    Songbird(Error),
    Serenity(SerenityError),
    SpotifyAuth,
    Poise(Error),
    TrackFail(Error),
    UrlParse(url::ParseError),
    UnauthorizedUser,
    UnimplementedEvent(ChannelId, &'static str),
    VideoError(VideoError),
    WrongVoiceChannel,
}

/// `CrackedError` implements the [`Debug`] and [`Display`] traits
/// meaning it implements the [`std::error::Error`] trait.
/// This just makes it explicit.
impl std::error::Error for CrackedError {}

/// `CrackedError` implements the [`Send`] trait.
unsafe impl Send for CrackedError {}

/// `CrackedError` implements the [`Sync`] trait.
unsafe impl Sync for CrackedError {}

/// Implementation of the [`Display`] trait for the [`CrackedError`] enum.
/// Errors are formatted with this and then sent as responses to the interaction.
impl Display for CrackedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AudioStream(err) => f.write_str(&format!("{err}")),
            Self::AuthorDisconnected(mention) => {
                f.write_fmt(format_args!("{} {}", FAIL_AUTHOR_DISCONNECTED, mention))
            },
            Self::AuthorNotFound => f.write_str(FAIL_AUTHOR_NOT_FOUND),
            Self::AlreadyConnected(mention) => {
                f.write_fmt(format_args!("{} {}", FAIL_ANOTHER_CHANNEL, mention))
            },
            Self::Anyhow(err) => f.write_str(&format!("{err}")),
            Self::CrackGPT(err) => f.write_str(&format!("{err}")),
            Self::CommandFailed(program, status, output) => f.write_str(&format!(
                "Command `{program}` failed with status `{status}` and output `{output}`"
            )),
            Self::DurationParseError(d, u) => {
                f.write_str(&format!("Failed to parse duration `{d}` and `{u}`",))
            },
            Self::EmptySearchResult => f.write_str(EMPTY_SEARCH_RESULT),
            Self::EmptyVector(msg) => f.write_str(&format!("{} {}", FAIL_EMPTY_VECTOR, msg)),
            Self::GuildOnly => f.write_str(GUILD_ONLY),
            Self::JoinChannelError(err) => f.write_str(&format!("{err}")),
            Self::Json(err) => f.write_str(&format!("{err}")),
            Self::Other(msg) => f.write_str(msg),
            Self::NotInRange(param, value, lower, upper) => f.write_str(&format!(
                "`{param}` should be between {lower} and {upper} but was {value}"
            )),
            Self::NotConnected => f.write_str(FAIL_NO_VOICE_CONNECTION),
            Self::NotImplemented => f.write_str(FAIL_NOT_IMPLEMENTED),
            Self::NoTrackName => f.write_str("No track name"),
            Self::NoDatabasePool => f.write_str(NO_DATABASE_POOL),
            Self::NoGuildCached => f.write_str(NO_GUILD_CACHED),
            Self::NoGuildId => f.write_str(NO_GUILD_ID),
            Self::NoGuildForChannelId(channel_id) => {
                f.write_fmt(format_args!("No guild for channel id {}", channel_id))
            },
            Self::NoGuildSettings => f.write_str(NO_GUILD_SETTINGS),
            Self::NoLogChannel => f.write_str("No log channel"),
            Self::NoUserAutoplay => f.write_str("(auto)"),
            Self::WrongVoiceChannel => f.write_str(FAIL_WRONG_CHANNEL),

            Self::NothingPlaying => f.write_str(FAIL_NOTHING_PLAYING),
            Self::PlayListFail => f.write_str(FAIL_PLAYLIST_FETCH),
            Self::ParseTimeFail => f.write_str(FAIL_PARSE_TIME),
            Self::PoisonError(err) => f.write_str(&format!("{err}")),
            Self::TrackFail(err) => f.write_str(&format!("{err}")),
            Self::Serenity(err) => f.write_str(&format!("{err}")),
            Self::SQLX(err) => f.write_str(&format!("{err}")),
            Self::SpotifyAuth => f.write_str(SPOTIFY_AUTH_FAILED),
            Self::Reqwest(err) => f.write_str(&format!("{err}")),
            Self::RSpotify(err) => f.write_str(&format!("{err}")),
            Self::UnauthorizedUser => f.write_str(UNAUTHORIZED_USER),
            Self::IO(err) => f.write_str(&format!("{err}")),
            Self::InvalidIP(ip) => f.write_str(&format!("Invalid ip {}", ip)),
            Self::InvalidTopGGToken => f.write_str(FAIL_INVALID_TOPGG_TOKEN),
            Self::Serde(err) => f.write_str(&format!("{err}")),
            Self::SerdeStream(err) => f.write_str(&format!("{err}")),
            Self::Songbird(err) => f.write_str(&format!("{err}")),
            Self::NoSongbird => f.write_str(FAIL_NO_SONGBIRD),
            Self::NoVirusTotalApiKey => f.write_str(FAIL_NO_VIRUSTOTAL_API_KEY),
            Self::Poise(err) => f.write_str(&format!("{err}")),
            Self::QueueEmpty => f.write_str(QUEUE_IS_EMPTY),
            Self::LogChannelWarning(event_name, guild_id) => f.write_str(&format!(
                "No log channel set for {event_name} in {guild_id}",
            )),
            Self::UrlParse(err) => f.write_str(&format!("{err}")),
            Self::UnimplementedEvent(channel, value) => f.write_str(&format!(
                "Unimplemented event {value} for channel {channel}",
            )),
            Self::VideoError(err) => f.write_str(&format!("{err}")),
            Self::RSpotifyLockError(_) => todo!(),
        }
    }
}

/// Implementation of the [`PartialEq`] trait for the [`CrackedError`] enum.
/// For some enum variants, values are considered equal when their inner values
/// are equal and for others when they are of the same type.
impl PartialEq for CrackedError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Other(l0), Self::Other(r0)) => l0 == r0,
            (Self::NotInRange(l0, l1, l2, l3), Self::NotInRange(r0, r1, r2, r3)) => {
                l0 == r0 && l1 == r1 && l2 == r2 && l3 == r3
            },
            (Self::AuthorDisconnected(l0), Self::AuthorDisconnected(r0)) => {
                l0.to_string() == r0.to_string()
            },
            (Self::AlreadyConnected(l0), Self::AlreadyConnected(r0)) => {
                l0.to_string() == r0.to_string()
            },
            (Self::Serenity(l0), Self::Serenity(r0)) => format!("{l0:?}") == format!("{r0:?}"),
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

/// Provides an implementation to convert a [`AudioStreamError`] to a [`CrackedError`].
impl From<AudioStreamError> for CrackedError {
    fn from(err: AudioStreamError) -> Self {
        Self::AudioStream(err)
    }
}

/// Provides an implementation to convert a [`sqlx::Error`] to a [`CrackedError`].
impl From<sqlx::Error> for CrackedError {
    fn from(err: sqlx::Error) -> Self {
        Self::SQLX(err)
    }
}

/// Provides an implementation to convert a [`AudiopusError`] to a [`CrackedError`].
impl From<AudiopusError> for CrackedError {
    fn from(err: AudiopusError) -> Self {
        Self::Poise(Box::new(err))
    }
}

/// Provides an implementation to convert a [`Error`] to a [`CrackedError`].
impl From<Error> for CrackedError {
    fn from(err: Error) -> Self {
        CrackedError::Poise(err)
    }
}

/// Provides an implementation to convert a [`serde_stream::Error`] to a [`CrackedError`].
impl From<serde_stream::Error> for CrackedError {
    fn from(err: serde_stream::Error) -> Self {
        CrackedError::SerdeStream(err)
    }
}

/// Provides an implementation to convert a [`std::io::Error`] to a [`CrackedError`].
impl From<std::io::Error> for CrackedError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err)
    }
}

/// Provides an implementation to convert a [`serde_json::Error`] to a [`CrackedError`].
impl From<serde_json::Error> for CrackedError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serde(err)
    }
}

/// Provides an implementation to convert a [`SerenityError`] to a [`CrackedError`].
impl From<SerenityError> for CrackedError {
    fn from(err: SerenityError) -> Self {
        match err {
            SerenityError::NotInRange(param, value, lower, upper) => {
                Self::NotInRange(param, value as isize, lower as isize, upper as isize)
            },
            SerenityError::Other(msg) => Self::Other(msg),
            _ => Self::Serenity(err),
        }
    }
}

/// Provides an implementation to convert a [`reqwest::Error`] to a [`CrackedError`].
impl From<reqwest::Error> for CrackedError {
    fn from(err: reqwest::Error) -> Self {
        Self::Reqwest(err)
    }
}

/// Provides an implementation to convert a [`url::ParseError`] to a [`CrackedError`].
impl From<url::ParseError> for CrackedError {
    fn from(err: url::ParseError) -> Self {
        Self::UrlParse(err)
    }
}

/// Provides an implementation to convert a rspotify [`RSpotifyClientError`] to a [`CrackedError`].
impl From<RSpotifyClientError> for CrackedError {
    fn from(err: RSpotifyClientError) -> CrackedError {
        CrackedError::RSpotify(err)
    }
}

impl From<VideoError> for CrackedError {
    fn from(err: VideoError) -> CrackedError {
        CrackedError::VideoError(err)
    }
}

/// Types that implement this trait can be tested as true or false and also provide
/// a way of unpacking themselves.
pub trait Verifiable<T> {
    fn to_bool(&self) -> bool;
    fn unpack(self) -> T;
}

impl Verifiable<bool> for bool {
    fn to_bool(&self) -> bool {
        *self
    }

    fn unpack(self) -> bool {
        self
    }
}

impl<T> Verifiable<T> for Option<T> {
    fn to_bool(&self) -> bool {
        self.is_some()
    }

    fn unpack(self) -> T {
        self.unwrap()
    }
}

impl<T, E> Verifiable<T> for Result<T, E>
where
    E: Debug,
{
    fn to_bool(&self) -> bool {
        self.is_ok()
    }

    fn unpack(self) -> T {
        self.unwrap()
    }
}

/// Verifies if a value is true (or equivalent).
/// Returns an [`Err`] with the given error or the value wrapped in [`Ok`].
pub fn verify<K, T: Verifiable<K>>(verifiable: T, err: CrackedError) -> Result<K, CrackedError> {
    if verifiable.to_bool() {
        Ok(verifiable.unpack())
    } else {
        Err(err)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::{Error as StdError, ErrorKind};

    #[test]
    fn test_verify() {
        let ok = verify(true, CrackedError::NoGuildCached);
        assert_eq!(ok, Ok(true));

        let err = verify(false, CrackedError::NoGuildCached);
        assert_eq!(err, Err(CrackedError::NoGuildCached));

        let ok = verify(Some(1), CrackedError::NoGuildCached);
        assert_eq!(ok, Ok(1));

        let x: Option<i32> = None;
        let err = verify(x, CrackedError::NoGuildCached);
        assert_eq!(err, Err(CrackedError::NoGuildCached));

        let ok = verify(Ok::<i32, CrackedError>(1), CrackedError::NoGuildCached);
        assert_eq!(ok, Ok(1));
    }

    #[test]
    fn test_cracked_error_display() {
        let err = CrackedError::NoGuildCached;
        assert_eq!(format!("{}", err), NO_GUILD_CACHED);

        let err = CrackedError::NoGuildId;
        assert_eq!(format!("{}", err), NO_GUILD_ID);

        // let err = CrackedError::NoGuildForChannelId(ChannelId(1));
        // assert_eq!(format!("{}", err), "No guild for channel id 1");

        let err = CrackedError::NoGuildSettings;
        assert_eq!(format!("{}", err), NO_GUILD_SETTINGS);

        let err = CrackedError::NoLogChannel;
        assert_eq!(format!("{}", err), "No log channel");

        let err = CrackedError::NoUserAutoplay;
        assert_eq!(format!("{}", err), "(auto)");

        let err = CrackedError::WrongVoiceChannel;
        assert_eq!(format!("{}", err), FAIL_WRONG_CHANNEL);

        let err = CrackedError::NothingPlaying;
        assert_eq!(format!("{}", err), FAIL_NOTHING_PLAYING);

        let err = CrackedError::PlayListFail;
        assert_eq!(format!("{}", err), FAIL_PLAYLIST_FETCH);

        let err = CrackedError::ParseTimeFail;
        assert_eq!(format!("{}", err), FAIL_PARSE_TIME);

        // let err = CrackedError::PoisonError(PoisonError::Other("test"));
        // assert_eq!(format!("{}", err), "No guild id");

        // let err = CrackedError::TrackFail(Error::Other("test"));
        // assert_eq!(format!("{}", err), "No guild id");

        let err = CrackedError::Serenity(SerenityError::Other("test"));
        assert_eq!(format!("{}", err), "test");

        let err = CrackedError::SQLX(sqlx::Error::RowNotFound);
        assert_eq!(
            format!("{}", err),
            "no rows returned by a query that expected to return at least one row"
        );

        let err = CrackedError::SpotifyAuth;
        assert_eq!(format!("{}", err), SPOTIFY_AUTH_FAILED);

        let response = reqwest::blocking::get("http://notreallol");
        let err = CrackedError::Reqwest(response.unwrap_err());
        assert!(format!("{}", err).starts_with("error sending request for url"));

        // let err = CrackedError::RSpotify(RSpotifyClientError::Unauthorized);
        // assert_eq!(format!("{}", err), "Unauthorized");

        let err = CrackedError::UnauthorizedUser;
        assert_eq!(format!("{}", err), UNAUTHORIZED_USER);

        let err = CrackedError::IO(StdError::new(ErrorKind::Other, "test"));
        assert_eq!(format!("{}", err), "test");
    }

    #[test]
    fn test_partial_eq() {
        let err = CrackedError::NoGuildCached;
        assert_eq!(err, CrackedError::NoGuildCached);

        let err = CrackedError::NoGuildId;
        assert_eq!(err, CrackedError::NoGuildId);

        let err = CrackedError::NoGuildForChannelId(ChannelId::new(1));
        assert_eq!(err, CrackedError::NoGuildForChannelId(ChannelId::new(1)));

        let err = CrackedError::NoGuildSettings;
        assert_eq!(err, CrackedError::NoGuildSettings);

        let err = CrackedError::NoLogChannel;
        assert_eq!(err, CrackedError::NoLogChannel);

        let err = CrackedError::NoUserAutoplay;
        assert_eq!(err, CrackedError::NoUserAutoplay);

        let err = CrackedError::WrongVoiceChannel;
        assert_eq!(err, CrackedError::WrongVoiceChannel);

        let err = CrackedError::NothingPlaying;
        assert_eq!(err, CrackedError::NothingPlaying);

        let err = CrackedError::PlayListFail;
        assert_eq!(err, CrackedError::PlayListFail);

        let err = CrackedError::ParseTimeFail;
        assert_eq!(err, CrackedError::ParseTimeFail);

        let err = CrackedError::PoisonError(Error::from("test"));
        assert_eq!(err, CrackedError::PoisonError(Error::from("test")));

        let err = CrackedError::TrackFail(Error::from("test"));
        assert_eq!(err, CrackedError::TrackFail(Error::from("test")));

        let err = CrackedError::Serenity(SerenityError::Other("test"));
        assert_eq!(err, CrackedError::Serenity(SerenityError::Other("test")));

        let err = CrackedError::SQLX(sqlx::Error::RowNotFound);
        assert_eq!(err, CrackedError::SQLX(sqlx::Error::RowNotFound));

        let err = CrackedError::SpotifyAuth;
        assert_eq!(err, CrackedError::SpotifyAuth);

        let response1 = reqwest::blocking::get("http://notreallol");
        let response2 = reqwest::blocking::get("http://notreallol");
        let err = CrackedError::Reqwest(response1.unwrap_err());
        assert_eq!(err, CrackedError::Reqwest(response2.unwrap_err()));

        let err = CrackedError::RSpotify(RSpotifyClientError::InvalidToken);
        assert_eq!(
            err,
            CrackedError::RSpotify(RSpotifyClientError::InvalidToken)
        );
    }
}
