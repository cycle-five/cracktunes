use self::serenity::{model::mention::Mention, SerenityError};
use crate::messaging::messages::{
    FAIL_ANOTHER_CHANNEL, FAIL_AUTHOR_DISCONNECTED, FAIL_AUTHOR_NOT_FOUND,
    FAIL_NO_VOICE_CONNECTION, FAIL_PARSE_TIME, FAIL_WRONG_CHANNEL, NOTHING_IS_PLAYING,
    PLAYLIST_FAILED, QUEUE_IS_EMPTY, TRACK_INAPPROPRIATE, TRACK_NOT_FOUND,
};
use crate::Error;
use poise::serenity_prelude as serenity;
use rspotify::ClientError as RSpotifyClientError;
use songbird::input::error::{DcaError, Error as InputError};
use std::fmt;
use std::fmt::{Debug, Display};

/// A common error enum returned by most of the crate's functions within a [`Result`].
#[derive(Debug)]
pub enum CrackedError {
    Other(&'static str),
    QueueEmpty,
    NotInRange(&'static str, isize, isize, isize),
    NotConnected,
    AuthorDisconnected(Mention),
    WrongVoiceChannel,
    AuthorNotFound,
    NothingPlaying,
    PlayListFail,
    ParseTimeFail,
    TrackFail(InputError),
    AlreadyConnected(Mention),
    Serenity(SerenityError),
    Shuttle(shuttle_runtime::Error),
    RSpotify(RSpotifyClientError),
    IO(std::io::Error),
    Serde(serde_json::Error),
    Songbird(songbird::input::error::Error),
    Poise(Error),
}

/// `CrackedError` implements the [`Debug`] and [`Display`] traits
/// meaning it implements the [`std::error::Error`] trait.
/// This just makes it explicit.
impl std::error::Error for CrackedError {}

unsafe impl Send for CrackedError {}

unsafe impl Sync for CrackedError {}

/// Implementation of the [`Display`] trait for the [`CrackedError`] enum.
/// Errors are formatted with this and then sent as responses to the interaction.
impl Display for CrackedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(msg) => f.write_str(msg),
            Self::QueueEmpty => f.write_str(QUEUE_IS_EMPTY),
            Self::NotInRange(param, value, lower, upper) => f.write_str(&format!(
                "`{param}` should be between {lower} and {upper} but was {value}"
            )),
            Self::NotConnected => f.write_str(FAIL_NO_VOICE_CONNECTION),
            Self::AuthorDisconnected(mention) => {
                f.write_fmt(format_args!("{} {}", FAIL_AUTHOR_DISCONNECTED, mention))
            }
            Self::WrongVoiceChannel => f.write_str(FAIL_WRONG_CHANNEL),
            Self::AuthorNotFound => f.write_str(FAIL_AUTHOR_NOT_FOUND),
            Self::AlreadyConnected(mention) => {
                f.write_fmt(format_args!("{} {}", FAIL_ANOTHER_CHANNEL, mention))
            }
            Self::NothingPlaying => f.write_str(NOTHING_IS_PLAYING),
            Self::PlayListFail => f.write_str(PLAYLIST_FAILED),
            Self::ParseTimeFail => f.write_str(FAIL_PARSE_TIME),
            Self::TrackFail(err) => match err {
                InputError::Json {
                    error: _,
                    parsed_text,
                } => {
                    if parsed_text.contains("Sign in to confirm your age") {
                        f.write_str(TRACK_INAPPROPRIATE)
                    } else {
                        f.write_str(TRACK_NOT_FOUND)
                    }
                }
                _ => f.write_str(&format!("{err}")),
            },
            Self::Serenity(err) => f.write_str(&format!("{err}")),
            Self::Shuttle(err) => f.write_str(&format!("{err}")),
            Self::RSpotify(err) => f.write_str(&format!("{err}")),
            Self::IO(err) => f.write_str(&format!("{err}")),
            Self::Serde(err) => f.write_str(&format!("{err}")),
            Self::Songbird(err) => f.write_str(&format!("{err}")),
            Self::Poise(err) => f.write_str(&format!("{err}")),
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
            }
            (Self::AuthorDisconnected(l0), Self::AuthorDisconnected(r0)) => {
                l0.to_string() == r0.to_string()
            }
            (Self::AlreadyConnected(l0), Self::AlreadyConnected(r0)) => {
                l0.to_string() == r0.to_string()
            }
            (Self::Serenity(l0), Self::Serenity(r0)) => format!("{l0:?}") == format!("{r0:?}"),
            _ => core::mem::discriminant(self) == core::mem::discriminant(other),
        }
    }
}

impl From<DcaError> for CrackedError {
    fn from(err: songbird::input::error::DcaError) -> Self {
        Self::Poise(Box::new(err))
    }
}

use audiopus::error::Error as AudiopusError;
impl From<AudiopusError> for CrackedError {
    fn from(err: AudiopusError) -> Self {
        Self::Poise(Box::new(err))
    }
}

impl From<CrackedError> for songbird::input::error::Error {
    fn from(val: CrackedError) -> songbird::input::error::Error {
        match val {
            CrackedError::Poise(_) => songbird::input::error::Error::Metadata,
            CrackedError::Serde(err) => songbird::input::error::Error::Json {
                error: err,
                parsed_text: "".to_string(),
            },
            CrackedError::IO(err) => songbird::input::error::Error::Io(err),
            CrackedError::Songbird(err) => err,
            CrackedError::TrackFail(err) => err,
            CrackedError::RSpotify(_) => songbird::input::error::Error::Metadata,
            CrackedError::Serenity(_) => songbird::input::error::Error::Metadata,
            CrackedError::AlreadyConnected(_) => songbird::input::error::Error::Metadata,
            CrackedError::NotConnected => songbird::input::error::Error::Metadata,
            CrackedError::Other(_) => songbird::input::error::Error::Metadata,
            CrackedError::QueueEmpty => songbird::input::error::Error::Metadata,
            CrackedError::NotInRange(_, _, _, _) => songbird::input::error::Error::Metadata,
            CrackedError::AuthorDisconnected(_) => songbird::input::error::Error::Metadata,
            CrackedError::WrongVoiceChannel => songbird::input::error::Error::Metadata,
            CrackedError::AuthorNotFound => songbird::input::error::Error::Metadata,
            _ => songbird::input::error::Error::Metadata,
        }
    }
}

impl From<songbird::input::error::Error> for CrackedError {
    fn from(err: songbird::input::error::Error) -> Self {
        match err {
            songbird::input::error::Error::Json {
                error,
                parsed_text: _,
            } => Self::Poise(error.into()),
            songbird::input::error::Error::Io(err) => Self::IO(err),
            songbird::input::error::Error::Metadata => Self::Other("Metadata"),
            songbird::input::error::Error::Stdout => Self::Other("Stdout"),
            songbird::input::error::Error::Dca(err) => Self::Poise(Box::new(err)),
            songbird::input::error::Error::Streams => Self::Other("Streams"),
            songbird::input::error::Error::Streamcatcher(err) => Self::Poise(Box::new(err)),
            _ => Self::Other("FAILED"),
        }
    }
}

impl From<Error> for CrackedError {
    fn from(err: Error) -> Self {
        CrackedError::Poise(err)
    }
}

// impl From<songbird::input::error::Error> for CrackedError {
//     fn from(err: songbird::input::error::Error) -> CrackedError {
//         CrackedError::IO(err)
//     }
// }

// impl Into<songbird::input::error::Error> for Box<dyn std::error::Error + Send + Sync> {
//     fn into(self) -> songbird::input::error::Error {
//         Into<StdError>(self).into()
//     }
// }

/// Provides an implementation to convert a [`std::io::Error`] to a [`CrackedError`].
impl From<std::io::Error> for CrackedError {
    fn from(err: std::io::Error) -> Self {
        Self::IO(err)
    }
}

/// Provides an implementation to convert a [`shuttle_serenity::Error`] to a [`CrackedError`].
// impl From<shuttle_serenity::Error> for CrackedError {
//     fn from(err: shuttle_serenity::Error) -> Self {
//         Self::Shuttle(err)
//     }
// }

// /// Provides an implementation to convert a [`CrackedError`] to a [`shuttle_serenity::Error`].
// impl From<CrackedError> for shuttle_runtime::Error {
//     fn from(val: CrackedError) -> shuttle_runtime::Error {
//         val.into()
//     }
// }

impl From<anyhow::Error> for CrackedError {
    fn from(err: anyhow::Error) -> Self {
        Self::Poise(err.into())
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
            }
            SerenityError::Other(msg) => Self::Other(msg),
            _ => Self::Serenity(err),
        }
    }
}

/// Provides an implementation to convert a rspotify [`ClientError`] to a [`CrackedError`].
impl From<RSpotifyClientError> for CrackedError {
    fn from(err: RSpotifyClientError) -> CrackedError {
        CrackedError::RSpotify(err)
    }
}

// pub trait NewTrait: DerefMut + Deref<Target = serenity::Client> + NewTraitClone {}

// pub trait NewTraitClone {
//     fn clone_box(&self) -> Box<dyn NewTrait>;
// }

// impl<T> NewTraitClone for T
// where
//     T: 'static + NewTrait + Clone,
// {
//     fn clone_box(&self) -> Box<dyn NewTrait> {
//         Box::new(self.clone())
//     }
// }

// impl Clone for Box<dyn NewTraitClone> {
//     fn clone(&self) -> Box<dyn NewTraitClone> {
//         self.clone_box().into()
//     }
// }

// pub struct NewTraitCloneBox(Box<dyn NewTraitClone>);

// impl From<Box<dyn NewTrait<Target = serenity::Client>>> for NewTraitCloneBox {
//     fn from(item: Box<dyn NewTrait<Target = serenity::Client>>) -> Self {
//         item.into()
//     }
// }

// impl From<Box<dyn NewTrait<Target = serenity::Client>>> for Box<dyn NewTraitClone> {
//     fn from(item: Box<dyn NewTrait<Target = serenity::Client>>) -> Self {
//         item.clone_box().into()
//     }
// }

// impl From<Box<dyn NewTrait<Target = serenity::Client>>> for SerenityService {
//     fn from(router: Box<dyn NewTrait<Target = serenity::Client>>) -> Self {
//         <Box<dyn NewTrait<Target = serenity::Client>> as Into<SerenityService>>::into(router).into()
//     }
// }

/// The return type that should be returned from the [shuttle_runtime::main] function.
// pub type ShuttleSerenity = Result<SerenityService, Error>;

// impl From<serenity::Client> for SerenityService {
//     fn from(item: serenity::Client) -> Self {
//         SerenityService(item)
//     }
// }

// impl From<Box<serenity::Client>> for SerenityService {
//     fn from(item: Box<serenity::Client>) -> Self {
//         SerenityService(*item)
//     }
// }

// impl From<dyn NewTrait + 'a> for CrackedError {
//     fn from(err: impl DerefMut + Deref<Target = serenity::Client> + 'a) -> Self {
//         Self::Poise(Box::new(err))
//     }
// }

// impl From<Error> for CrackedError {
//     fn from(err: Error) -> Self {
//         Self::Poise(err)
//     }
// }

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

// impl<T> From<Result<T, songbird::input::error::Error>> for Result<T, CrackedError> {
//     fn from(err: Result<T, songbird::input::error::Error>) -> Self {
//         match err {
//             Ok(x) => Ok(x),
//             Err(err) => Self::Songbird(err),
//         }
//     }
// }

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
