use self::serenity::model::prelude::UserId;
use self::serenity::{model::id::GuildId, TypeMapKey};
use lazy_static::lazy_static;
use poise::serenity_prelude::{self as serenity, ChannelId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{create_dir_all, OpenOptions},
    io::{BufReader, BufWriter},
    path::Path,
};

use crate::errors::CrackedError;

pub(crate) const DEFAULT_ALLOW_ALL_DOMAINS: bool = true;
pub(crate) const DEFAULT_SETTINGS_PATH: &str = "data/settings";
pub(crate) const DEFAULT_ALLOWED_DOMAINS: [&str; 1] = ["youtube.com"];
pub(crate) const DEFAULT_VOLUME_LEVEL: f32 = 1.0;
pub(crate) const DEFAULT_VIDEO_STATUS_POLL_INTERVAL: u64 = 120;
pub(crate) const DEFAULT_PREFIX: &str = "r!";
pub(crate) const DEFAULT_DB_URL: &str = "sqlite:///data/cracktunes.db";
pub(crate) const DEFAULT_IDLE_TIMEOUT: u32 = 0; //5 * 60;
pub(crate) const DEFAULT_LYRICS_PAGE_SIZE: usize = 1024;

lazy_static! {
    static ref SETTINGS_PATH: String =
        env::var("SETTINGS_PATH").unwrap_or(DEFAULT_SETTINGS_PATH.to_string());
}

#[derive(Default, Deserialize, Serialize, Debug, Clone)]
pub struct LogSettings {
    // TODO: Decide if I want to have separate raw events and all log channels.
    all_log_channel: Option<u64>,
    raw_event_log_channel: Option<u64>,
    // TODO: Decide on what level of granularity I want for logging options.
    // Also should they be able to overlap?
    server_log_channel: Option<u64>,
    member_log_channel: Option<u64>,
    join_leave_log_channel: Option<u64>,
    voice_log_channel: Option<u64>,
}

const DEFAULT_LOG_CHANNEL: u64 = 1165246445654388746;

impl LogSettings {
    pub fn get_all_log_channel(&self) -> Option<ChannelId> {
        self.all_log_channel
            .map(ChannelId)
            .or(Some(ChannelId(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_server_log_channel(&self) -> Option<ChannelId> {
        self.server_log_channel
            .map(ChannelId)
            .or(Some(ChannelId(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_join_leave_log_channel(&self) -> Option<ChannelId> {
        self.join_leave_log_channel
            .map(ChannelId)
            .or(Some(ChannelId(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_member_log_channel(&self) -> Option<ChannelId> {
        self.member_log_channel
            .map(ChannelId)
            .or(Some(ChannelId(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_voice_log_channel(&self) -> Option<ChannelId> {
        self.voice_log_channel
            .map(ChannelId)
            .or(Some(ChannelId(DEFAULT_LOG_CHANNEL)))
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
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct WelcomeSettings {
    pub channel_id: Option<u64>,
    pub message: Option<String>,
    pub auto_role: Option<u64>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct GuildSettings {
    pub guild_id: GuildId,
    pub prefix: String,
    pub prefix_up: String,
    pub autopause: bool,
    pub allow_all_domains: Option<bool>,
    pub allowed_domains: HashSet<String>,
    pub banned_domains: HashSet<String>,
    pub authorized_users: HashSet<u64>,
    pub volume: f32,
    pub self_deafen: bool,
    pub timeout: u32,
    pub welcome_settings: Option<WelcomeSettings>,
    pub log_settings: Option<LogSettings>,
}

impl GuildSettings {
    pub fn new(guild_id: GuildId, prefix: Option<&str>) -> GuildSettings {
        let allowed_domains: HashSet<String> = DEFAULT_ALLOWED_DOMAINS
            .iter()
            .map(|d| d.to_string())
            .collect();

        let my_prefix = match prefix {
            Some(p) => p.to_string(),
            None => DEFAULT_PREFIX.to_string(),
        };

        GuildSettings {
            guild_id,
            prefix: my_prefix.clone(),
            prefix_up: my_prefix.to_string().to_ascii_uppercase(),
            autopause: false,
            allow_all_domains: Some(DEFAULT_ALLOW_ALL_DOMAINS),
            allowed_domains,
            banned_domains: HashSet::new(),
            authorized_users: HashSet::new(),
            volume: DEFAULT_VOLUME_LEVEL,
            self_deafen: true,
            timeout: DEFAULT_IDLE_TIMEOUT,
            welcome_settings: None,
            log_settings: None,
        }
    }

    pub fn load_if_exists(&mut self) -> Result<(), CrackedError> {
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);
        if !Path::new(&path).exists() {
            return Ok(());
        }
        self.load()
    }

    pub fn load(&mut self) -> Result<(), CrackedError> {
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);
        let file = OpenOptions::new().read(true).open(path)?;
        let reader = BufReader::new(file);
        *self = serde_json::from_reader::<_, GuildSettings>(reader)?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), CrackedError> {
        tracing::warn!("Saving guild settings: {:?}", self);
        create_dir_all(SETTINGS_PATH.as_str())?;
        let path = format!("{}/{}.json", SETTINGS_PATH.as_str(), self.guild_id);

        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)?;

        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, self)?;
        Ok(())
    }

    pub fn toggle_autopause(&mut self) {
        self.autopause = !self.autopause;
    }

    pub fn toggle_self_deafen(&mut self) {
        self.self_deafen = !self.self_deafen;
    }

    pub fn set_allowed_domains(&mut self, allowed_str: &str) {
        let allowed = allowed_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.allowed_domains = allowed;
    }

    pub fn set_banned_domains(&mut self, banned_str: &str) {
        let banned = banned_str
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        self.banned_domains = banned;
    }

    pub fn update_domains(&mut self) {
        if !self.allowed_domains.is_empty() && !self.banned_domains.is_empty() {
            self.banned_domains.clear();
        }

        if self.allowed_domains.is_empty() && self.banned_domains.is_empty() {
            self.allowed_domains.insert(String::from("youtube.com"));
            self.banned_domains.clear();
        }
    }

    pub fn authorize_user(&mut self, user_id: u64) {
        if !self.authorized_users.contains(&user_id) {
            self.authorized_users.insert(user_id);
        }
    }

    pub fn deauthorize_user(&mut self, user_id: u64) {
        if self.authorized_users.contains(&user_id) {
            self.authorized_users.remove(&user_id);
        }
    }

    pub fn check_authorized(&self, user_id: u64) -> bool {
        self.authorized_users.contains(&user_id)
    }

    pub fn check_authorized_user_id(&self, user_id: UserId) -> bool {
        self.authorized_users.contains(&user_id.0)
    }

    pub fn set_default_volume(&mut self, volume: f32) {
        self.volume = volume;
    }

    pub fn set_allow_all_domains(&mut self, allow: bool) {
        self.allow_all_domains = Some(allow);
    }

    pub fn set_timeout(&mut self, timeout: u32) {
        self.timeout = timeout;
    }

    pub fn set_welcome_settings(&mut self, channel_id: u64, message: &str) {
        self.welcome_settings = Some(WelcomeSettings {
            channel_id: Some(channel_id),
            message: Some(message.to_string()),
            auto_role: None,
        });
    }

    pub fn set_log_settings(&mut self, all_log_channel: u64, join_leave_log_channel: u64) {
        self.log_settings = Some(LogSettings {
            all_log_channel: Some(all_log_channel),
            raw_event_log_channel: None,
            server_log_channel: None,
            member_log_channel: None,
            join_leave_log_channel: Some(join_leave_log_channel),
            voice_log_channel: None,
        });
    }

    pub fn set_auto_role(&mut self, auto_role: u64) {
        if let Some(welcome_settings) = &mut self.welcome_settings {
            welcome_settings.auto_role = Some(auto_role);
        }
    }

    pub fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
        self.prefix_up = prefix.to_string().to_ascii_uppercase();
    }

    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    pub fn set_all_log_channel(&mut self, channel_id: u64) {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.all_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_all_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
    }

    pub fn set_join_leave_log_channel(&mut self, channel_id: u64) {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.join_leave_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_join_leave_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
    }

    pub fn get_log_channel_type(&self, event: &poise::Event<'_>) -> Option<ChannelId> {
        if let Some(log_settings) = &self.log_settings {
            match event {
                poise::Event::GuildBanRemoval { .. }
                | poise::Event::GuildMemberAddition { .. }
                | poise::Event::GuildMemberRemoval { .. }
                | poise::Event::GuildScheduledEventCreate { .. }
                | poise::Event::GuildScheduledEventUpdate { .. }
                | poise::Event::GuildScheduledEventDelete { .. }
                | poise::Event::GuildScheduledEventUserAdd { .. }
                | poise::Event::GuildScheduledEventUserRemove { .. }
                | poise::Event::GuildStickersUpdate { .. }
                | poise::Event::GuildBanAddition { .. }
                | poise::Event::GuildCreate { .. }
                | poise::Event::GuildDelete { .. }
                | poise::Event::GuildEmojisUpdate { .. }
                | poise::Event::GuildIntegrationsUpdate { .. }
                | poise::Event::GuildMemberUpdate { .. }
                | poise::Event::GuildMembersChunk { .. }
                | poise::Event::GuildRoleCreate { .. }
                | poise::Event::GuildRoleDelete { .. }
                | poise::Event::GuildRoleUpdate { .. }
                | poise::Event::GuildUnavailable { .. }
                | poise::Event::GuildUpdate { .. } => log_settings
                    .get_server_log_channel()
                    .or(log_settings.get_all_log_channel()),
                poise::Event::Message { .. }
                | poise::Event::PresenceReplace { .. }
                | poise::Event::Resume { .. }
                | poise::Event::ShardStageUpdate { .. }
                | poise::Event::WebhookUpdate { .. }
                | poise::Event::ApplicationCommandPermissionsUpdate { .. }
                | poise::Event::AutoModerationActionExecution { .. }
                | poise::Event::AutoModerationRuleCreate { .. }
                | poise::Event::AutoModerationRuleUpdate { .. }
                | poise::Event::AutoModerationRuleDelete { .. }
                | poise::Event::CacheReady { .. }
                | poise::Event::ChannelCreate { .. }
                | poise::Event::CategoryCreate { .. }
                | poise::Event::CategoryDelete { .. }
                | poise::Event::ChannelDelete { .. }
                | poise::Event::ChannelPinsUpdate { .. }
                | poise::Event::ChannelUpdate { .. }
                | poise::Event::IntegrationCreate { .. }
                | poise::Event::IntegrationUpdate { .. }
                | poise::Event::IntegrationDelete { .. }
                | poise::Event::InteractionCreate { .. }
                | poise::Event::InviteCreate { .. }
                | poise::Event::InviteDelete { .. }
                | poise::Event::MessageDelete { .. }
                | poise::Event::MessageDeleteBulk { .. }
                | poise::Event::MessageUpdate { .. }
                | poise::Event::ReactionAdd { .. }
                | poise::Event::ReactionRemove { .. }
                | poise::Event::ReactionRemoveAll { .. }
                | poise::Event::PresenceUpdate { .. }
                | poise::Event::Ready { .. }
                | poise::Event::StageInstanceCreate { .. }
                | poise::Event::StageInstanceDelete { .. }
                | poise::Event::StageInstanceUpdate { .. }
                | poise::Event::ThreadCreate { .. }
                | poise::Event::ThreadDelete { .. }
                | poise::Event::ThreadListSync { .. }
                | poise::Event::ThreadMemberUpdate { .. }
                | poise::Event::ThreadMembersUpdate { .. }
                | poise::Event::ThreadUpdate { .. }
                | poise::Event::TypingStart { .. }
                | poise::Event::Unknown { .. }
                | poise::Event::UserUpdate { .. }
                | poise::Event::VoiceServerUpdate { .. }
                | poise::Event::VoiceStateUpdate { .. } => log_settings.get_all_log_channel(),
                _ => todo!(),
            }
        } else {
            None
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
                return Some(ChannelId(channel_id));
            }
        }
        None
    }
}

pub struct GuildSettingsMap;

impl TypeMapKey for GuildSettingsMap {
    type Value = HashMap<GuildId, GuildSettings>;
}

pub struct Float;

impl TypeMapKey for Float {
    type Value = f32;
}
