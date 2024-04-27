pub mod admin;
#[cfg(feature = "crack-gpt")]
pub mod chatgpt;
pub mod music;
pub mod music_utils;
#[cfg(feature = "crack-osint")]
pub mod osint;
pub mod ping;
pub mod playlist;
pub mod settings;
pub mod version;

pub use admin::*;
#[cfg(feature = "crack-gpt")]
pub use chatgpt::*;
pub use music::*;
pub use music_utils::*;
#[cfg(feature = "crack-osint")]
pub use osint::*;
pub use ping::*;
pub use playlist::playlist;
use serenity::all::Message;
pub use settings::*;
pub use version::*;

use crate::{errors::CrackedError, Error};

pub type MessageResult = Result<Message, CrackedError>;
pub type EmptyResult = Result<(), Error>;

pub trait ConvertToEmptyResult {
    fn convert(self) -> EmptyResult;
}

impl ConvertToEmptyResult for MessageResult {
    fn convert(self) -> EmptyResult {
        self.map(|_| ()).map_err(|e| e.into())
    }
}
