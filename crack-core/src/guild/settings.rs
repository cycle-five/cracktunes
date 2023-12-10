use self::serenity::model::id::GuildId;
use self::serenity::model::prelude::UserId;
use crate::db::WelcomeSettingsRead;
use crate::errors::CrackedError;
use lazy_static::lazy_static;
use poise::serenity_prelude::{self as serenity, ChannelId, FullEvent};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt::{Display, Formatter};
use std::io::Write;
use std::sync::{atomic, Arc};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs::{create_dir_all, OpenOptions},
    io::BufReader,
    path::Path,
};
use tokio::sync::{Mutex, RwLock};
use typemap_rev::TypeMapKey;

pub(crate) const DEFAULT_ALLOW_ALL_DOMAINS: bool = true;
pub(crate) const DEFAULT_SETTINGS_PATH: &str = "data/settings";
pub(crate) const DEFAULT_ALLOWED_DOMAINS: [&str; 1] = ["youtube.com"];
pub(crate) const DEFAULT_VOLUME_LEVEL: f32 = 1.0;
pub(crate) const DEFAULT_VIDEO_STATUS_POLL_INTERVAL: u64 = 120;
pub(crate) const DEFAULT_PREFIX: &str = "r!";
pub(crate) const DEFAULT_DB_URL: &str = "postgres:///data/crackedmusic.db";
pub(crate) const DEFAULT_IDLE_TIMEOUT: u32 = 0; //5 * 60;
pub(crate) const DEFAULT_LYRICS_PAGE_SIZE: usize = 1024;
pub(crate) const DEFAULT_PREMIUM: bool = false;
pub(crate) const ADDITIONAL_PREFIXES: [&str; 10] = [
    "hey bot,", "hey bot", "bot,", "bot", "!play", "!music", "!youtube", "!yt", "m/", "M/",
];

lazy_static! {
    static ref SETTINGS_PATH: String =
        env::var("SETTINGS_PATH").unwrap_or(DEFAULT_SETTINGS_PATH.to_string());
}

#[derive(Serialize, Deserialize)]
pub struct SerializableData {
    #[serde(skip)]
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>,
}

impl SerializableData {
    pub fn serialize_inner(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&*self.guild_settings_map.read().unwrap())
    }

    pub fn deserialize_inner(serialized_data: &str) -> Result<Self, serde_json::Error> {
        let map: HashMap<GuildId, GuildSettings> = serde_json::from_str(serialized_data)?;
        Ok(Self {
            guild_settings_map: Arc::new(RwLock::new(map)),
        })
    }
}

#[derive(Default, Deserialize, Serialize, Debug, Clone)]
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

const DEFAULT_LOG_CHANNEL: u64 = 1165246445654388746;

impl LogSettings {
    pub fn get_all_log_channel(&self) -> Option<ChannelId> {
        self.all_log_channel
            .map(ChannelId::new)
            .or(Some(ChannelId::new(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_server_log_channel(&self) -> Option<ChannelId> {
        self.server_log_channel
            .map(ChannelId::new)
            .or(Some(ChannelId::new(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_join_leave_log_channel(&self) -> Option<ChannelId> {
        self.join_leave_log_channel
            .map(ChannelId::new)
            .or(Some(ChannelId::new(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_member_log_channel(&self) -> Option<ChannelId> {
        self.member_log_channel
            .map(ChannelId::new)
            .or(Some(ChannelId::new(DEFAULT_LOG_CHANNEL)))
    }

    pub fn get_voice_log_channel(&self) -> Option<ChannelId> {
        self.voice_log_channel
            .map(ChannelId::new)
            .or(Some(ChannelId::new(DEFAULT_LOG_CHANNEL)))
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

#[derive(Deserialize, Serialize, Default, Debug, Clone)]
pub struct WelcomeSettings {
    pub channel_id: Option<u64>,
    pub message: Option<String>,
    pub auto_role: Option<u64>,
}

impl From<WelcomeSettingsRead> for WelcomeSettings {
    fn from(settings_db: WelcomeSettingsRead) -> Self {
        WelcomeSettings {
            channel_id: settings_db.channel_id.map(|x| x as u64),
            message: settings_db.message,
            auto_role: settings_db.auto_role.map(|x| x as u64),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct GuildSettings {
    pub guild_id: GuildId,
    pub guild_name: String,
    pub prefix: String,
    #[serde(default = "premium_default")]
    pub premium: bool,
    pub autopause: bool,
    pub allow_all_domains: Option<bool>,
    pub allowed_domains: HashSet<String>,
    pub banned_domains: HashSet<String>,
    pub authorized_users: HashSet<u64>,
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
    // #[serde(skip)]
    // pub db_pool: Option<Arc<RwLock<Pin<Box<PgPool>>>>>,
}

fn additional_prefixes_default() -> Vec<String> {
    Vec::<String>::new()
}

fn volume_default() -> f32 {
    DEFAULT_VOLUME_LEVEL
}

fn premium_default() -> bool {
    DEFAULT_PREMIUM
}

impl Display for GuildSettings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GuildSettings {{ guild_id: {}, guild_name: {}, prefix: {}, premium: {}, 
                autopause: {}, allow_all_domains: {}, allowed_domains: {:?}, banned_domains: {:?}, 
                authorized_users: {:?}, ignored_channels: {:?}, old_volume: {}, volume: {}, 
                self_deafen: {}, timeout: {}, welcome_settings: {:?}, log_settings: {:?} }}",
            self.guild_id,
            self.guild_name,
            self.prefix,
            self.premium,
            self.autopause,
            self.allow_all_domains.unwrap_or(true),
            self.allowed_domains,
            self.banned_domains,
            self.authorized_users,
            self.ignored_channels,
            self.old_volume,
            self.volume,
            self.self_deafen,
            self.timeout,
            self.welcome_settings,
            self.log_settings,
        )
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
        settings.allow_all_domains = Some(settings_db.allow_all_domains);
        settings.allowed_domains = settings_db.allowed_domains.into_iter().collect();
        settings.banned_domains = settings_db.banned_domains.into_iter().collect();
        settings.authorized_users = settings_db
            .authorized_users
            .into_iter()
            .map(|x| x as u64)
            .collect();
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
            autopause: false,
            allow_all_domains: Some(DEFAULT_ALLOW_ALL_DOMAINS),
            allowed_domains,
            banned_domains: HashSet::new(),
            authorized_users: HashSet::new(),
            ignored_channels: asdf.into_iter().collect(),
            old_volume: DEFAULT_VOLUME_LEVEL,
            volume: DEFAULT_VOLUME_LEVEL,
            self_deafen: true,
            timeout: DEFAULT_IDLE_TIMEOUT,
            welcome_settings: None,
            log_settings: None,
            additional_prefixes: Vec::new(),
            // db_pool: Some(Arc::new(
            //     sqlx::PgPool::connect_lazy(DEFAULT_DB_URL).unwrap(),
            // )),
        }
    }

    pub fn get_prefix_up(&self) -> String {
        self.prefix.to_ascii_uppercase()
    }

    pub async fn load_or_create(
        &mut self,
        pool: Arc<Mutex<PgPool>>,
        guild_id: i64,
        name: String,
    ) -> Result<GuildSettings, CrackedError> {
        let guild = crate::db::GuildEntity::get_or_create(pool, guild_id, name).await?;
        let mut settings = guild.get_settings(pool.clone()).await?;
        let welcome_settings = guild.get_welcome_settings(pool.clone()).await?;
        let log_settings = guild.get_log_settings(pool.clone()).await?;
        settings.welcome_settings = welcome_settings;
        settings.log_settings = log_settings;
        Ok(settings)
    }

    pub fn load_if_exists(&mut self) -> Result<(), CrackedError> {
        let path = format!(
            "{}/{}-{}.json",
            SETTINGS_PATH.as_str(),
            self.guild_name.to_ascii_lowercase(),
            self.guild_id,
        );
        if !Path::new(&path).exists() {
            return Ok(());
        }
        self.load()
    }

    pub fn load(&mut self) -> Result<(), CrackedError> {
        let path = format!(
            "{}/{}-{}.json",
            SETTINGS_PATH.as_str(),
            self.get_guild_name(),
            self.guild_id,
        );
        let file = OpenOptions::new().read(true).open(path)?;
        let reader = BufReader::new(file);
        let mut loaded_guild = serde_json::from_reader::<_, GuildSettings>(reader)?;
        loaded_guild.guild_name = self.guild_name.clone();
        *self = loaded_guild;
        Ok(())
    }

    pub async fn save(&self, pool: Arc<Mutex<PgPool>>) -> Result<(), CrackedError> {
        tracing::warn!("Saving guild settings: {:?}", self);
        let guild_id = self.guild_id.get() as i64;
        let guild_name = self.guild_name.clone();
        let guild =
            crate::db::GuildEntity::get_or_create(pool.clone(), guild_id, guild_name.clone())
                .await
                .unwrap();
        let _ = guild.write_settings(pool.clone(), self).await;
        Ok(())
    }

    pub fn old_save(&self) -> Result<(), CrackedError> {
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
        self.authorized_users.contains(&user_id.into())
    }

    pub fn set_volume(&mut self, volume: f32) -> &mut Self {
        self.old_volume = self.volume;
        self.volume = volume;
        self
    }

    pub fn set_allow_all_domains(&mut self, allow: bool) -> &mut Self {
        self.allow_all_domains = Some(allow);
        self
    }

    pub fn set_timeout(&mut self, timeout: u32) -> &mut Self {
        self.timeout = timeout;
        self
    }

    pub fn set_welcome_settings(&mut self, welcome_settings: WelcomeSettings) -> &mut Self {
        self.welcome_settings = Some(welcome_settings);
        self
    }

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
        });
        self
    }

    pub fn set_welcome_settings3(&mut self, channel_id: u64, message: String) -> &mut Self {
        self.welcome_settings = Some(WelcomeSettings {
            channel_id: Some(channel_id),
            message: Some(message.to_string()),
            auto_role: self
                .welcome_settings
                .clone()
                .map(|x| x.auto_role)
                .unwrap_or_default(),
        });
        self
    }

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

    pub fn set_prefix(&mut self, prefix: &str) {
        self.prefix = prefix.to_string();
        // self.prefix_up = self.prefix.to_string().to_ascii_uppercase();
    }

    pub fn set_default_additional_prefixes(&mut self) -> &mut Self {
        self.additional_prefixes = ADDITIONAL_PREFIXES
            .to_vec()
            .iter()
            .map(|x| x.to_string())
            .collect();
        self
    }

    pub fn set_ignored_channels(&mut self, ignored_channels: HashSet<u64>) -> &mut Self {
        self.ignored_channels = ignored_channels;
        self
    }

    pub fn get_guild_name(&self) -> String {
        if self.guild_name.is_empty() {
            self.guild_id.to_string().to_ascii_lowercase()
        } else {
            self.guild_name.to_ascii_lowercase()
        }
    }

    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

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

    pub fn set_join_leave_log_channel(&mut self, channel_id: u64) {
        if let Some(log_settings) = &mut self.log_settings {
            log_settings.join_leave_log_channel = Some(channel_id);
        } else {
            let mut log_settings = LogSettings::default();
            log_settings.set_join_leave_log_channel(channel_id);
            self.log_settings = Some(log_settings);
        }
    }

    pub fn get_log_channel_type_fe(&self, event: &FullEvent) -> Option<ChannelId> {
        let log_settings = self.log_settings.clone().unwrap_or_default();
        match event {
            FullEvent::GuildBanRemoval { .. }
            | FullEvent::GuildMemberAddition { .. }
            | FullEvent::GuildMemberRemoval { .. }
            | FullEvent::GuildScheduledEventCreate { .. }
            | FullEvent::GuildScheduledEventUpdate { .. }
            | FullEvent::GuildScheduledEventDelete { .. }
            | FullEvent::GuildScheduledEventUserAdd { .. }
            | FullEvent::GuildScheduledEventUserRemove { .. }
            | FullEvent::GuildStickersUpdate { .. }
            | FullEvent::GuildBanAddition { .. }
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
            | FullEvent::PresenceReplace { .. }
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
            | FullEvent::PresenceUpdate { .. }
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

pub async fn save_guild_settings(
    guild_settings_map: &HashMap<GuildId, GuildSettings>,
    pool: &PgPool,
) {
    for guild_settings in guild_settings_map.values() {
        let _ = guild_settings.save(pool).await;
    }
}

// use self::serenity::{Context as SerenityContext, EventHandler};
// pub async fn load_guilds_settings(
//     ctx: &SerenityContext,
//     guilds: &[UnavailableGuild],
//     data_new: &Data,
// ) -> HashMap<GuildId, GuildSettings> {
//     let prefix = data_new.bot_settings.get_prefix();
//     tracing::info!("Loading guilds' settings");
//     let mut data = ctx.data.write().await;
//     let settings = match data.get_mut::<GuildSettingsMap>() {
//         Some(settings) => settings,
//         None => {
//             tracing::error!("Guild settings not found");
//             data.insert::<GuildSettingsMap>(HashMap::default());
//             data.get_mut::<GuildSettingsMap>().unwrap()
//         }
//     };
//     for guild in guilds {
//         let guild_id = guild.id;
//         let guild_full = match guild_id.to_guild_cached(&ctx.cache) {
//             Some(guild_match) => guild_match,
//             None => {
//                 tracing::error!("Guild not found in cache");
//                 continue;
//             }
//         };
//         tracing::info!(
//             "Loading guild settings for {}, {}",
//             guild_full.id,
//             guild_full.name.clone()
//         );

//         let mut default =
//             GuildSettings::new(guild_full.id, Some(&prefix), Some(guild_full.name.clone()));

//         let _ = default.load_if_exists().map_err(|err| {
//             tracing::error!(
//                 "Failed to load guild {} settings due to {}",
//                 default.guild_id,
//                 err
//             );
//         });

//         tracing::warn!("GuildSettings: {:?}", default);

//         let _ = settings.insert(default.guild_id, default.clone());

//         let guild_settings = settings.get(&default.guild_id);

//         guild_settings
//             .map(|x| {
//                 tracing::info!("saving guild {}...", x);
//                 x.save().expect("Error saving guild settings");
//                 x
//             })
//             .or_else(|| {
//                 tracing::error!("Guild not found in settings map");
//                 None
//             });
//     }
//     let data_read = ctx.data.read().await;
//     let guild_settings_map_read = data_read.get::<GuildSettingsMap>().unwrap().clone();
//     guild_settings_map_read
//     // .get::<GuildSettingsMap>()
//     // .unwrap()
//     // .clone()
// }

#[derive(Default)]
pub struct GuildSettingsMap;

impl TypeMapKey for GuildSettingsMap {
    type Value = HashMap<GuildId, GuildSettings>;
}

#[derive(Default)]
pub struct AtomicU16Key;

impl TypeMapKey for AtomicU16Key {
    type Value = Arc<atomic::AtomicU16>;
}

// impl GuildSettingsMap {
//     pub fn save(self) {
//         let data = self.0;
//         for (_, guild_settings) in data {
//             let _ = guild_settings.save();
//         }
//     }
// }
