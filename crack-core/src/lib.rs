#![allow(internal_features)]
#![feature(fmt_internals)]
pub mod commands;
pub mod config;
pub mod connection;
pub mod db;
pub mod errors;
pub mod guild;
pub mod handlers;
pub mod http_utils;
#[macro_use]
pub mod macros;
pub mod messaging;
// pub mod metrics;
#[cfg(feature = "crack-music")]
pub mod music;
pub mod poise_ext;
pub mod sources;
#[cfg(test)]
pub mod test;
pub mod utils;

//#![feature(linked_list_cursors)]
use crate::handlers::event_log::LogEntry;
#[cfg(feature = "crack-activity")]
use ::serenity::all::Activity;
use chrono::{DateTime, Utc};
#[cfg(feature = "crack-gpt")]
use crack_gpt::GptContext;
use crack_testing::CrackTrackClient;
use db::worker_pool::MetadataMsg;
use db::{PlayLog, TrackReaction};
use errors::CrackedError;
use guild::settings::get_log_prefix;
use guild::settings::{GuildSettings, GuildSettingsMapParam};
use guild::settings::{
    DEFAULT_DB_URL, DEFAULT_LOG_PREFIX, DEFAULT_PREFIX, DEFAULT_VIDEO_STATUS_POLL_INTERVAL,
    DEFAULT_VOLUME_LEVEL,
};
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use serenity::all::{GuildId, Message, UserId};
use songbird::Songbird;
// use std::sync::atomic::AtomicU16;
use std::time::SystemTime;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Display,
    fs,
    fs::File,
    io::Write,
    path::Path,
    sync::Arc,
};
use tokio::sync::{mpsc::Sender, Mutex, RwLock};

// ------------------------------------------------------------------
// Our public types used throughout cracktunes.
// Probably want to move these to crack-types...
// ------------------------------------------------------------------

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type ArcTRwLock<T> = Arc<tokio::sync::RwLock<T>>;
pub type ArcTMutex<T> = Arc<tokio::sync::Mutex<T>>;
pub type ArcRwMap<K, V> = Arc<std::sync::RwLock<HashMap<K, V>>>;
pub type ArcTRwMap<K, V> = Arc<tokio::sync::RwLock<HashMap<K, V>>>;
pub type ArcMutDMap<K, V> = Arc<tokio::sync::Mutex<HashMap<K, V>>>;
pub type CrackedResult<T> = std::result::Result<T, CrackedError>;
pub type CrackedHowResult<T> = anyhow::Result<T, CrackedError>;

pub type Command = poise::Command<Data, CommandError>;
pub type Context<'a> = poise::Context<'a, Data, CommandError>;
pub type PrefixContext<'a> = poise::PrefixContext<'a, Data, CommandError>;
pub type PartialContext<'a> = poise::PartialContext<'a, Data, CommandError>;
pub type ApplicationContext<'a> = poise::ApplicationContext<'a, Data, CommandError>;

pub type CommandError = Error;
pub type CommandResult<E = Error> = Result<(), E>;
pub type FrameworkContext<'a> = poise::FrameworkContext<'a, Data, CommandError>;

use crate::messaging::message::CrackedMessage;
// use crate::serenity::prelude::SerenityError;
use crack_types::reply_handle::MessageOrReplyHandle;

/// Struct for the cammed down kicking configuration.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CamKickConfig {
    pub timeout: u64,
    pub guild_id: u64,
    pub chan_id: u64,
    pub dc_msg: String,
    pub msg_on_deafen: bool,
    pub msg_on_mute: bool,
    pub msg_on_dc: bool,
}

/// Default for the CamKickConfig.
impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            timeout: 0,
            guild_id: 0,
            chan_id: 0,
            // FIXME: This should be a const or a static
            dc_msg: "You have been violated for being cammed down for too long.".to_string(),
            msg_on_deafen: false,
            msg_on_mute: false,
            msg_on_dc: false,
        }
    }
}

/// Display impl for CamKickConfig
impl Display for CamKickConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("timeout:       {:?}\n", self.timeout));
        result.push_str(&format!("guild_id:      {:?}\n", self.guild_id));
        result.push_str(&format!("chan_id:       {:?}\n", self.chan_id));
        result.push_str(&format!("dc_msg:        {:?}\n", self.dc_msg));
        result.push_str(&format!("msg_on_deafen: {}\n", self.msg_on_deafen));
        result.push_str(&format!("msg_on_mute:   {}\n", self.msg_on_mute));
        result.push_str(&format!("msg_on_dc:     {}\n", self.msg_on_dc));

        write!(f, "{}", result)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotCredentials {
    pub discord_token: String,
    pub discord_app_id: String,
    pub spotify_client_id: Option<String>,
    pub spotify_client_secret: Option<String>,
    pub openai_api_key: Option<String>,
    pub virustotal_api_key: Option<String>,
}

impl Default for BotCredentials {
    fn default() -> Self {
        Self {
            discord_token: "XXXX".to_string(),
            discord_app_id: "XXXX".to_string(),
            spotify_client_id: None,
            spotify_client_secret: None,
            openai_api_key: None,
            virustotal_api_key: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotConfig {
    pub video_status_poll_interval: Option<u64>,
    // TODO: Get rid of this, it's redundent with the owners in the serenity library.
    pub owners: Option<Vec<u64>>,
    // Cammed down kicking config
    pub cam_kick: Option<Vec<CamKickConfig>>,
    pub sys_log_channel_id: Option<u64>,
    pub self_deafen: Option<bool>,
    pub volume: Option<f32>,
    #[serde(skip)]
    pub guild_settings_map: Option<Vec<guild::settings::GuildSettings>>,
    pub prefix: Option<String>,
    pub credentials: Option<BotCredentials>,
    pub database_url: Option<String>,
    pub log_prefix: Option<String>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: Some(DEFAULT_VIDEO_STATUS_POLL_INTERVAL),
            owners: None,
            cam_kick: None,
            sys_log_channel_id: None,
            self_deafen: Some(true),
            volume: Some(DEFAULT_VOLUME_LEVEL),
            guild_settings_map: None,
            prefix: Some(DEFAULT_PREFIX.to_string()),
            credentials: Some(BotCredentials::default()),
            database_url: Some(DEFAULT_DB_URL.to_string()),
            log_prefix: Some(DEFAULT_LOG_PREFIX.to_string()),
        }
    }
}

impl Display for BotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!(
            "video_status_poll_interval: {:?}\n",
            self.video_status_poll_interval
        ));
        result.push_str(&format!("owners: {:?}\n", self.owners));
        result.push_str(&format!("cam_kick: {:?}\n", self.cam_kick));
        result.push_str(&format!(
            "sys_log_channel_id: {:?}\n",
            self.sys_log_channel_id
        ));
        result.push_str(&format!("self_deafen: {:?}\n", self.self_deafen));
        result.push_str(&format!("volume: {:?}\n", self.volume));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!(
            "prefix: {}",
            self.prefix
                .as_ref()
                .cloned()
                .unwrap_or(DEFAULT_PREFIX.to_string())
        ));
        result.push_str(&format!("credentials: {:?}\n", self.credentials.is_some()));
        result.push_str(&format!("database_url: {:?}\n", self.database_url));
        result.push_str(&format!("log_prefix: {:?}\n", self.log_prefix));
        write!(f, "{}", result)
    }
}

impl BotConfig {
    pub fn set_credentials(&mut self, creds: BotCredentials) -> &mut Self {
        self.credentials = Some(creds);
        self
    }

    pub fn get_prefix(&self) -> String {
        self.prefix.clone().unwrap_or(DEFAULT_PREFIX.to_string())
    }

    pub fn get_video_status_poll_interval(&self) -> u64 {
        self.video_status_poll_interval
            .unwrap_or(DEFAULT_VIDEO_STATUS_POLL_INTERVAL)
    }

    pub fn get_database_url(&self) -> String {
        self.database_url
            .as_ref()
            .cloned()
            .unwrap_or(DEFAULT_DB_URL.to_string())
    }
}

/// Phone code data for the osint commands
#[derive(Default, Debug, Clone)]
pub struct PhoneCodeData {
    #[allow(dead_code)]
    phone_codes: HashMap<String, String>,
    #[allow(dead_code)]
    country_names: HashMap<String, String>,
    country_by_phone_code: HashMap<String, Vec<String>>,
}

/// impl of PhoneCodeData
impl PhoneCodeData {
    /// Load the phone code data from the local file, or download it if it doesn't exist
    pub fn load() -> Result<Self, CrackedError> {
        let phone_codes = Self::load_data("./data/phone.json", "http://country.io/phone.json")?;
        let country_names = Self::load_data("./data/names.json", "http://country.io/names.json")?;
        let country_by_phone_code = phone_codes
            .iter()
            .map(|(k, v)| (v.clone(), k.clone()))
            .fold(
                HashMap::new(),
                |mut acc: HashMap<String, Vec<String>>, (k, v)| {
                    acc.entry(k).or_default().push(v);
                    acc
                },
            );
        Ok(Self {
            phone_codes,
            country_names,
            country_by_phone_code,
        })
    }

    /// Load the data from the local file, or download it if it doesn't exist
    fn load_data(file_name: &str, url: &str) -> Result<HashMap<String, String>, CrackedError> {
        match fs::read_to_string(file_name) {
            Ok(contents) => serde_json::from_str(&contents).map_err(CrackedError::Json),
            Err(_) => Self::download_and_parse(url, file_name),
        }
    }

    /// Download the data from the url and parse it. Internally used.
    fn download_and_parse(
        url: &str,
        file_name: &str,
    ) -> Result<HashMap<String, String>, CrackedError> {
        let client = reqwest::blocking::ClientBuilder::new()
            .use_rustls_tls()
            .cookie_store(true)
            .build()?;
        //let client = crate::http_utils::get_client();
        let response = client.get(url).send().map_err(CrackedError::Reqwest)?;
        let content = response.text().map_err(CrackedError::Reqwest)?;

        // Save to local file
        fs::create_dir_all(Path::new(file_name).parent().unwrap()).map_err(CrackedError::IO)?;
        let mut file = fs::File::create(file_name).map_err(CrackedError::IO)?;
        file.write_all(content.as_bytes())
            .map_err(CrackedError::IO)?;

        serde_json::from_str(&content).map_err(CrackedError::Json)
    }

    /// Get names of countries that match the given phone code.
    /// Due to edge cases, there may be multiples.
    pub fn get_countries_by_phone_code(&self, phone_code: &str) -> Option<Vec<String>> {
        self.country_by_phone_code.get(phone_code).cloned()
    }
}

/// User data, which is stored and accessible in all command invocations
#[derive(Clone)]
pub struct DataInner {
    pub bot_settings: BotConfig,
    pub start_time: SystemTime,
    #[cfg(feature = "crack-activity")]
    pub user_activity_map: Arc<dashmap::DashMap<UserId, Activity>>,
    #[cfg(feature = "crack-activity")]
    pub activity_user_map: Arc<dashmap::DashMap<String, dashmap::DashSet<UserId>>>,
    pub authorized_users: HashSet<u64>,
    pub join_vc_tokens: dashmap::DashMap<serenity::GuildId, Arc<tokio::sync::Mutex<()>>>,
    pub phone_data: PhoneCodeData,
    pub event_log_async: EventLogAsync,
    pub db_channel: Option<Sender<MetadataMsg>>,
    pub database_pool: Option<sqlx::PgPool>,
    pub http_client: reqwest::Client,
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, guild::settings::GuildSettings>>>,
    pub guild_cache_map: Arc<Mutex<HashMap<GuildId, guild::cache::GuildCache>>>,
    pub guild_msg_cache_ordered: Arc<Mutex<BTreeMap<GuildId, guild::cache::GuildCache>>>,
    pub guild_command_msg_queue: dashmap::DashMap<GuildId, Vec<MessageOrReplyHandle>>,
    pub guild_cnt_map: dashmap::DashMap<GuildId, u64>,
    #[cfg(feature = "crack-gpt")]
    pub gpt_ctx: Arc<RwLock<Option<GptContext>>>,
    pub ct_client: CrackTrackClient<'static>,
    pub songbird: Arc<Songbird>,
}

// /// Get the default topgg client
// fn default_topgg_client() -> topgg::Client {
//     topgg::Client::new(std::env::var("TOPGG_TOKEN").unwrap_or_default())
// }

impl std::fmt::Debug for DataInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("phone_data: {:?}\n", self.phone_data));
        result.push_str(&format!("bot_settings: {:?}\n", self.bot_settings));
        result.push_str(&format!("authorized_users: {:?}\n", self.authorized_users));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!(
            "guild_msg_cache_ordered: {:?}\n",
            self.guild_msg_cache_ordered
        ));
        result.push_str(&format!("guild_cache_map: {:?}\n", self.guild_cache_map));
        result.push_str(&format!("event_log: {:?}\n", self.event_log_async));
        result.push_str(&format!("database_pool: {:?}\n", self.database_pool));
        #[cfg(feature = "crack-gpt")]
        result.push_str(&format!("gpt_context: {:?}\n", self.gpt_ctx));
        result.push_str(&format!("http_client: {:?}\n", self.http_client));
        result.push_str("topgg_client: <skipped>\n");
        write!(f, "{}", result)
    }
}

impl DataInner {
    /// Set the bot settings for the data.
    pub fn with_bot_settings(&self, bot_settings: BotConfig) -> Self {
        Self {
            bot_settings,
            ..self.clone()
        }
    }

    /// Set the database pool for the data.
    pub fn with_database_pool(&self, database_pool: sqlx::PgPool) -> Self {
        Self {
            database_pool: Some(database_pool),
            ..self.clone()
        }
    }

    /// Set the channel for the database pool communication.
    pub fn with_db_channel(&self, db_channel: Sender<MetadataMsg>) -> Self {
        Self {
            db_channel: Some(db_channel),
            ..self.clone()
        }
    }

    /// Set the GPT context for the data.
    #[cfg(feature = "crack-gpt")]
    pub fn with_gpt_ctx(&self, gpt_ctx: GptContext) -> Self {
        Self {
            gpt_ctx: Arc::new(RwLock::new(Some(gpt_ctx))),
            ..self.clone()
        }
    }

    pub fn with_ct_client(&self, ct_client: CrackTrackClient<'static>) -> Self {
        Self {
            ct_client,
            ..self.clone()
        }
    }

    pub fn with_songbird(&self, songbird: Arc<songbird::Songbird>) -> Self {
        Self {
            songbird,
            ..self.clone()
        }
    }

    /// Set the guild settings map for the data.
    pub fn with_guild_settings_map(&self, guild_settings: GuildSettingsMapParam) -> Self {
        Self {
            guild_settings_map: guild_settings,
            ..self.clone()
        }
    }
}

/// General log for events that the bot reveices from Discord.
#[derive(Clone, Debug)]
pub struct EventLogAsync(pub ArcTMutex<File>);

impl std::ops::Deref for EventLogAsync {
    type Target = ArcTMutex<File>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for EventLogAsync {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for EventLogAsync {
    fn default() -> Self {
        let log_path = format!("{}/events2.log", get_log_prefix());
        let _ = fs::create_dir_all(Path::new(&log_path).parent().unwrap());
        let log_file = match File::create(log_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error creating log file: {}", e);
                // FIXME: Maybe use io::null()?
                // I went down this path with sink and it was a mistake.
                File::create("/dev/null")
                    .expect("Should be able to have a file object to write too.")
            },
        };
        Self(Arc::new(tokio::sync::Mutex::new(log_file)))
    }
}

impl EventLogAsync {
    /// Create a new EventLog, calls default
    pub fn new() -> Self {
        Self::default()
    }

    /// Write an object to the log file without a note async.
    pub async fn write_log_obj_async<T: serde::Serialize>(
        &self,
        name: &str,
        obj: &T,
    ) -> Result<(), Error> {
        self.write_log_obj_note_async(name, None, obj).await
    }

    /// Write an object to the log file with a note.
    pub async fn write_log_obj_note_async<T: serde::Serialize>(
        &self,
        name: &str,
        notes: Option<&str>,
        obj: &T,
    ) -> Result<(), Error> {
        let entry = LogEntry {
            name: name.to_string(),
            notes: notes.unwrap_or("").to_string(),
            event: obj,
        };
        let mut buf = serde_json::to_vec(&entry).unwrap();
        let _ = buf.write(b"\n");
        let buf: &[u8] = buf.as_slice();
        self.lock()
            .await
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }

    /// Write an object to the log file.
    pub async fn write_obj<T: serde::Serialize>(&self, obj: &T) -> Result<(), Error> {
        let mut buf = serde_json::to_vec(obj).unwrap();
        let _ = buf.write(b"\n");
        let buf: &[u8] = buf.as_slice();
        self.lock()
            .await
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }

    /// Write a buffer to the log file.
    pub async fn write(self, buf: &[u8]) -> Result<(), Error> {
        self.lock()
            .await
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }
}

impl Default for DataInner {
    fn default() -> Self {
        Self {
            start_time: SystemTime::now(),
            #[cfg(feature = "crack-activity")]
            user_activity_map: Arc::new(dashmap::DashMap::new()),
            #[cfg(feature = "crack-activity")]
            activity_user_map: Arc::new(dashmap::DashMap::new()),
            #[cfg(feature = "crack-gpt")]
            gpt_ctx: Arc::new(RwLock::new(None)),
            ct_client: CrackTrackClient::default(),
            songbird: songbird::Songbird::serenity(),
            phone_data: PhoneCodeData::default(),
            bot_settings: Default::default(),
            join_vc_tokens: Default::default(),
            authorized_users: Default::default(),
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
            guild_msg_cache_ordered: Arc::new(Mutex::new(BTreeMap::new())),
            guild_command_msg_queue: Default::default(),
            guild_cnt_map: Default::default(),
            http_client: http_utils::get_client().clone(),
            event_log_async: EventLogAsync::default(),
            database_pool: None,
            db_channel: None,
        }
    }
}

impl Default for Data {
    fn default() -> Self {
        Self(Arc::new(DataInner::default()))
    }
}

/// Data struct for the bot, which is stored and accessible in all command invocations
#[derive(Clone, Debug)]
pub struct Data(pub Arc<DataInner>);

/// Impl [`Deref`] for our custom [`Data`] struct
impl std::ops::Deref for Data {
    type Target = DataInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Impl for our custom [`Data`] struct
impl Data {
    /// Insert a guild into the guild settings map.
    pub async fn insert_guild(
        &self,
        guild_id: GuildId,
        guild_settings: GuildSettings,
    ) -> Option<GuildSettings> {
        self.guild_settings_map
            .write()
            .await
            .insert(guild_id, guild_settings)
    }

    /// Create a new Data, calls default
    pub async fn downvote_track(
        &self,
        guild_id: GuildId,
        _track: &str,
    ) -> Result<TrackReaction, CrackedError> {
        let pool = self.get_db_pool()?;
        let play_log_id =
            PlayLog::get_last_played_by_guild_metadata(&pool, guild_id.into(), 1).await?;
        let pool = self.database_pool.as_ref().unwrap();
        let id = *play_log_id.first().unwrap() as i32;
        let _ = TrackReaction::insert(pool, id).await?;
        TrackReaction::add_dislike(pool, id).await
    }

    /// Add a message to the cache
    pub async fn add_msg_to_cache(&self, guild_id: GuildId, msg: Message) -> Option<Message> {
        let now = chrono::Utc::now();
        self.add_msg_to_cache_ts(guild_id, now, msg).await
    }

    /// Add msg to the cache with a timestamp.
    pub async fn add_msg_to_cache_ts(
        &self,
        guild_id: GuildId,
        ts: DateTime<Utc>,
        msg: Message,
    ) -> Option<Message> {
        let mut guild_msg_cache_ordered = self.guild_msg_cache_ordered.lock().await;
        guild_msg_cache_ordered
            .entry(guild_id)
            .or_default()
            .time_ordered_messages
            .insert(ts, msg)
    }

    /// Remove and return a message from the cache based on the guild_id and timestamp.
    pub async fn remove_msg_from_cache(
        &self,
        guild_id: GuildId,
        ts: DateTime<Utc>,
    ) -> Option<Message> {
        let mut guild_msg_cache_ordered = self.guild_msg_cache_ordered.lock().await;
        guild_msg_cache_ordered
            .get_mut(&guild_id)
            .unwrap()
            .time_ordered_messages
            .remove(&ts)
    }

    /// Add the guild settings for a guild.
    pub async fn add_guild_settings(&self, guild_id: GuildId, settings: GuildSettings) {
        self.guild_settings_map
            .write()
            .await
            .insert(guild_id, settings);
    }

    /// Set the guild settings for a guild and return a new copy.
    pub fn with_guild_settings_map(&self, guild_settings: GuildSettingsMapParam) -> Self {
        Self(Arc::new(self.0.with_guild_settings_map(guild_settings)))
    }

    /// Get the database pool for the postgresql database.
    pub fn get_db_pool(&self) -> Result<sqlx::PgPool, CrackedError> {
        self.database_pool
            .as_ref()
            .ok_or(CrackedError::NoDatabasePool)
            .cloned()
    }

    /// Deny a user permission to use the music commands.
    pub async fn add_denied_music_user(
        &self,
        guild_id: GuildId,
        user: UserId,
    ) -> CrackedResult<bool> {
        self.guild_settings_map
            .write()
            .await
            .entry(guild_id)
            .or_insert_with(GuildSettings::default)
            .add_denied_music_user(user)
            .await
    }

    /// Check if a user is allowed to use the music commands.
    pub async fn check_music_permissions(&self, guild_id: GuildId, user: UserId) -> bool {
        if let Some(settings) = self.guild_settings_map.read().await.get(&guild_id).cloned() {
            settings
                .get_music_permissions()
                .map(|x| x.is_user_allowed(user.get()))
                .unwrap_or(true)
        } else {
            true
        }
    }

    /// Push a message to the command message queue.
    pub async fn push_latest_msg(
        &self,
        guild_id: GuildId,
        msg: MessageOrReplyHandle,
    ) -> CrackedResult<()> {
        self.guild_command_msg_queue
            .entry(guild_id)
            .or_default()
            .push(msg);
        Ok(())
    }

    /// Forget all skip votes for a guild
    // This is used when a track ends, or when a user leaves the voice channel.
    // This is to prevent users from voting to skip a track, then leaving the voice channel.
    // TODO: Should this be moved to a separate module? Or should it be moved to a separate file?
    pub async fn forget_skip_votes(&self, guild_id: GuildId) -> Result<(), Error> {
        let _res = self
            .guild_cache_map
            .lock()
            .await
            .entry(guild_id)
            .and_modify(|cache| cache.current_skip_votes = HashSet::new())
            .or_default();

        Ok(())
    }

    pub async fn with_bot_settings(&self, bot_settings: BotConfig) -> Self {
        Self(Arc::new(self.0.with_bot_settings(bot_settings)))
    }

    pub fn with_songbird(&self, songbird: Arc<songbird::Songbird>) -> Self {
        Self(self.arc_inner().with_songbird(songbird).into())
    }

    pub fn arc_inner(&self) -> Arc<DataInner> {
        Into::into(self.0.clone())
    }
}

#[cfg(test)]
mod lib_test {
    use super::*;

    #[test]
    fn test_phone_code_data() {
        let data = PhoneCodeData::load().unwrap();
        let country_names = data.country_names;
        let phone_codes = data.phone_codes;
        let country_by_phone_code = data.country_by_phone_code;

        assert_eq!(country_names.get("US"), Some(&"United States".to_string()));
        assert_eq!(phone_codes.get("IS"), Some(&"354".to_string()));
        let want = &vec!["CA".to_string(), "UM".to_string(), "US".to_string()];
        let got = country_by_phone_code.get("1").unwrap();
        // This would be cheaper using a heap or tree
        assert!(got.iter().all(|x| want.contains(x)));
        assert!(want.iter().all(|x| got.contains(x)));
    }

    /// Test the creation of a default EventLog
    #[tokio::test]
    async fn test_event_log_default() {
        let event_log = EventLogAsync::default();
        let file = event_log.lock().await;
        assert_eq!(file.metadata().unwrap().len(), 0);
    }

    /// Test the creation and printing of CamKickConfig
    #[test]
    fn test_display_cam_kick_config() {
        let cam_kick = CamKickConfig::default();
        let want = r#"timeout:       0
guild_id:      0
chan_id:       0
dc_msg:        "You have been violated for being cammed down for too long."
msg_on_deafen: false
msg_on_mute:   false
msg_on_dc:     false
"#;
        assert_eq!(cam_kick.to_string(), want);
    }

    use serde_json::json;
    #[tokio::test]
    async fn test_with_data_inner() {
        let data = DataInner::default();
        let new_data = data.with_bot_settings(BotConfig::default());
        assert_eq!(json!(new_data.bot_settings), json!(BotConfig::default()));

        // let pool = sqlx::PgPool::connect_lazy("postgres://");
        // let new_data = new_data.with_database_pool(pool);
        // let want = sqlx::PgPool::connect_lazy("postgres://");
        // assert_eq!(new_data.database_pool.is_some(), want.is_some());

        let guild_settings = GuildSettingsMapParam::default();
        let new_data = new_data.with_guild_settings_map(guild_settings);
        assert!(new_data.guild_settings_map.read().await.is_empty());
    }
}
