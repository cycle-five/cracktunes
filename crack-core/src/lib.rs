use crate::guild::settings::DEFAULT_PREFIX;
use crate::handlers::event_log::LogEntry;
use chrono::DateTime;
use chrono::Utc;
use errors::CrackedError;
use guild::settings::DEFAULT_DB_URL;
use guild::settings::DEFAULT_VIDEO_STATUS_POLL_INTERVAL;
use poise::serenity_prelude::GuildId;
use reqwest::blocking::get;
use serde::{Deserialize, Serialize};
use serenity::all::Message;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::RwLock;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Display,
    sync::{Arc, Mutex},
};

pub mod commands;
pub mod connection;
pub mod db;
pub mod errors;
pub mod guild;
pub mod handlers;
pub mod http_utils;
pub mod messaging;
pub mod metrics;
pub mod sources;
pub mod utils;

//pub extern crate osint;

#[cfg(test)]
pub mod test;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
pub use Result;

pub fn is_prefix(ctx: Context) -> bool {
    ctx.prefix() != "/"
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CamKickConfig {
    pub cammed_down_timeout: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub dc_message: String,
    pub send_msg_deafen: bool,
    pub send_msg_mute: bool,
    pub send_msg_dc: bool,
}

impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            cammed_down_timeout: 0,
            guild_id: 0,
            channel_id: 0,
            dc_message: "You have been violated for being cammed down for too long.".to_string(),
            send_msg_deafen: false,
            send_msg_mute: false,
            send_msg_dc: false,
        }
    }
}

impl Display for CamKickConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!(
            "cammed_down_timeout: {:?}\n",
            self.cammed_down_timeout
        ));
        result.push_str(&format!("guild_id: {:?}\n", self.guild_id));
        result.push_str(&format!("channel_id: {:?}\n", self.channel_id));
        result.push_str(&format!("dc_message: {:?}\n", self.dc_message));
        result.push_str(&format!(
            "send_msg
        deafen: {}\n",
            self.send_msg_deafen
        ));
        result.push_str(&format!(
            "send_msg
        mute: {}\n",
            self.send_msg_mute
        ));
        result.push_str(&format!(
            "send_msg
        dc: {}\n",
            self.send_msg_dc
        ));

        write!(f, "{}", result)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotCredentials {
    pub discord_token: String,
    pub discord_app_id: String,
    pub spotify_client_id: Option<String>,
    pub spotify_client_secret: Option<String>,
    pub openai_key: Option<String>,
}
#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct BotConfig {
    pub video_status_poll_interval: Option<u64>,
    pub owners: Option<Vec<u64>>,
    // Cammed down kicking config
    pub cam_kick: Option<Vec<CamKickConfig>>,
    pub sys_log_channel_id: Option<u64>,
    pub self_deafen: Option<bool>,
    pub volume: Option<f32>,
    pub guild_settings_map: Option<Vec<guild::settings::GuildSettings>>,
    pub prefix: Option<String>,
    pub credentials: Option<BotCredentials>,
    pub database_url: Option<String>,
    pub users_to_log: Option<Vec<u64>>,
}

impl Display for BotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!(
            "video_status_poll_interval: {:?}\n",
            self.video_status_poll_interval
        ));
        result.push_str(&format!("authorized_users: {:?}\n", self.owners));
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
        write!(f, "{}", result)
    }
}

impl BotConfig {
    pub fn set_credentials(&mut self, creds: BotCredentials) -> &mut Self {
        self.credentials = Some(creds);
        self
    }

    pub fn get_prefix(&self) -> String {
        self.prefix
            .as_ref()
            .cloned()
            .unwrap_or(DEFAULT_PREFIX.to_string())
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
        let response = get(url).map_err(CrackedError::Reqwest)?;
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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DataInner {
    #[serde(skip)]
    pub phone_data: PhoneCodeData,
    pub up_prefix: &'static str,
    pub bot_settings: BotConfig,
    // TODO: Make this a HashMap, pointing to a settings struct containiong
    // user priviledges, etc
    pub authorized_users: HashSet<u64>,
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, guild::settings::GuildSettings>>>,
    //pub guild_settings_map: Arc<Mutex<HashMap<GuildId, guild::settings::GuildSettings>>>,
    #[serde(skip)]
    pub guild_msg_cache_ordered: Arc<Mutex<BTreeMap<GuildId, guild::cache::GuildCache>>>,
    #[serde(skip)]
    pub guild_cache_map: Arc<Mutex<HashMap<GuildId, guild::cache::GuildCache>>>,
    #[serde(skip)]
    pub event_log: EventLog,
    #[serde(skip)]
    pub database_pool: Option<sqlx::PgPool>,
}

/// General log for events that the bot reveices from Discord.
#[derive(Clone, Debug)]
pub struct EventLog(pub Arc<Mutex<File>>);

impl std::ops::Deref for EventLog {
    type Target = Arc<Mutex<File>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for EventLog {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(
            File::create("/data/events.log").unwrap(),
        )))
    }
}

/// impl of EventLog
impl EventLog {
    /// Create a new EventLog, calls default
    pub fn new() -> Self {
        Self::default()
    }

    /// Write an object to the log file without a note.
    pub fn write_log_obj<T: serde::Serialize>(&self, name: &str, obj: &T) -> Result<(), Error> {
        self.write_log_obj_note(name, None, obj)
    }

    /// Write an object to the log file with a note.
    pub fn write_log_obj_note<T: serde::Serialize>(
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
        let _ = buf.write(&[b'\n']);
        let buf: &[u8] = buf.as_slice();
        self.lock()
            .unwrap()
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }

    /// Write an object to the log file.
    pub fn write_obj<T: serde::Serialize>(&self, obj: &T) -> Result<(), Error> {
        let mut buf = serde_json::to_vec(obj).unwrap();
        let _ = buf.write(&[b'\n']);
        let buf: &[u8] = buf.as_slice();
        self.lock()
            .unwrap()
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }

    /// Write a buffer to the log file.
    pub fn write(self, buf: &[u8]) -> Result<(), Error> {
        self.lock()
            .unwrap()
            .write_all(buf)
            .map_err(|e| CrackedError::IO(e).into())
    }
}

impl Default for DataInner {
    fn default() -> Self {
        Self {
            phone_data: PhoneCodeData::load().unwrap(),
            up_prefix: "R",
            bot_settings: Default::default(),
            authorized_users: Default::default(),
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
            guild_msg_cache_ordered: Arc::new(Mutex::new(BTreeMap::new())),
            event_log: EventLog::default(),
            database_pool: None,
        }
    }
}

impl Default for Data {
    fn default() -> Self {
        Self(Arc::new(DataInner::default()))
    }
}

#[derive(Clone, Debug)]
pub struct Data(pub Arc<DataInner>);

impl std::ops::Deref for Data {
    type Target = DataInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Data {
    pub fn add_msg_to_cache(&self, guild_id: GuildId, msg: Message) -> Option<Message> {
        let now = chrono::Utc::now();
        self.add_msg_to_cache_ts(guild_id, now, msg)
    }
    /// Add msg to the cache with a timestamp.
    pub fn add_msg_to_cache_ts(
        &self,
        guild_id: GuildId,
        ts: DateTime<Utc>,
        msg: Message,
    ) -> Option<Message> {
        let mut guild_msg_cache_ordered = self.guild_msg_cache_ordered.lock().unwrap();
        guild_msg_cache_ordered
            .entry(guild_id)
            .or_default()
            .time_ordered_messages
            .insert(ts, msg)
    }

    pub fn get_msg_from_cache(&self, guild_id: GuildId, ts: DateTime<Utc>) -> Option<Message> {
        let mut guild_msg_cache_ordered = self.guild_msg_cache_ordered.lock().unwrap();
        guild_msg_cache_ordered
            .get_mut(&guild_id)
            .unwrap()
            .time_ordered_messages
            .remove(&ts)
    }
}
