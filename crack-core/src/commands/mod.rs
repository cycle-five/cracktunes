pub mod admin;
#[cfg(feature = "crack-gpt")]
pub mod chatgpt;
pub mod music;
// #[cfg(feature = "crack-osint")]
// pub mod osint;
pub mod ping;
pub mod playlist;
pub mod settings;
pub mod version;

pub use admin::*;
#[cfg(feature = "crack-gpt")]
pub use chatgpt::*;
pub use music::*;
// #[cfg(feature = "crack-osint")]
// pub use osint::*;
pub use ping::*;
pub use playlist::playlist;
pub use settings::*;
pub use version::*;
