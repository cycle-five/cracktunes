//! The rooms settings the dashboard shows and saves. The wire types are typed
//! serde; saving goes through crack-core's one save path.

use crack_core::{
    messaging::messages::{
        DASH_SETTINGS_BAD_GRACE, DASH_SETTINGS_BAD_LIMIT, DASH_SETTINGS_BAD_TEMPLATE,
        DASH_SETTINGS_NO_DATABASE, DASH_SETTINGS_NO_HUB, DASH_SETTINGS_UNKNOWN_CHANNEL,
        MODLOG_NO_CHANNEL, MODLOG_NO_DATABASE,
    },
    modlog::settings::ModLogSettingsError,
    rooms::settings::SettingsError,
};
use serde::{Deserialize, Serialize};

/// A voice channel the hub can be.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelOption {
    pub id: String,
    pub name: String,
}

/// What the settings page shows. Ids are strings: snowflakes exceed what a
/// JavaScript number holds exactly.
#[derive(Debug, Clone, Serialize)]
pub struct RoomSettingsView {
    pub enabled: bool,
    pub hub: Option<String>,
    pub name_template: String,
    pub default_limit: u8,
    pub empty_grace_secs: u16,
    pub voice_channels: Vec<ChannelOption>,
    /// Rendered permission findings; `None` when they could not be checked
    /// (or there is no hub yet).
    pub findings: Option<Vec<String>>,
}

/// What the page posts.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RoomSettingsForm {
    pub enabled: bool,
    pub hub: Option<String>,
    pub name_template: String,
    pub default_limit: u8,
    pub empty_grace_secs: u16,
}

/// Why a save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveRefused {
    Invalid(SettingsError),
    /// The hub is not a voice channel of this server.
    UnknownChannel,
    /// The database refused the write.
    Failed,
    /// The server's stored settings have not been read yet, so a save
    /// would write defaults over them.
    NotLoaded,
}

/// The reason shown for a refused settings value.
pub fn invalid_message(e: SettingsError) -> &'static str {
    match e {
        SettingsError::NoDatabase => DASH_SETTINGS_NO_DATABASE,
        SettingsError::NoHub => DASH_SETTINGS_NO_HUB,
        SettingsError::BadTemplate => DASH_SETTINGS_BAD_TEMPLATE,
        SettingsError::BadLimit => DASH_SETTINGS_BAD_LIMIT,
        SettingsError::BadGrace => DASH_SETTINGS_BAD_GRACE,
    }
}

/// What the page shows for the mod log. Ids are strings, as for rooms.
#[derive(Debug, Clone, Serialize)]
pub struct ModLogSettingsView {
    pub enabled: bool,
    pub channel: Option<String>,
    pub text_channels: Vec<ChannelOption>,
    /// Rendered permission findings; `None` when they could not be checked
    /// (or there is no channel yet).
    pub findings: Option<Vec<String>>,
    /// "Anyone who can see #channel can read the log."; `None` without a
    /// channel the cache knows.
    pub visibility: Option<String>,
}

/// What the page posts for the mod log.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModLogSettingsForm {
    pub enabled: bool,
    pub channel: Option<String>,
}

/// Why a mod log save changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModLogSaveRefused {
    Invalid(ModLogSettingsError),
    /// The channel is not a text channel of this server.
    UnknownChannel,
    /// The database refused the write.
    Failed,
    /// The stored settings have not been read yet.
    NotLoaded,
}

/// The reason shown for a refused mod log value.
pub fn modlog_invalid_message(e: ModLogSettingsError) -> &'static str {
    match e {
        ModLogSettingsError::NoDatabase => MODLOG_NO_DATABASE,
        ModLogSettingsError::NoChannel => MODLOG_NO_CHANNEL,
    }
}

pub const UNKNOWN_CHANNEL: &str = DASH_SETTINGS_UNKNOWN_CHANNEL;
