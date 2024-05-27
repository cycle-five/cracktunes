use self::serenity::model::id::GuildId;
use self::serenity::model::prelude::UserId;
use crate::db::{GuildEntity, WelcomeSettingsRead};
use crate::errors::CrackedError;
use lazy_static::lazy_static;
use poise::serenity_prelude::{self as serenity, ChannelId, FullEvent};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::sync::{atomic, Arc};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{create_dir_all, OpenOptions},
};
use typemap_rev::TypeMapKey;

use super::permissions::{CommandChannel, GenericPermissionSettings};

pub const DEFAULT_LOG_PREFIX: &str = "data/logs";
pub(crate) const DEFAULT_ALLOW_ALL_DOMAINS: bool = true;
pub(crate) const DEFAULT_SETTINGS_PATH: &str = "data/settings";
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
pub(crate) const MOD_VAL: u64 = 1 << 1;
pub(crate) const ADMIN_VAL: u64 = 2 << 1;

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

/// Settings for a command channel.
// #[derive(Deserialize, Serialize, Debug, Clone, Default)]
// pub struct CommandChannelSettings {
//     pub id: ChannelId,
//     pub perms: GenericPermissionSettings,
// }

/// Command channels to restrict where and who can use what commands
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub struct CommandChannels {
    pub music_channel: Option<CommandChannel>,
}

impl CommandChannels {
    /// Set the music channel, mutating.
    pub fn set_music_channel(
        &mut self,
        channel_id: ChannelId,
        guild_id: GuildId,
        perms: GenericPermissionSettings,
    ) -> &mut Self {
        self.music_channel = Some(CommandChannel {
            command: "music".to_string(),
            channel_id,
            guild_id,
            permission_settings: perms,
        });
        self
    }

    /// Set the music channel, returning a new CommandChannels.
    pub fn with_music_channel(
        self,
        channel_id: ChannelId,
        guild_id: GuildId,
        perms: GenericPermissionSettings,
    ) -> Self {
        let music_channel = Some(CommandChannel {
            command: "music".to_string(),
            channel_id,
            guild_id,
            permission_settings: perms,
        });
        Self { music_channel }
    }

    /// Get the music channel.
    pub fn get_music_channel(&self) -> Option<CommandChannel> {
        self.music_channel.clone()
    }

    /// Insert the command channel into the database.
    pub async fn save(&self, pool: &PgPool) -> Option<CommandChannel> {
        match self.music_channel {
            Some(ref c) => c.insert_command_channel(pool).await.ok(),
            None => None,
        }
    }

    pub async fn load(guild_id: GuildId, pool: &PgPool) -> Result<Self, CrackedError> {
        let music_channels =
            CommandChannel::get_command_channels(pool, "music".to_string(), guild_id).await;

        let music_channel = music_channels.first().cloned();

        Ok(Self { music_channel })
    }
}

#[derive(Default, Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct LogSettings {
    // TODO: Decide if I want to have separate raw events and all log channels.
    pub all_log_channel: Option<u64>,
    pub raw_event_log_channel: Option<u64>,
    // TODO: Decide on what level of granularity I want for logging options.
    // Also should they be able to overlap?
    pub server_log_channel: Option<u64>,
    pub member_log_channel: Option<u64>,
    pub join_leave_log_channel: Option<u64>,
    pub voice_log_channel: Option<u64>,
}

impl From<crate::db::LogSettingsRead> for LogSettings {
    fn from(settings_db: crate::db::LogSettingsRead) -> Self {
        LogSettings {
            all_log_channel: settings_db.all_log_channel.map(|x| x as u64),
            raw_event_log_channel: settings_db.raw_event_log_channel.map(|x| x as u64),
            server_log_channel: settings_db.server_log_channel.map(|x| x as u64),
            member_log_channel: settings_db.member_log_channel.map(|x| x as u64),
            join_leave_log_channel: settings_db.join_leave_log_channel.map(|x| x as u64),
            voice_log_channel: settings_db.voice_log_channel.map(|x| x as u64),
        }
    }
}

const DEFAULT_LOG_CHANNEL: Option<ChannelId> = None;

impl LogSettings {
    pub fn get_all_log_channel(&self) -> Option<ChannelId> {
        self.all_log_channel
            .map(ChannelId::new)
            .or(DEFAULT_LOG_CHANNEL)
    }

    pub fn get_server_log_channel(&self) -> Option<ChannelId> {
        self.server_log_channel
            .map(ChannelId::new)
            .or(DEFAULT_LOG_CHANNEL)
    }

    pub fn get_join_leave_log_channel(&self) -> Option<ChannelId> {
        self.join_leave_log_channel
            .map(ChannelId::new)
            .or(DEFAULT_LOG_CHANNEL)
    }

    pub fn get_member_log_channel(&self) -> Option<ChannelId> {
        self.member_log_channel
            .map(ChannelId::new)
            .or(DEFAULT_LOG_CHANNEL)
    }

    pub fn get_voice_log_channel(&self) -> Option<ChannelId> {
        self.voice_log_channel
            .map(ChannelId::new)
            .or(DEFAULT_LOG_CHANNEL)
    }

    pub fn set_all_log_channel(&mut self, channel_id: u64) -> &mut Self {
        self.all_log_channel = Some(channel_id);
        self
    }

    pub fn set_server_log_channel(&mut self, channel_id: u64) -> &mut Self {
        self.server_log_channel = Some(channel_id);
        self
    }

    pub fn set_join_leave_log_channel(&mut self, channel_id: u64) -> &mut Self {
        self.join_leave_log_channel = Some(channel_id);
        self
    }

    pub fn set_member_log_channel(&mut self, channel_id: u64) -> &mut Self {
        self.member_log_channel = Some(channel_id);
        self
    }

    pub fn set_voice_log_channel(&mut self, channel_id: u64) -> &mut Self {
        self.voice_log_channel = Some(channel_id);
        self
    }

    /// Write the log settings to the database.
    pub async fn save(&self, pool: &PgPool, guild_id: u64) -> Result<(), CrackedError> {
        crate::db::GuildEntity::write_log_settings(pool, guild_id as i64, self).await
    }
}

#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq)]
pub struct WelcomeSettings {
    pub channel_id: Option<u64>,
    pub message: Option<String>,
    pub auto_role: Option<u64>,
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
            channel_id: settings_db.channel_id.map(|x| x as u64),
            message: settings_db.message,
            auto_role: settings_db.auto_role.map(|x| x as u64),
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
    pub fn with_channel_id(self, channel_id: u64) -> Self {
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
    pub fn with_auto_role(self, auto_role: u64) -> Self {
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
        crate::db::GuildEntity::write_welcome_settings(pool, guild_id as i64, self)
            .await
            .map_err(CrackedError::SQLX)
    }
}

/// A struct that represents a user's permission level for a guild.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct UserPermission {
    pub user_id: i64,
    pub guild_id: i64,
    pub permission: i64,
}

/// A struct that represents a user's permission level for a guild.
impl UserPermission {
    /// Creates a new UserPermission with default values.
    pub fn new_default(user_id: i64, guild_id: i64) -> Self {
        Self {
            user_id,
            guild_id,
            permission: 0,
        }
    }

    /// Creates a new UserPermission with the given values.
    pub fn new(user_id: i64, guild_id: i64, permission: i64) -> Self {
        Self {
            user_id,
            guild_id,
            permission,
        }
    }

    /// Get the user id.
    pub fn get_user_id(&self) -> i64 {
        self.user_id
    }

    /// Get the guild id.
    pub fn get_guild_id(&self) -> i64 {
        self.guild_id
    }

    /// Get the permission level.
    pub fn get_permission(&self) -> i64 {
        self.permission
    }

    /// Set the permission level, mutating
    pub fn set_permission(&mut self, permission: i64) {
        self.permission = permission;
    }

    /// Set the user id, mutating
    pub fn set_user_id(&mut self, user_id: i64) {
        self.user_id = user_id;
    }

    /// Set the guild id, mutating
    pub fn set_guild_id(&mut self, guild_id: i64) {
        self.guild_id = guild_id;
    }

    /// Set the permission level, returning a new UserPermission
    pub fn with_permission(self, permission: i64) -> Self {
        Self { permission, ..self }
    }

    /// Set the user id, returning a new UserPermission
    pub fn with_user_id(self, user_id: i64) -> Self {
        Self { user_id, ..self }
    }

    /// Set the guild id, returning a new UserPermission
    pub fn with_guild_id(self, guild_id: i64) -> Self {
        Self { guild_id, ..self }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct GuildSettings {
    pub guild_id: GuildId,
    pub guild_name: String,
    pub prefix: String,
    #[serde(default = "premium_default")]
    pub premium: bool,
    #[serde(default = "CommandChannels::default")]
    pub command_channels: CommandChannels,
    #[serde(default = "default_false")]
    pub autopause: bool,
    #[serde(default = "default_true")]
    pub autoplay: bool,
    #[serde(default = "default_true")]
    pub reply_with_embed: bool,
    #[serde(default = "allow_all_domains_default")]
    pub allow_all_domains: Option<bool>,
    pub allowed_domains: HashSet<String>,
    pub banned_domains: HashSet<String>,
    #[serde(default = "authorized_users_default")]
    pub authorized_users: BTreeMap<u64, u64>,
    #[serde(default = "authorized_users_default")]
    pub authorized_groups: BTreeMap<u64, u64>,
    pub ignored_channels: HashSet<u64>,
    #[serde(default = "volume_default")]
    pub old_volume: f32,
    #[serde(default = "volume_default")]
    pub volume: f32,
    pub self_deafen: bool,
    pub timeout: u32,
    pub welcome_settings: Option<WelcomeSettings>,
    pub log_settings: Option<LogSettings>,
    #[serde(default = "additional_prefixes_default")]
    pub additional_prefixes: Vec<String>,
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

/// Default value function for serialization for authorized users.
fn authorized_users_default() -> BTreeMap<u64, u64> {
    BTreeMap::new()
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

impl From<crate::db::GuildSettingsRead> for GuildSettings {
    fn from(settings_db: crate::db::GuildSettingsRead) -> Self {
        let mut settings = GuildSettings::new(
            GuildId::new(settings_db.guild_id as u64),
            Some(&settings_db.prefix),
            Some(settings_db.guild_name),
        );
        settings.premium = settings_db.premium;
        settings.autopause = settings_db.autopause;
        settings.autoplay = true; //settings_db.autoplay;
        settings.allow_all_domains = Some(settings_db.allow_all_domains);
        settings.allowed_domains = settings_db.allowed_domains.into_iter().collect();
        settings.banned_domains = settings_db.banned_domains.into_iter().collect();
        settings.authorized_users = BTreeMap::<u64, u64>::new(); // FIXME
        settings.authorized_groups = BTreeMap::<u64, u64>::new(); // FIXME

        settings.ignored_channels = settings_db
            .ignored_channels
            .into_iter()
            .map(|x| x as u64)
            .collect();
        settings.old_volume = settings_db.old_volume as f32;
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
        guild_name: Option<String>,
    ) -> GuildSettings {
        let allowed_domains: HashSet<String> = DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|d| d.to_string())
            .collect();

        let my_prefix = match prefix {
            Some(p) => p.to_string(),
            None => DEFAULT_PREFIX.to_string(),
        };

        let guild_name = guild_name.map(|x| x.to_string()).unwrap_or_default();
        let asdf: Vec<u64> = vec![1165246445654388746];
        GuildSettings {
            guild_id,
            guild_name,
            prefix: my_prefix.clone(),
            premium: DEFAULT_PREMIUM,
            command_channels: CommandChannels::default(),
            autopause: false,
            autoplay: true,
            reply_with_embed: true,
            allow_all_domains: Some(DEFAULT_ALLOW_ALL_DOMAINS),
            allowed_domains,
            banned_domains: HashSet::new(),
            authorized_users: BTreeMap::new(),
            authorized_groups: BTreeMap::new(),
            ignored_channels: asdf.into_iter().collect(),
            old_volume: DEFAULT_VOLUME_LEVEL,
            volume: DEFAULT_VOLUME_LEVEL,
            self_deafen: true,
            timeout: DEFAULT_IDLE_TIMEOUT,
            welcome_settings: None,
            log_settings: None,
            additional_prefixes: Vec::new(),
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
        tracing::warn!("Saving guild settings: {:?}", self);
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
        tracing::warn!("Saving guild settings: {:?}", self);
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

    /// Toggle the autoplay setting, mutating.
    pub fn toggle_self_deafen(&mut self) -> &mut Self {
        self.self_deafen = !self.self_deafen;
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

    /// Set the music channel, without mutating.
    pub fn set_music_channel(&mut self, channel_id: u64) -> &mut Self {
        self.command_channels.set_music_channel(
            ChannelId::new(channel_id),
            self.guild_id,
            Default::default(),
        );
        self
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

    /// Authorize a user.
    pub fn authorize_user(&mut self, user_id: i64) -> &mut Self {
        self.authorized_users.entry(user_id as u64).or_insert(0);
        self
    }

    /// Deauthorize a user.
    pub fn deauthorize_user(&mut self, user_id: i64) {
        if self.authorized_users.contains_key(&(user_id as u64)) {
            self.authorized_users.remove(&(user_id as u64));
        }
    }

    /// Check if the user is authorized.
    pub fn check_authorized(&self, user_id: u64) -> bool {
        self.authorized_users.contains_key(&user_id)
    }

    /// Check if the user is authorized.
    pub fn check_authorized_user_id(&self, user_id: UserId) -> bool {
        self.authorized_users.contains_key(&user_id.into())
    }

    /// Check if the user is a mod.
    pub fn check_mod(&self, user_id: u64) -> bool {
        self.authorized_users.get(&user_id).unwrap_or(&0) >= &MOD_VAL
    }

    /// Check if the user is a mod.
    pub fn check_mod_user_id(&self, user_id: UserId) -> bool {
        self.authorized_users.get(&user_id.into()).unwrap_or(&0) >= &MOD_VAL
    }

    /// Check if the user is an admin.
    pub fn check_admin(&self, user_id: u64) -> bool {
        self.authorized_users.get(&user_id).unwrap_or(&0) >= &ADMIN_VAL
    }

    /// Check if the user is an admin.
    pub fn check_admin_user_id(&self, user_id: UserId) -> bool {
        self.authorized_users.get(&user_id.into()).unwrap_or(&0) >= &ADMIN_VAL
    }

    /// Set the volume level without mutating.
    pub fn with_volume(self, volume: f32) -> Self {
        Self {
            old_volume: self.volume,
            volume,
            ..self
        }
    }

    /// Set the volume level with mutating.
    pub fn set_volume(&mut self, volume: f32) -> &mut Self {
        self.old_volume = self.volume;
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
        channel_id: u64,
        auto_role: Option<u64>,
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
    pub fn set_welcome_settings3(&mut self, channel_id: u64, message: String) -> &mut Self {
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
    pub fn with_auto_role(self, auto_role: Option<u64>) -> Self {
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
    pub fn set_auto_role(&mut self, auto_role: Option<u64>) -> &mut Self {
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
        all_log_channel: u64,
        join_leave_log_channel: u64,
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
            self.guild_name.clone()
        }
    }

    /// Get the prefix.
    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    /// Set the all log channel, with mutating.
    pub fn set_all_log_channel(&mut self, channel_id: u64) -> &mut Self {
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
    pub fn with_join_leave_log_channel(&self, channel_id: u64) -> Self {
        let log_settings = if let Some(log_settings) = self.log_settings.clone() {
            LogSettings {
                join_leave_log_channel: Some(channel_id),
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

    /// Set the command channels, notmutating.
    pub fn with_command_channels(&self, command_channels: CommandChannels) -> Self {
        Self {
            command_channels,
            ..self.clone()
        }
    }

    /// Set the server join/leave log channel, mutating.
    pub fn set_join_leave_log_channel(&mut self, channel_id: u64) -> &mut Self {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.join_leave_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_join_leave_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
        self
    }

    /// Get the log channel for the given event type.
    pub fn get_log_channel_type_fe(&self, event: &FullEvent) -> Option<ChannelId> {
        let log_settings = self.log_settings.clone().unwrap_or_default();
        match event {
            | FullEvent::PresenceUpdate { .. } => {
                None
                //.or(log_settings.get_all_log_channel()),
            }
            | FullEvent::GuildMemberAddition { .. }
            | FullEvent::GuildMemberRemoval { .. } => {
                log_settings.get_join_leave_log_channel().or(log_settings.get_all_log_channel())
            }
            | FullEvent::GuildBanRemoval { .. }
            | FullEvent::GuildBanAddition { .. }
            | FullEvent::GuildScheduledEventCreate { .. }
            | FullEvent::GuildScheduledEventUpdate { .. }
            | FullEvent::GuildScheduledEventDelete { .. }
            | FullEvent::GuildScheduledEventUserAdd { .. }
            | FullEvent::GuildScheduledEventUserRemove { .. }
            | FullEvent::GuildStickersUpdate { .. }
            | FullEvent::GuildCreate { .. }
            | FullEvent::GuildDelete { .. }
            | FullEvent::GuildEmojisUpdate { .. }
            | FullEvent::GuildIntegrationsUpdate { .. }
            | FullEvent::GuildMemberUpdate { .. }
            | FullEvent::GuildMembersChunk { .. }
            | FullEvent::GuildRoleCreate { .. }
            | FullEvent::GuildRoleDelete { .. }
            | FullEvent::GuildRoleUpdate { .. }
            //| FullEvent::GuildUnavailable { .. }
            | FullEvent::GuildUpdate { .. } => log_settings
                .get_server_log_channel()
                .or(log_settings.get_all_log_channel()),
            FullEvent::Message { .. }
            | FullEvent::Resume { .. }
            | FullEvent::ShardStageUpdate { .. }
            | FullEvent::WebhookUpdate { .. }
            | FullEvent::CommandPermissionsUpdate { .. }
            | FullEvent::AutoModActionExecution { .. }
            | FullEvent::AutoModRuleCreate { .. }
            | FullEvent::AutoModRuleUpdate { .. }
            | FullEvent::AutoModRuleDelete { .. }
            | FullEvent::CacheReady { .. }
            | FullEvent::ChannelCreate { .. }
            | FullEvent::CategoryCreate { .. }
            | FullEvent::CategoryDelete { .. }
            | FullEvent::ChannelDelete { .. }
            | FullEvent::ChannelPinsUpdate { .. }
            | FullEvent::ChannelUpdate { .. }
            | FullEvent::IntegrationCreate { .. }
            | FullEvent::IntegrationUpdate { .. }
            | FullEvent::IntegrationDelete { .. }
            | FullEvent::InteractionCreate { .. }
            | FullEvent::InviteCreate { .. }
            | FullEvent::InviteDelete { .. }
            | FullEvent::MessageDelete { .. }
            | FullEvent::MessageDeleteBulk { .. }
            | FullEvent::MessageUpdate { .. }
            | FullEvent::ReactionAdd { .. }
            | FullEvent::ReactionRemove { .. }
            | FullEvent::ReactionRemoveAll { .. }
            | FullEvent::Ready { .. }
            | FullEvent::StageInstanceCreate { .. }
            | FullEvent::StageInstanceDelete { .. }
            | FullEvent::StageInstanceUpdate { .. }
            | FullEvent::ThreadCreate { .. }
            | FullEvent::ThreadDelete { .. }
            | FullEvent::ThreadListSync { .. }
            | FullEvent::ThreadMemberUpdate { .. }
            | FullEvent::ThreadMembersUpdate { .. }
            | FullEvent::ThreadUpdate { .. }
            | FullEvent::TypingStart { .. }
            // | FullEvent::Unknown { .. }
            | FullEvent::UserUpdate { .. }
            | FullEvent::VoiceServerUpdate { .. }
            | FullEvent::VoiceStateUpdate { .. } => {
                // tracing::warn!(
                //     "{}",
                //     format!("Event: {:?}", event).as_str().to_string().white()
                // );
                log_settings.get_all_log_channel()
            }
            _ => todo!(),
        }
    }

    pub fn get_log_channel(&self, _name: &str) -> Option<ChannelId> {
        self.get_all_log_channel()
    }

    pub fn get_all_log_channel(&self) -> Option<ChannelId> {
        if let Some(log_settings) = &self.log_settings {
            return log_settings.get_all_log_channel();
        }
        None
    }

    pub fn get_join_leave_log_channel(&self) -> Option<ChannelId> {
        if let Some(log_settings) = &self.log_settings {
            if let Some(channel_id) = log_settings.join_leave_log_channel {
                return Some(ChannelId::new(channel_id));
            }
        }
        None
    }
}

/// Save the guild settings to the database.
pub async fn save_guild_settings(
    guild_settings_map: &HashMap<GuildId, GuildSettings>,
    pool: &PgPool,
) {
    for guild_settings in guild_settings_map.values() {
        let _ = guild_settings.save(pool).await;
    }
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
    std::sync::Arc<std::sync::RwLock<std::collections::HashMap<GuildId, GuildSettings>>>;

#[cfg(test)]
mod test {
    use serenity::all::GuildId;

    #[test]
    fn test_default() {
        let settings = crate::guild::settings::GuildSettings::new(GuildId::new(123), None, None);
        assert_eq!(settings.prefix, "r!");
        assert_eq!(settings.allowed_domains.len(), 1);
        assert_eq!(settings.allowed_domains.contains("youtube.com"), true);
        assert_eq!(settings.banned_domains.len(), 0);
        assert_eq!(settings.authorized_users.len(), 0);
        assert_eq!(settings.ignored_channels.len(), 1);
        assert_eq!(settings.old_volume, 1.0);
        assert_eq!(settings.volume, 1.0);
        assert_eq!(settings.self_deafen, true);
        assert_eq!(settings.timeout, 600);
        assert_eq!(settings.welcome_settings.is_none(), true);
        assert_eq!(settings.log_settings.is_none(), true);
        assert_eq!(settings.additional_prefixes.len(), 0);
    }

    #[test]
    fn test_get_log_channel() {
        let mut settings =
            crate::guild::settings::GuildSettings::new(GuildId::new(123), None, None);
        let channel_id = 123;
        settings.set_all_log_channel(channel_id);
        assert_eq!(
            settings.get_log_channel("all"),
            Some(serenity::model::id::ChannelId::new(123))
        );
    }

    #[test]
    fn test_get_log_channel_type_fe() {
        let mut settings =
            crate::guild::settings::GuildSettings::new(GuildId::new(123), None, None);
        let channel_id = 123;
        settings.set_all_log_channel(channel_id);
        let event = serenity::all::FullEvent::GuildCreate {
            guild: serenity::model::guild::Guild::default(),
            is_new: Some(true),
        };
        assert_eq!(
            settings.get_log_channel_type_fe(&event),
            Some(serenity::model::id::ChannelId::new(123))
        );
    }

    #[test]
    fn test_default_functions() {
        use super::{
            additional_prefixes_default, allow_all_domains_default, authorized_users_default,
            default_false, default_true, premium_default, volume_default,
        };
        assert_eq!(allow_all_domains_default(), Some(true));
        assert_eq!(authorized_users_default().len(), 0);
        assert_eq!(additional_prefixes_default().len(), 0);
        assert_eq!(volume_default(), 1.0);
        assert_eq!(premium_default(), false);
        assert_eq!(default_false(), false);
        assert_eq!(default_true(), true);
    }
}
