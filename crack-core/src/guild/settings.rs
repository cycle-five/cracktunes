use self::serenity::model::id::GuildId;
use crate::db::{GuildEntity, WelcomeSettingsRead};
use crate::errors::CrackedError;
use crate::CrackedResult;
use ::serenity::small_fixed_array::FixedString;
use crack_model::{RoleRef, RoomRef, UserRef};
use crack_types::discord::{as_snowflake_opt, from_snowflake, snowflake};
use lazy_static::lazy_static;
use poise::serenity_prelude::{self as serenity};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt::{Display, Formatter};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::str::FromStr;
use std::sync::{atomic, Arc};
use std::{
    collections::{HashMap, HashSet},
    env,
};
use typemap_rev::TypeMapKey;

pub(crate) const DEFAULT_ACTIVITY: &str = "/play";
pub(crate) const DEFAULT_LOG_PREFIX: &str = "data/logs";
pub(crate) const DEFAULT_ALLOW_ALL_DOMAINS: bool = true;
pub(crate) const DEFAULT_SETTINGS_PATH: &str = "data/settings";
pub(crate) const YOUTUBE_WATCH_URL: &str = "https://www.youtube.com/watch?v=";
pub(crate) const VIDEO_WATCH_URL: &str = YOUTUBE_WATCH_URL;
pub(crate) const DEFAULT_ALLOWED_DOMAINS: [&str; 1] = ["youtube.com"];
pub(crate) const DEFAULT_VOLUME_LEVEL: f32 = 1.0;
pub(crate) const DEFAULT_VIDEO_STATUS_POLL_INTERVAL: u64 = 120;
pub(crate) const DEFAULT_PREFIX: &str = "r!";
pub(crate) const DEFAULT_DB_URL: &str =
    "postgresql://postgres:mysecretpassword@localhost:5432/postgres";
pub(crate) const DEFAULT_IDLE_TIMEOUT: u32 = 10 * 60;
pub(crate) const DEFAULT_LYRICS_PAGE_SIZE: usize = 1024;
pub(crate) const DEFAULT_PREMIUM: bool = false;
pub(crate) const ADDITIONAL_PREFIXES: [&str; 8] = [
    "hey bot,", "hey bot", "bot,", "bot", "!play", "!music", "!youtube", "!yt", //, "m/", "M/",
];

lazy_static! {
    static ref SETTINGS_PATH: String =
        env::var("SETTINGS_PATH").unwrap_or(DEFAULT_SETTINGS_PATH.to_string());
    pub static ref LOG_PREFIX: String =
        env::var("LOG_PREFIX").unwrap_or(DEFAULT_LOG_PREFIX.to_string());
}

/// Get the settings path, global.
pub fn get_settings_path() -> String {
    SETTINGS_PATH.to_string()
}

/// Get the log prefix, global.
pub fn get_log_prefix() -> String {
    LOG_PREFIX.to_string()
}

#[derive(Default, Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct LogSettings {
    // TODO: Decide if I want to have separate raw events and all log channels.
    #[serde(default, with = "as_snowflake_opt")]
    pub all_log_channel: Option<RoomRef>,
    #[serde(default, with = "as_snowflake_opt")]
    pub raw_event_log_channel: Option<RoomRef>,
    // TODO: Decide on what level of granularity I want for logging options.
    // Also should they be able to overlap?
    #[serde(default, with = "as_snowflake_opt")]
    pub server_log_channel: Option<RoomRef>,
    #[serde(default, with = "as_snowflake_opt")]
    pub member_log_channel: Option<RoomRef>,
    #[serde(default, with = "as_snowflake_opt")]
    pub join_leave_log_channel: Option<RoomRef>,
    #[serde(default, with = "as_snowflake_opt")]
    pub voice_log_channel: Option<RoomRef>,
}

/// Convert one stored id. A value that is not a snowflake is dropped with a
/// warning rather than panicking a conversion that cannot fail.
pub(crate) fn stored_ref<K: crack_model::Kind>(
    column: &'static str,
    raw: Option<i64>,
) -> Option<crack_model::Ref<K>> {
    let raw = raw?;
    match u64::try_from(raw)
        .ok()
        .and_then(|n| from_snowflake::<K>(n).ok())
    {
        Some(r) => Some(r),
        None => {
            tracing::warn!(column, raw, "dropped a stored id that is not a snowflake");
            None
        },
    }
}

/// The `BIGINT` to store for a ref. A ref that is not a Discord snowflake
/// cannot be stored in these columns and is refused, never truncated.
pub(crate) fn column_i64<K: crack_model::Kind>(
    r: &crack_model::Ref<K>,
) -> Result<i64, CrackedError> {
    snowflake(r)
        .map(|n| n as i64)
        .map_err(|_| CrackedError::Other("a stored id must be a Discord snowflake"))
}

/// The ref for a stored `BIGINT` that must be there. A value that is not a
/// snowflake is an error, because the row it sits in cannot be used without it.
pub(crate) fn ref_from_column<K: crack_model::Kind>(
    raw: i64,
) -> Result<crack_model::Ref<K>, CrackedError> {
    u64::try_from(raw)
        .ok()
        .and_then(|n| from_snowflake::<K>(n).ok())
        .ok_or(CrackedError::Other(
            "a stored id is not a Discord snowflake",
        ))
}

/// [`column_i64`] for an optional column.
pub(crate) fn column_opt<K: crack_model::Kind>(
    r: &Option<crack_model::Ref<K>>,
) -> Result<Option<i64>, CrackedError> {
    r.as_ref().map(column_i64).transpose()
}

impl From<crate::db::LogSettingsRead> for LogSettings {
    fn from(settings_db: crate::db::LogSettingsRead) -> Self {
        LogSettings {
            all_log_channel: stored_ref(
                "log_settings.all_log_channel",
                settings_db.all_log_channel,
            ),
            raw_event_log_channel: stored_ref(
                "log_settings.raw_event_log_channel",
                settings_db.raw_event_log_channel,
            ),
            server_log_channel: stored_ref(
                "log_settings.server_log_channel",
                settings_db.server_log_channel,
            ),
            member_log_channel: stored_ref(
                "log_settings.member_log_channel",
                settings_db.member_log_channel,
            ),
            join_leave_log_channel: stored_ref(
                "log_settings.join_leave_log_channel",
                settings_db.join_leave_log_channel,
            ),
            voice_log_channel: stored_ref(
                "log_settings.voice_log_channel",
                settings_db.voice_log_channel,
            ),
        }
    }
}

impl LogSettings {
    pub fn get_all_log_channel(&self) -> Option<RoomRef> {
        self.all_log_channel.clone()
    }

    pub fn get_server_log_channel(&self) -> Option<RoomRef> {
        self.server_log_channel.clone()
    }

    pub fn get_join_leave_log_channel(&self) -> Option<RoomRef> {
        self.join_leave_log_channel.clone()
    }

    pub fn get_member_log_channel(&self) -> Option<RoomRef> {
        self.member_log_channel.clone()
    }

    pub fn get_voice_log_channel(&self) -> Option<RoomRef> {
        self.voice_log_channel.clone()
    }

    pub fn set_all_log_channel(&mut self, channel: RoomRef) -> &mut Self {
        self.all_log_channel = Some(channel);
        self
    }

    pub fn set_server_log_channel(&mut self, channel: RoomRef) -> &mut Self {
        self.server_log_channel = Some(channel);
        self
    }

    pub fn set_join_leave_log_channel(&mut self, channel: RoomRef) -> &mut Self {
        self.join_leave_log_channel = Some(channel);
        self
    }

    pub fn set_member_log_channel(&mut self, channel: RoomRef) -> &mut Self {
        self.member_log_channel = Some(channel);
        self
    }

    pub fn set_voice_log_channel(&mut self, channel: RoomRef) -> &mut Self {
        self.voice_log_channel = Some(channel);
        self
    }

    /// Write the log settings to the database.
    pub async fn save(&self, pool: &PgPool, guild_id: u64) -> Result<(), CrackedError> {
        crate::db::GuildEntity::write_log_settings(pool, guild_id as i64, self).await
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
pub struct WelcomeSettings {
    #[serde(default, with = "as_snowflake_opt")]
    pub channel_id: Option<RoomRef>,
    pub message: Option<String>,
    #[serde(default, with = "as_snowflake_opt")]
    pub auto_role: Option<RoleRef>,
    pub password: Option<String>,
}

impl Display for WelcomeSettings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let res = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => format!("Error: {}", e),
        };
        write!(f, "{}", res)
    }
}

impl From<WelcomeSettingsRead> for WelcomeSettings {
    fn from(settings_db: WelcomeSettingsRead) -> Self {
        WelcomeSettings {
            channel_id: stored_ref("welcome_settings.channel_id", settings_db.channel_id),
            message: settings_db.message,
            auto_role: stored_ref("welcome_settings.auto_role", settings_db.auto_role),
            password: None,
        }
    }
}

impl WelcomeSettings {
    /// Create a new empty welcome settings struct.
    pub fn new() -> Self {
        Default::default()
    }

    /// Set the channel id, returning a new WelcomeSettings.
    pub fn with_channel_id(self, channel_id: RoomRef) -> Self {
        Self {
            channel_id: Some(channel_id),
            ..self
        }
    }

    /// Set the message, returning a new WelcomeSettings.
    pub fn with_message(self, message: String) -> Self {
        Self {
            message: Some(message),
            ..self
        }
    }

    /// Set the auto role, returning a new WelcomeSettings.
    pub fn with_auto_role(self, auto_role: RoleRef) -> Self {
        Self {
            auto_role: Some(auto_role),
            ..self
        }
    }

    /// Set the password, returning a new WelcomeSettings.
    pub fn with_password(self, password: String) -> Self {
        Self {
            password: Some(password),
            ..self
        }
    }

    /// Save the welcome settings to the database.
    pub async fn save(&self, pool: &PgPool, guild_id: u64) -> Result<(), CrackedError> {
        crate::db::GuildEntity::write_welcome_settings(pool, guild_id as i64, self).await
    }
}

use super::permissions::GenericPermissionSettings;

/// Where a guild's in-memory settings came from, and therefore whether writing
/// them back to Postgres is safe.
///
/// 🔑 `Fallback` is the `Default` **deliberately**. The shutdown handler writes
/// every in-memory guild back to the database, and `on_guild_create` falls back
/// to `GuildSettings::new()` defaults when the load fails. Without this tag a
/// transient read failure at boot becomes a permanent write at shutdown --
/// defaults silently replacing a guild's stored settings, with no backup to
/// restore from. Defaulting to `Fallback` means anything we did not explicitly
/// load from Postgres is never written back.
///
/// 🪤 **Mutations must not reset this.** `set_volume` and friends use
/// `and_modify` / `..self`, which leave the tag alone -- the behaviour we want.
/// A future mutation site that *replaces* the map entry with a fresh
/// `GuildSettings::new()` would silently re-arm the bug.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Defaults, built in memory. NOT safe to write back.
    #[default]
    Fallback,
    /// Round-tripped through Postgres, so the row exists and a write updates
    /// a row we actually read. Safe to write back.
    Database,
}

// TODO
//#[derive(Debug, Clone, Serialize, PartialEq)]
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct GuildSettings {
    pub guild_id: GuildId,
    pub guild_name: FixedString,
    pub prefix: String,
    // A vc token per guild.
    #[serde(default = "premium_default")]
    pub premium: bool,
    // Settings for different categories of commands.
    pub command_settings: HashMap<String, GenericPermissionSettings>,
    #[serde(default = "default_false")]
    pub autopause: bool,
    #[serde(default = "default_true")]
    pub autoplay: bool,
    #[serde(default = "default_true")]
    pub reply_with_embed: bool,
    /// Whether /play, /skip and /nowplaying reply ephemerally.
    #[serde(default = "default_false")]
    pub ephemeral_replies: bool,
    /// Whether a dashboard control or a now-playing button press posts an
    /// echo line in the channel.
    #[serde(default = "default_true")]
    pub control_echoes: bool,
    /// Whether the now-playing status message carries its buttons. Off is
    /// how a server that restricted the music commands under Integrations
    /// keeps them from being reached by a button.
    #[serde(default = "default_true")]
    pub now_playing_buttons: bool,
    #[serde(default = "allow_all_domains_default")]
    pub allow_all_domains: Option<bool>,
    pub allowed_domains: HashSet<String>,
    pub banned_domains: HashSet<String>,
    pub ignored_channels: HashSet<u64>,
    #[serde(default = "volume_default")]
    pub old_volume: f32,
    #[serde(default = "volume_default")]
    pub volume: f32,
    /// Unused since v0.30.1: CrackTunes always joins self-deafened. The column
    /// stays so older images (which write it) can still roll back.
    pub self_deafen: bool,
    pub timeout: u32,
    pub welcome_settings: Option<WelcomeSettings>,
    pub log_settings: Option<LogSettings>,
    #[serde(default = "additional_prefixes_default")]
    pub additional_prefixes: Vec<String>,
    /// Not a column and not on the wire -- see [`Provenance`].
    #[serde(skip)]
    pub provenance: Provenance,
}

/// 🔑 Hand-written to exclude `provenance`, which is bookkeeping about where a
/// value came from rather than part of a guild's settings. Two guilds with
/// identical settings are equal regardless of how they were loaded, and the
/// existing equality tests should not care.
impl PartialEq for GuildSettings {
    fn eq(&self, other: &Self) -> bool {
        self.guild_id == other.guild_id
            && self.guild_name == other.guild_name
            && self.prefix == other.prefix
            && self.premium == other.premium
            && self.command_settings == other.command_settings
            && self.autopause == other.autopause
            && self.autoplay == other.autoplay
            && self.reply_with_embed == other.reply_with_embed
            && self.ephemeral_replies == other.ephemeral_replies
            && self.control_echoes == other.control_echoes
            && self.now_playing_buttons == other.now_playing_buttons
            && self.allow_all_domains == other.allow_all_domains
            && self.allowed_domains == other.allowed_domains
            && self.banned_domains == other.banned_domains
            && self.ignored_channels == other.ignored_channels
            && self.old_volume == other.old_volume
            && self.volume == other.volume
            && self.self_deafen == other.self_deafen
            && self.timeout == other.timeout
            && self.welcome_settings == other.welcome_settings
            && self.log_settings == other.log_settings
            && self.additional_prefixes == other.additional_prefixes
    }
}

/// Default value function for serialization that is false.
fn default_false() -> bool {
    false
}

/// Default value function for serialization that is true.
fn default_true() -> bool {
    true
}

/// Default value function for serialization for allow all domains.
fn allow_all_domains_default() -> Option<bool> {
    Some(DEFAULT_ALLOW_ALL_DOMAINS)
}

/// Default value function for serialization for additional prefixes.
fn additional_prefixes_default() -> Vec<String> {
    Vec::<String>::new()
}

/// Default value function for serialization for volume.
fn volume_default() -> f32 {
    DEFAULT_VOLUME_LEVEL
}

/// Default value function for serialization for premium status.
fn premium_default() -> bool {
    DEFAULT_PREMIUM
}

impl Default for GuildSettings {
    fn default() -> Self {
        GuildSettings::new(GuildId::default(), Some("r!"), None)
    }
}

impl Display for GuildSettings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let res = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => format!("Error: {}", e),
        };
        write!(f, "{}", res)
    }
}

use crate::db::GuildSettingsRead;

impl From<GuildSettingsRead> for GuildSettings {
    fn from(settings_db: GuildSettingsRead) -> Self {
        let mut settings = GuildSettings::new(
            GuildId::new(settings_db.guild_id as u64),
            Some(&settings_db.prefix),
            Some(
                FixedString::from_str(&settings_db.guild_name)
                    .expect("Failed to convert guild name"),
            ),
        );
        settings.premium = settings_db.premium;
        settings.autopause = settings_db.autopause;
        settings.ephemeral_replies = settings_db.ephemeral_replies;
        settings.control_echoes = settings_db.control_echoes;
        settings.now_playing_buttons = settings_db.now_playing_buttons;
        settings.autoplay = true; //settings_db.autoplay;
        settings.allow_all_domains = Some(settings_db.allow_all_domains);
        settings.allowed_domains = settings_db.allowed_domains.into_iter().collect();
        settings.banned_domains = settings_db.banned_domains.into_iter().collect();

        settings.ignored_channels = settings_db
            .ignored_channels
            .into_iter()
            .map(|x| x as u64)
            .collect();
        settings.old_volume = settings_db.volume as f32;
        settings.volume = settings_db.volume as f32;
        settings.self_deafen = settings_db.self_deafen;
        settings.timeout = settings_db.timeout_seconds.unwrap_or(0) as u32;
        settings.welcome_settings = None; // FIXME
        settings.log_settings = None; //FIXME
        settings.additional_prefixes = settings_db.additional_prefixes;
        settings
    }
}

impl GuildSettings {
    pub fn new(
        guild_id: GuildId,
        prefix: Option<&str>,
        guild_name: Option<FixedString>,
    ) -> GuildSettings {
        let allowed_domains: HashSet<String> = DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|d| d.to_string())
            .collect();

        let my_prefix = match prefix {
            Some(p) => p.to_string(),
            None => DEFAULT_PREFIX.to_string(),
        };

        let guild_name = guild_name.unwrap_or_default();
        let asdf: Vec<u64> = vec![1165246445654388746];
        GuildSettings {
            guild_id,
            guild_name,
            prefix: my_prefix.clone(),
            premium: DEFAULT_PREMIUM,
            command_settings: HashMap::new(),
            autopause: false,
            autoplay: true,
            reply_with_embed: true,
            ephemeral_replies: false,
            control_echoes: true,
            now_playing_buttons: true,
            allow_all_domains: Some(DEFAULT_ALLOW_ALL_DOMAINS),
            allowed_domains,
            banned_domains: HashSet::new(),
            ignored_channels: asdf.into_iter().collect(),
            old_volume: DEFAULT_VOLUME_LEVEL,
            volume: DEFAULT_VOLUME_LEVEL,
            self_deafen: true,
            timeout: DEFAULT_IDLE_TIMEOUT,
            welcome_settings: None,
            log_settings: None,
            additional_prefixes: Vec::new(),
            provenance: Provenance::Fallback,
        }
    }

    /// Create a new GuildSettings with the given guild id.
    pub fn new_gid(guild_id: GuildId) -> GuildSettings {
        GuildSettings::new(guild_id, None, None)
    }

    /// Get the prefix in uppercase.
    pub fn get_prefix_up(&self) -> String {
        self.prefix.to_ascii_uppercase()
    }

    /// Load the guild settings from the database or create them if they don't exist.
    pub async fn load_or_create(&mut self, pool: &PgPool) -> Result<GuildSettings, CrackedError> {
        let guild_id = self.guild_id.get() as i64;
        let name = self.guild_name.clone();
        let prefix = self.prefix.clone();
        let (_guild, settings) =
            crate::db::GuildEntity::get_or_create(pool, guild_id, name, prefix).await?;
        Ok(settings)
    }

    pub async fn load_if_exists(&mut self, pool: &PgPool) -> Result<GuildSettings, CrackedError> {
        let guild_id = self.guild_id.get() as i64;
        let name = self.guild_name.clone();
        let prefix = self.prefix.clone();
        let (guild, _guild_settings) =
            crate::db::GuildEntity::get_or_create(pool, guild_id, name, prefix).await?;
        let settings = guild.get_settings(pool).await?;
        Ok(settings)
    }

    pub async fn save(&self, pool: &PgPool) -> Result<(), CrackedError> {
        let guild_id = self.guild_id.get() as i64;
        let guild_name = self.guild_name.clone();
        let prefix = self.prefix.clone();
        let (_guild, _guild_settings) = crate::db::GuildEntity::get_or_create(
            pool,
            guild_id,
            guild_name.clone(),
            prefix.clone(),
        )
        .await?;
        GuildEntity::write_settings(pool, self).await?;
        Ok(())
    }

    /// Save the guild settings to a JSON file. Unused?
    pub fn save_json(&self) -> Result<(), CrackedError> {
        create_dir_all(SETTINGS_PATH.as_str())?;
        let path = format!(
            "{}/{}-{}.json",
            SETTINGS_PATH.as_str(),
            self.get_guild_name(),
            self.guild_id
        );
        tracing::warn!("path: {:?}", path);

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)?;
        tracing::warn!("file: {:?}", file);

        // let mut writer = &BufWriter::new(file);
        let pretty_data = serde_json::to_string_pretty(self)?;
        file.write_all(pretty_data.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    /// Set the premium status, mutating.
    pub fn with_premium(self, premium: bool) -> Self {
        Self { premium, ..self }
    }

    /// Toggle the autopause setting, mutating.
    pub fn toggle_autopause(&mut self) -> &mut Self {
        self.autopause = !self.autopause;
        self
    }

    /// Toggle private (ephemeral) replies for the status-related commands.
    pub fn toggle_ephemeral_replies(&mut self) -> &mut Self {
        self.ephemeral_replies = !self.ephemeral_replies;
        self
    }

    /// Toggle whether controls (dashboard, buttons) echo in the channel.
    pub fn toggle_control_echoes(&mut self) -> &mut Self {
        self.control_echoes = !self.control_echoes;
        self
    }

    /// Toggle whether the now-playing message carries its buttons.
    pub fn toggle_now_playing_buttons(&mut self) -> &mut Self {
        self.now_playing_buttons = !self.now_playing_buttons;
        self
    }

    /// Set the allowed domains, mutating.
    pub fn set_allowed_domains(&mut self, allowed_str: &str) {
        let allowed = allowed_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.allowed_domains = allowed;
    }

    /// Set the banned domains, mutating.
    pub fn set_banned_domains(&mut self, banned_str: &str) {
        let banned = banned_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.banned_domains = banned;
    }

    /// Set the music channel, mutating.
    pub fn set_music_channel(&mut self, channel_id: RoomRef) {
        self.command_settings
            .entry("music".to_string())
            .and_modify(|perms| {
                perms.allowed_channels.clear();
                perms.denied_channels.clear();
                perms.add_allowed_channel(channel_id.clone());
            })
            .or_insert_with(|| {
                let mut perms = GenericPermissionSettings::default();
                perms.add_allowed_channel(channel_id);
                perms
            });
    }

    /// Update the allowed domains.
    pub fn update_domains(&mut self) {
        if !self.allowed_domains.is_empty() && !self.banned_domains.is_empty() {
            self.banned_domains.clear();
        }

        if self.allowed_domains.is_empty() && self.banned_domains.is_empty() {
            self.allowed_domains.insert(String::from("youtube.com"));
            self.banned_domains.clear();
        }
    }

    /// Set the volume level without mutating.
    pub fn with_volume(self, volume: f32) -> Self {
        Self {
            old_volume: volume,
            volume,
            ..self
        }
    }

    /// Record where these settings came from. See [`Provenance`].
    pub fn with_provenance(self, provenance: Provenance) -> Self {
        Self { provenance, ..self }
    }

    /// Whether the shutdown handler may write these settings back to Postgres.
    ///
    /// Only settings that round-tripped through the database may be written:
    /// writing a fallback default over a stored row destroys it.
    pub fn is_persistable(&self) -> bool {
        self.provenance == Provenance::Database
    }

    /// Set the volume level with mutating.
    pub fn set_volume(&mut self, volume: f32) -> &mut Self {
        self.old_volume = volume;
        self.volume = volume;
        self
    }

    /// Set allow all domains, mutating.
    pub fn set_allow_all_domains(&mut self, allow: bool) -> &mut Self {
        self.allow_all_domains = Some(allow);
        self
    }

    /// Set the idle timeout for the bot, mutating.
    pub fn set_timeout(&mut self, timeout: u32) -> &mut Self {
        self.timeout = timeout;
        self
    }

    /// Set the idle timeout for the bot, without mutating.
    pub fn with_timeout(self, timeout: u32) -> Self {
        Self { timeout, ..self }
    }

    /// Set the welcome settings, mutating
    pub fn set_welcome_settings(&mut self, welcome_settings: WelcomeSettings) -> &mut Self {
        self.welcome_settings = Some(welcome_settings);
        self
    }

    /// Set the welcome settings, without mutating.
    pub fn with_welcome_settings(self, welcome_settings: Option<WelcomeSettings>) -> Self {
        Self {
            welcome_settings,
            ..self
        }
    }

    /// Another set welcome settings
    pub fn set_welcome_settings2(
        &mut self,
        channel_id: RoomRef,
        auto_role: Option<RoleRef>,
        message: &str,
    ) -> &mut Self {
        self.welcome_settings = Some(WelcomeSettings {
            channel_id: Some(channel_id),
            message: Some(message.to_string()),
            auto_role,
            ..Default::default()
        });
        self
    }

    /// And a third.
    pub fn set_welcome_settings3(&mut self, channel_id: RoomRef, message: String) -> &mut Self {
        self.welcome_settings = Some(WelcomeSettings {
            channel_id: Some(channel_id),
            message: Some(message.to_string()),
            auto_role: self
                .welcome_settings
                .clone()
                .map(|x| x.auto_role)
                .unwrap_or_default(),

            ..Default::default()
        });
        self
    }

    /// Set the auto role,without mutating.
    pub fn with_auto_role(self, auto_role: Option<RoleRef>) -> Self {
        let welcome_settings = if let Some(welcome_settings) = self.welcome_settings {
            WelcomeSettings {
                auto_role,
                ..welcome_settings
            }
        } else {
            WelcomeSettings {
                auto_role,
                ..Default::default()
            }
        };

        Self {
            welcome_settings: Some(welcome_settings.clone()),
            ..self
        }
    }

    /// Set the auto role, mutating.
    pub fn set_auto_role(&mut self, auto_role: Option<RoleRef>) -> &mut Self {
        if let Some(welcome_settings) = &mut self.welcome_settings {
            welcome_settings.auto_role = auto_role;
        } else {
            let welcome_settings = WelcomeSettings {
                auto_role,
                ..Default::default()
            };
            self.welcome_settings = Some(welcome_settings);
        }
        self
    }

    /// Set the log settings, mutating.
    pub fn set_log_settings(
        &mut self,
        all_log_channel: RoomRef,
        join_leave_log_channel: RoomRef,
    ) -> &mut Self {
        self.log_settings = Some(LogSettings {
            all_log_channel: Some(all_log_channel),
            join_leave_log_channel: Some(join_leave_log_channel),
            ..self.log_settings.clone().unwrap_or_default()
        });
        self
    }

    /// Return a copy of the settings with the given log settings.
    pub fn with_log_settings(&self, log_settings: Option<LogSettings>) -> Self {
        Self {
            log_settings,
            ..self.clone()
        }
    }

    /// Set the guild name, mutating.
    pub fn set_prefix(&mut self, prefix: &str) -> &mut Self {
        self.prefix = prefix.to_string();
        self
    }

    /// Set the default additional prefixes, mutating.
    pub fn set_default_additional_prefixes(&mut self) -> &mut Self {
        self.additional_prefixes = ADDITIONAL_PREFIXES
            .to_vec()
            .iter()
            .map(|x| x.to_string())
            .collect();
        self
    }

    /// Set the ignored channels, mutating.
    pub fn set_ignored_channels(&mut self, ignored_channels: HashSet<u64>) -> &mut Self {
        self.ignored_channels = ignored_channels;
        self
    }

    /// Get the guild name.
    pub fn get_guild_name(&self) -> String {
        if self.guild_name.is_empty() {
            self.guild_id.to_string()
        } else {
            self.guild_name.to_string()
        }
    }

    /// Get the prefix.
    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    /// Set the all log channel, with mutating.
    pub fn set_all_log_channel(&mut self, channel_id: RoomRef) -> &mut Self {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.all_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_all_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
        self
    }

    /// Set the join/leave log channel, without mutating.
    pub fn with_join_leave_log_channel(&self, channel_id: RoomRef) -> Self {
        let log_settings = if let Some(log_settings) = self.log_settings.clone() {
            LogSettings {
                join_leave_log_channel: Some(channel_id.clone()),
                ..log_settings
            }
        } else {
            LogSettings {
                join_leave_log_channel: Some(channel_id),
                ..Default::default()
            }
        };
        Self {
            log_settings: Some(log_settings),
            ..self.clone()
        }
    }

    /// Set the server join/leave log channel, mutating.
    pub fn set_join_leave_log_channel(&mut self, channel_id: RoomRef) -> &mut Self {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.join_leave_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_join_leave_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
        self
    }

    /// Set command settings, returning a new GuildSettings.
    pub fn with_command_settings(
        self,
        command_settings: HashMap<String, GenericPermissionSettings>,
    ) -> Self {
        Self {
            command_settings,
            ..self
        }
    }

    /// Set the command settings, mutating.
    pub fn set_command_settings(
        &mut self,
        command_settings: HashMap<String, GenericPermissionSettings>,
    ) -> &mut Self {
        self.command_settings = command_settings;
        self
    }

    pub fn get_log_channel(&self, _name: &str) -> Option<RoomRef> {
        self.get_all_log_channel()
    }

    pub fn get_all_log_channel(&self) -> Option<RoomRef> {
        if let Some(log_settings) = &self.log_settings {
            return log_settings.get_all_log_channel();
        }
        None
    }

    pub fn get_join_leave_log_channel(&self) -> Option<RoomRef> {
        self.log_settings
            .as_ref()
            .and_then(|log_settings| log_settings.join_leave_log_channel.clone())
    }

    pub fn get_music_channel(&self) -> Option<RoomRef> {
        self.command_settings
            .get("music")
            .and_then(|x| x.allowed_channels.iter().next().cloned())
    }

    pub fn get_music_permissions(&self) -> Option<GenericPermissionSettings> {
        self.command_settings.get("music").cloned()
    }

    /// Adds a user to the denied music users list.
    pub async fn add_denied_music_user(&mut self, user_id: UserRef) -> CrackedResult<bool> {
        let mut perms = self.get_music_permissions().unwrap_or_default();
        perms.add_denied_user(user_id);
        self.command_settings.insert("music".to_string(), perms);
        Ok(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CommandSettingsMap;

impl TypeMapKey for CommandSettingsMap {
    type Value = HashMap<String, GenericPermissionSettings>;
}

/// Struct to hold the guild settings map in a typemap
#[derive(Default)]
pub struct GuildSettingsMap;

/// Implement the TypeMapKey trait for the GuildSettingsMap
impl TypeMapKey for GuildSettingsMap {
    type Value = HashMap<GuildId, GuildSettings>;
}

/// Struct to hold the atomic u16 key in a typemap
#[derive(Default)]
pub struct AtomicU16Key;

/// Implement the TypeMapKey trait for the AtomicU16Key
impl TypeMapKey for AtomicU16Key {
    type Value = Arc<atomic::AtomicU16>;
}

/// Convenience type for the GuildSettingsMap
pub type GuildSettingsMapParam =
    std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<GuildId, GuildSettings>>>;

#[cfg(test)]
mod test {
    use crate::guild::settings::{GuildSettings, Provenance};
    use crack_types::discord::ToRef;
    use serenity::all::{GenericChannelId, GuildId};

    #[test]
    fn test_default() {
        let settings = crate::guild::settings::GuildSettings::new(GuildId::new(123), None, None);
        assert_eq!(settings.prefix, "r!");
        assert_eq!(settings.allowed_domains.len(), 1);
        assert!(settings.allowed_domains.contains("youtube.com"));
        assert_eq!(settings.banned_domains.len(), 0);
        assert_eq!(settings.ignored_channels.len(), 1);
        assert_eq!(settings.old_volume, 1.0);
        assert_eq!(settings.volume, 1.0);
        assert!(settings.self_deafen);
        assert_eq!(settings.timeout, 600);
        assert!(settings.welcome_settings.is_none());
        assert!(settings.log_settings.is_none());
        assert_eq!(settings.additional_prefixes.len(), 0);
    }

    #[test]
    fn settings_json_with_the_removed_authorization_keys_still_loads() {
        let settings = GuildSettings::new(GuildId::new(123), None, None);
        let mut value = serde_json::to_value(&settings).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.insert("authorized_users".into(), serde_json::json!({"1": 2}));
        obj.insert("authorized_groups".into(), serde_json::json!({}));
        let back: GuildSettings = serde_json::from_value(value).unwrap();
        assert_eq!(back, settings);
        assert!(!serde_json::to_string(&settings)
            .unwrap()
            .contains("authorized"));
    }

    #[test]
    fn test_get_log_channel() {
        let mut settings =
            crate::guild::settings::GuildSettings::new(GuildId::new(123), None, None);
        let channel_id = 123;
        settings.set_all_log_channel(GenericChannelId::new(channel_id).to_ref());
        assert_eq!(
            settings.get_log_channel("all"),
            Some(GenericChannelId::new(123).to_ref())
        );
    }

    #[test]
    fn test_default_functions() {
        use super::{
            additional_prefixes_default, allow_all_domains_default, default_false, default_true,
            premium_default, volume_default,
        };
        assert_eq!(allow_all_domains_default(), Some(true));
        assert_eq!(additional_prefixes_default().len(), 0);
        assert_eq!(volume_default(), 1.0);
        assert!(!premium_default());
        assert!(!default_false());
        assert!(default_true());
    }

    #[test]
    fn a_fresh_settings_value_is_fallback() {
        // The safe direction: anything not explicitly loaded from Postgres
        // must never be written back over a stored row.
        let settings = GuildSettings::new(GuildId::new(123), None, None);
        assert_eq!(settings.provenance, Provenance::Fallback);
        assert!(!settings.is_persistable());
    }

    #[test]
    fn the_default_provenance_is_the_safe_one() {
        assert_eq!(Provenance::default(), Provenance::Fallback);
    }

    #[test]
    fn with_provenance_marks_it_persistable() {
        let settings =
            GuildSettings::new(GuildId::new(123), None, None).with_provenance(Provenance::Database);
        assert_eq!(settings.provenance, Provenance::Database);
        assert!(settings.is_persistable());
    }

    #[test]
    fn provenance_does_not_affect_equality() {
        // GuildSettings derives PartialEq and is compared in existing tests.
        // Provenance is bookkeeping, not part of a guild's settings.
        let fallback = GuildSettings::new(GuildId::new(123), None, None);
        let from_db = fallback.clone().with_provenance(Provenance::Database);
        assert_eq!(fallback, from_db);
    }

    #[test]
    fn provenance_is_not_serialized() {
        // It is not a column and must not reach the wire.
        let from_db =
            GuildSettings::new(GuildId::new(123), None, None).with_provenance(Provenance::Database);
        let json = serde_json::to_string(&from_db).expect("serialize");
        assert!(
            !json.contains("provenance"),
            "provenance leaked into serialized output: {json}"
        );
    }

    #[test]
    fn a_builder_that_copies_self_preserves_provenance() {
        // `with_volume` and friends use `..self`. A mutation must not silently
        // downgrade a Database-loaded value back to Fallback.
        let from_db = GuildSettings::new(GuildId::new(123), None, None)
            .with_provenance(Provenance::Database)
            .with_volume(0.8);
        assert_eq!(from_db.provenance, Provenance::Database);
    }

    #[test]
    fn ephemeral_replies_are_off_by_default() {
        assert!(!GuildSettings::new(GuildId::new(123), None, None).ephemeral_replies);
    }

    #[test]
    fn toggling_ephemeral_replies_flips_them() {
        let mut settings = GuildSettings::new(GuildId::new(123), None, None);

        settings.toggle_ephemeral_replies();
        assert!(settings.ephemeral_replies);
        settings.toggle_ephemeral_replies();
        assert!(!settings.ephemeral_replies);
    }

    #[test]
    fn a_database_row_carries_ephemeral_replies() {
        let row = crate::db::GuildSettingsRead {
            guild_id: 123,
            guild_name: "guild".to_string(),
            prefix: "r!".to_string(),
            premium: false,
            autopause: false,
            allow_all_domains: true,
            allowed_domains: vec![],
            banned_domains: vec![],
            ignored_channels: vec![],
            old_volume: 1.0,
            volume: 1.0,
            self_deafen: true,
            timeout_seconds: Some(360),
            additional_prefixes: vec![],
            ephemeral_replies: true,
            control_echoes: true,
            now_playing_buttons: true,
        };

        assert!(GuildSettings::from(row).ephemeral_replies);
    }

    #[test]
    fn a_database_row_carries_now_playing_buttons() {
        let row = crate::db::GuildSettingsRead {
            guild_id: 123,
            guild_name: "guild".to_string(),
            prefix: "r!".to_string(),
            premium: false,
            autopause: false,
            allow_all_domains: true,
            allowed_domains: vec![],
            banned_domains: vec![],
            ignored_channels: vec![],
            old_volume: 1.0,
            volume: 1.0,
            self_deafen: true,
            timeout_seconds: Some(360),
            additional_prefixes: vec![],
            ephemeral_replies: false,
            control_echoes: true,
            now_playing_buttons: false,
        };
        assert!(!GuildSettings::from(row).now_playing_buttons);
    }

    #[test]
    fn new_settings_have_buttons_and_settings_differing_in_them_are_not_equal() {
        let on = GuildSettings::new(GuildId::new(123), None, None);
        assert!(on.now_playing_buttons);
        let mut off = on.clone();
        off.toggle_now_playing_buttons();
        assert!(!off.now_playing_buttons);
        assert_ne!(on, off);
    }

    #[test]
    fn settings_differing_only_in_ephemeral_replies_are_not_equal() {
        let visible = GuildSettings::new(GuildId::new(123), None, None);
        let mut private = visible.clone();
        private.ephemeral_replies = true;

        assert_ne!(visible, private);
    }

    #[test]
    fn control_echoes_are_on_by_default() {
        assert!(GuildSettings::new(GuildId::new(123), None, None).control_echoes);
    }

    #[test]
    fn toggling_control_echoes_flips_them() {
        let mut settings = GuildSettings::new(GuildId::new(123), None, None);
        settings.toggle_control_echoes();
        assert!(!settings.control_echoes);
        settings.toggle_control_echoes();
        assert!(settings.control_echoes);
    }

    #[test]
    fn a_database_row_carries_control_echoes() {
        let row = crate::db::GuildSettingsRead {
            guild_id: 123,
            guild_name: "guild".to_string(),
            prefix: "r!".to_string(),
            premium: false,
            autopause: false,
            allow_all_domains: true,
            allowed_domains: vec![],
            banned_domains: vec![],
            ignored_channels: vec![],
            old_volume: 1.0,
            volume: 1.0,
            self_deafen: true,
            timeout_seconds: Some(360),
            additional_prefixes: vec![],
            ephemeral_replies: false,
            control_echoes: false,
            now_playing_buttons: true,
        };
        assert!(!GuildSettings::from(row).control_echoes);
    }

    #[test]
    fn settings_differing_only_in_control_echoes_are_not_equal() {
        let on = GuildSettings::new(GuildId::new(123), None, None);
        let mut off = on.clone();
        off.control_echoes = false;
        assert_ne!(on, off);
    }

    #[test]
    fn the_welcome_reply_text_is_unchanged() {
        use super::WelcomeSettings;
        use serenity::all::RoleId;
        // set_welcome_settings.rs replies with this Display, so its bytes are user-visible.
        let w = WelcomeSettings::new()
            .with_channel_id(GenericChannelId::new(123).to_ref())
            .with_message("hi".into())
            .with_auto_role(RoleId::new(456).to_ref());
        assert_eq!(
            w.to_string(),
            "{\n  \"channel_id\": 123,\n  \"message\": \"hi\",\n  \"auto_role\": 456,\n  \"password\": null\n}"
        );
    }

    #[test]
    fn log_settings_serialise_channels_as_numbers() {
        use super::LogSettings;
        let mut l = LogSettings::default();
        l.set_all_log_channel(GenericChannelId::new(9).to_ref());
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(v["all_log_channel"], 9);
        assert_eq!(serde_json::from_value::<LogSettings>(v).unwrap(), l);
    }

    #[test]
    fn garbage_stored_ids_are_dropped_not_panicked_on() {
        use super::LogSettings;
        use crack_types::discord::from_snowflake;
        let row = crate::db::LogSettingsRead {
            guild_id: 1,
            all_log_channel: Some(0),
            raw_event_log_channel: Some(-5),
            server_log_channel: Some(i64::MAX),
            member_log_channel: None,
            join_leave_log_channel: None,
            voice_log_channel: None,
        };
        let l = LogSettings::from(row);
        assert_eq!(l.get_all_log_channel(), None);
        assert_eq!(l.raw_event_log_channel, None);
        assert_eq!(
            l.get_server_log_channel(),
            Some(from_snowflake(i64::MAX as u64).unwrap())
        );
    }

    #[test]
    fn garbage_welcome_ids_are_dropped_not_panicked_on() {
        use super::WelcomeSettings;
        let row = crate::db::WelcomeSettingsRead {
            guild_id: 1,
            auto_role: Some(-1),
            channel_id: Some(0),
            message: Some("hi".into()),
        };
        let w = WelcomeSettings::from(row);
        assert_eq!(w.channel_id, None);
        assert_eq!(w.auto_role, None);
        assert_eq!(w.message.as_deref(), Some("hi"));
    }
}
