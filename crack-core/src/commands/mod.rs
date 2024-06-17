pub mod admin;
#[cfg(feature = "crack-bf")]
pub mod bf;
#[cfg(feature = "crack-gpt")]
pub mod chatgpt;
pub mod help;
pub mod music;
pub mod music_utils;
#[cfg(feature = "crack-osint")]
pub mod osint;
pub mod permissions;
pub mod playlist;
pub mod settings;
pub mod utility;

pub use admin::*;
#[cfg(feature = "crack-bf")]
pub use bf::*;
#[cfg(feature = "crack-gpt")]
pub use chatgpt::*;
pub use help::sub_help;
pub use music::*;
pub use music_utils::*;
#[cfg(feature = "crack-osint")]
pub use osint::*;
pub use permissions::*;
pub use settings::*;
pub use utility::*;

pub use crate::errors::CrackedError;
use serenity::all::Message;

pub type MessageResult = Result<Message, CrackedError>;
pub type EmptyResult = Result<(), crate::Error>;

pub trait ConvertToEmptyResult {
    fn convert(self) -> EmptyResult;
}

impl ConvertToEmptyResult for MessageResult {
    fn convert(self) -> EmptyResult {
        self.map(|_| ()).map_err(|e| e.into())
    }
}

/// Return all the commands that are available in the bot.
pub fn all_commands() -> Vec<crate::Command> {
    vec![
        #[cfg(feature = "crack-bf")]
        bf(),
        #[cfg(feature = "crack-osint")]
        osint(),
        #[cfg(feature = "crack-gpt")]
        chat(),
    ]
    .into_iter()
    .chain(music::music_commands())
    .chain(playlist::commands())
    .chain(utility::utility_commands())
    .chain(help::help_commands())
    .chain(admin::admin_commands())
    .chain(settings::settings_commands())
    .collect()
}

pub fn all_command_names() -> Vec<String> {
    all_commands().into_iter().map(|c| c.name).collect()
}
