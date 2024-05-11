use crate::handlers::event_log::LogEntry;
use chrono::DateTime;
use chrono::Utc;
use db::PlayLog;
use db::TrackReaction;
use errors::CrackedError;
use guild::settings::get_log_prefix;
use guild::settings::GuildSettings;
use guild::settings::GuildSettingsMapParam;
use guild::settings::{
    DEFAULT_DB_URL, DEFAULT_LOG_PREFIX, DEFAULT_PREFIX, DEFAULT_VIDEO_STATUS_POLL_INTERVAL,
    DEFAULT_VOLUME_LEVEL,
};
use poise::serenity_prelude::GuildId;
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
pub mod interface;
pub mod messaging;
pub mod metrics;
pub mod sources;
#[cfg(test)]
pub mod test;
pub mod utils;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
pub use Result;

pub trait CrackContext<'a> {
    fn add_msg_to_cache(&self, guild_id: GuildId, msg: Message) -> Option<Message>;
}

impl<'a> CrackContext<'a> for Context<'a> {
    fn add_msg_to_cache(&self, guild_id: GuildId, msg: Message) -> Option<Message> {
        self.data().add_msg_to_cache(guild_id, msg)
    }
}

/// Checks if we're in a prefix context or not.
pub fn is_prefix(ctx: Context) -> bool {
    matches!(ctx, Context::Prefix(_))
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

/// Display impl for CamKickConfig
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
        result.push_str(&format!("deafen: {}\n", self.send_msg_deafen));
        result.push_str(&format!("mute: {}\n", self.send_msg_mute));
        result.push_str(&format!("dc: {}\n", self.send_msg_dc));

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
        result.push_str(&format!("credentials: {:?}\n", self.credentials));
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
        let client = reqwest::blocking::ClientBuilder::new()
            .use_rustls_tls()
            .build()?;
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
#[derive(Serialize, Deserialize, Clone)]
pub struct DataInner {
    #[serde(skip)]
    pub phone_data: PhoneCodeData,
    pub up_prefix: &'static str,
    pub bot_settings: BotConfig,
    // TODO: Make this a HashMap, pointing to a settings struct containiong
    // user priviledges, etc
    pub authorized_users: HashSet<u64>,
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, guild::settings::GuildSettings>>>,
    #[serde(skip)]
    pub guild_msg_cache_ordered: Arc<Mutex<BTreeMap<GuildId, guild::cache::GuildCache>>>,
    #[serde(skip)]
    pub guild_cache_map: Arc<Mutex<HashMap<GuildId, guild::cache::GuildCache>>>,
    #[serde(skip)]
    pub event_log: EventLog,
    #[serde(skip)]
    pub database_pool: Option<sqlx::PgPool>,
    #[serde(skip)]
    pub http_client: reqwest::Client,
    #[serde(skip)]
    pub sender: Option<tokio::sync::mpsc::Sender<db::worker_pool::MetadataWriteData>>,
    // #[serde(skip, default = "default_topgg_client")]
    // pub topgg_client: topgg::Client,
}

// /// Get the default topgg client
// fn default_topgg_client() -> topgg::Client {
//     topgg::Client::new(std::env::var("TOPGG_TOKEN").unwrap_or_default())
// }

impl std::fmt::Debug for DataInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("phone_data: {:?}\n", self.phone_data));
        result.push_str(&format!("up_prefix: {:?}\n", self.up_prefix));
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
        result.push_str(&format!("event_log: {:?}\n", self.event_log));
        result.push_str(&format!("database_pool: {:?}\n", self.database_pool));
        result.push_str(&format!("http_client: {:?}\n", self.http_client));
        result.push_str("topgg_client: <skipped>\n");
        write!(f, "{}", result)
    }
}

impl DataInner {
    /// Set the bot settings for the data
    pub fn with_bot_settings(&self, bot_settings: BotConfig) -> Self {
        Self {
            bot_settings,
            ..self.clone()
        }
    }

    /// Set the database pool for the data
    pub fn with_database_pool(&self, database_pool: sqlx::PgPool) -> Self {
        Self {
            database_pool: Some(database_pool),
            ..self.clone()
        }
    }

    /// Set the guild settings map for the data
    pub fn with_guild_settings_map(&self, guild_settings: GuildSettingsMapParam) -> Self {
        Self {
            guild_settings_map: guild_settings,
            ..self.clone()
        }
    }
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
        let log_path = format!("{}/events.log", get_log_prefix());
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
        Self(Arc::new(Mutex::new(log_file)))
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
        // let topgg_token = std::env::var("TOPGG_TOKEN").unwrap_or_default();
        Self {
            phone_data: PhoneCodeData::default(), //PhoneCodeData::load().unwrap(),
            up_prefix: "R",
            bot_settings: Default::default(),
            authorized_users: Default::default(),
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
            guild_msg_cache_ordered: Arc::new(Mutex::new(BTreeMap::new())),
            event_log: EventLog::default(),
            database_pool: None,
            http_client: http_utils::get_client().clone(),
            sender: None,
            // topgg_client: topgg::Client::new(topgg_token),
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
    /// Create a new Data, calls default
    pub async fn downvote_track(
        &self,
        guild_id: GuildId,
        _track: &str,
    ) -> Result<TrackReaction, CrackedError> {
        let play_log_id = PlayLog::get_last_played_by_guild_metadata(
            self.database_pool.as_ref().unwrap(),
            guild_id.into(),
        )
        .await?;
        let pool = self.database_pool.as_ref().unwrap();
        let id = *play_log_id.first().unwrap() as i32;
        let _ = TrackReaction::insert(pool, id).await;
        TrackReaction::add_dislike(pool, id).await
    }

    /// Add a message to the cache
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

    /// Remove and return a message from the cache based on the guild_id and timestamp.
    pub fn remove_msg_from_cache(&self, guild_id: GuildId, ts: DateTime<Utc>) -> Option<Message> {
        let mut guild_msg_cache_ordered = self.guild_msg_cache_ordered.lock().unwrap();
        guild_msg_cache_ordered
            .get_mut(&guild_id)
            .unwrap()
            .time_ordered_messages
            .remove(&ts)
    }

    /// Get the guild settings for a guild (read only)
    pub fn get_guild_settings(&self, guild_id: GuildId) -> Option<GuildSettings> {
        self.guild_settings_map.read().ok()?.get(&guild_id).cloned()
    }

    pub fn add_guild_settings(&self, guild_id: GuildId, settings: GuildSettings) {
        self.guild_settings_map
            .write()
            .unwrap()
            .insert(guild_id, settings);
    }

    // /// Get the guild settings for a guild (read)
    // pub fn get_guild_settings_mut(&self, guild_id: GuildId) -> Option<&mut GuildSettings> {
    //     let mut asdf = self.guild_settings_map.write().unwrap().clone();
    //     let qwer = asdf.get_mut(&guild_id);
    //     qwer
    // }

    /// Set the guild settings for a guild and return a new copy.
    pub fn with_guild_settings_map(&self, guild_settings: GuildSettingsMapParam) -> Self {
        Self(Arc::new(self.0.with_guild_settings_map(guild_settings)))
    }

    // /// Get the guild settings for a guild (mutable)
    // pub fn get_guild_settings_mut(&self, guild_id: GuildId) -> Option<&mut GuildSettings> {
    //     self.guild_settings_map.write().unwrap().get_mut(&guild_id)
    // }
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
    #[test]
    fn test_event_log_default() {
        let event_log = EventLog::default();
        let file = event_log.lock().unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
    }

    /// Test the creation and printing of CamKickConfig
    #[test]
    fn test_display_cam_kick_config() {
        let cam_kick = CamKickConfig::default();
        let want = "cammed_down_timeout: 0\nguild_id: 0\nchannel_id: 0\ndc_message: \"You have been violated for being cammed down for too long.\"\ndeafen: false\nmute: false\ndc: false\n";
        assert_eq!(cam_kick.to_string(), want);
    }

    use serde_json::json;
    #[test]
    fn test_with_data_inner() {
        let data = DataInner::default();
        let new_data = data.with_bot_settings(BotConfig::default());
        assert_eq!(json!(new_data.bot_settings), json!(BotConfig::default()));

        // let pool = sqlx::PgPool::connect_lazy("postgres://");
        // let new_data = new_data.with_database_pool(pool);
        // let want = sqlx::PgPool::connect_lazy("postgres://");
        // assert_eq!(new_data.database_pool.is_some(), want.is_some());

        let guild_settings = GuildSettingsMapParam::default();
        let new_data = new_data.with_guild_settings_map(guild_settings);
        assert!(new_data.guild_settings_map.read().unwrap().is_empty());
    }
}
