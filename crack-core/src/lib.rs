// Nightly's trait solver overflows proving `songbird::Call: Send` while
// checking `RandomMuteHandler: Sync` (rust-lang/rust#159228). 128 is the
// depth it reports; stable still solves it inside the default limit.
#![recursion_limit = "256"]

#[cfg(feature = "crack-coin")]
pub mod coin;
pub mod commands;
pub mod config;
pub mod connection;
pub mod db;
pub use crack_common::errors;
pub mod guild;
pub mod handlers;
pub mod http_utils;
#[macro_use]
pub mod macros;
pub mod economy;
pub mod messaging;
pub mod metrics;
pub mod modlog;
#[cfg(feature = "crack-music")]
pub mod music;
pub mod owners;
pub mod poise_ext;
pub mod rooms;
pub mod sources;
#[cfg(test)]
pub mod test;
#[cfg(test)]
pub(crate) mod test_db;
pub mod utils;

//#![feature(linked_list_cursors)]
use crate::handlers::event_log::LogEntry;
use crate::music::query::DEFAULT_PLAYLIST_LIMIT;

/// The most tracks a playlist link may queue, however it is configured.
pub const MAX_PLAYLIST_LIMIT: u64 = 1000;
#[cfg(feature = "crack-activity")]
use ::serenity::all::Activity;
use chrono::{DateTime, Utc};
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
use std::time::SystemTime;
use std::{
    collections::{HashMap, HashSet},
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
pub use crack_common::{CrackedHowResult, CrackedResult};

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
use crack_types::discord::ToRef;
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

#[derive(Serialize, Deserialize, Clone)]
pub struct BotCredentials {
    pub discord_token: String,
    pub discord_app_id: String,
    pub spotify_client_id: Option<String>,
    pub spotify_client_secret: Option<String>,
    pub openai_api_key: Option<String>,
    pub virustotal_api_key: Option<String>,
}

/// Hand-written so no `{:?}` anywhere can print a secret. The startup config
/// dump did exactly that with the Discord token, which is a full bot-takeover
/// credential; a derived Debug puts it in the logs of every deployment. What
/// is useful in a log is whether a credential is PRESENT, never its value.
impl std::fmt::Debug for BotCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn shown(v: &Option<String>) -> &'static str {
            if v.is_some() {
                "<set>"
            } else {
                "<unset>"
            }
        }
        f.debug_struct("BotCredentials")
            .field("discord_token", &"<redacted>")
            .field(
                "discord_app_id",
                &if self.discord_app_id.is_empty() {
                    "<unset>"
                } else {
                    "<set>"
                },
            )
            .field("spotify_client_id", &shown(&self.spotify_client_id))
            .field("spotify_client_secret", &shown(&self.spotify_client_secret))
            .field("openai_api_key", &shown(&self.openai_api_key))
            .field("virustotal_api_key", &shown(&self.virustotal_api_key))
            .finish()
    }
}

/// A connection URL with its password replaced. These get logged at startup,
/// so the password must not survive the trip.
pub fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let rest = &url[scheme_end + 3..];
    let Some(at) = rest.rfind('@') else {
        return url.to_string();
    };
    let user = rest[..at].split(':').next().unwrap_or("");
    format!(
        "{}{}:<redacted>{}",
        &url[..scheme_end + 3],
        user,
        &rest[at..]
    )
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
    pub volume: Option<f32>,
    #[serde(skip)]
    pub guild_settings_map: Option<Vec<guild::settings::GuildSettings>>,
    pub prefix: Option<String>,
    pub credentials: Option<BotCredentials>,
    pub database_url: Option<String>,
    pub log_prefix: Option<String>,
    /// How slash commands reach Discord. `COMMAND_REGISTRATION` overrides it.
    #[serde(default)]
    pub command_registration: commands::registration::CommandRegistration,
    /// Most entries pulled in when playing a playlist link. Unset or 0 means
    /// `DEFAULT_PLAYLIST_LIMIT`.
    pub playlist_limit: Option<u64>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: Some(DEFAULT_VIDEO_STATUS_POLL_INTERVAL),
            owners: None,
            cam_kick: None,
            sys_log_channel_id: None,
            volume: Some(DEFAULT_VOLUME_LEVEL),
            guild_settings_map: None,
            prefix: Some(DEFAULT_PREFIX.to_string()),
            credentials: Some(BotCredentials::default()),
            // None, not a localhost guess: the bot must not assume a Postgres
            // nobody configured. DATABASE_URL or cracktunes.toml supplies it.
            database_url: None,
            log_prefix: Some(DEFAULT_LOG_PREFIX.to_string()),
            command_registration: Default::default(),
            playlist_limit: None,
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
        result.push_str(&format!("volume: {:?}\n", self.volume));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!(
            "prefix: {}\n",
            self.prefix
                .as_ref()
                .cloned()
                .unwrap_or(DEFAULT_PREFIX.to_string())
        ));
        result.push_str(&format!("credentials: {:?}\n", self.credentials.is_some()));
        result.push_str(&format!(
            "database_url: {:?}\n",
            self.database_url.as_deref().map(redact_url)
        ));
        result.push_str(&format!("log_prefix: {:?}\n", self.log_prefix));
        result.push_str(&format!(
            "command_registration: {:?}\n",
            self.command_registration
        ));
        result.push_str(&format!("playlist_limit: {}\n", self.get_playlist_limit()));
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

    /// The playlist entry cap: the configured value, or the default when it
    /// is unset or 0 (0 would make every playlist empty), and never more than
    /// `MAX_PLAYLIST_LIMIT`: a whole playlist's play-log writes go out in one
    /// `try_send` burst into a 1024-slot channel (`queue.rs`, `poise_ext.rs`),
    /// so a larger cap drops play_log rows.
    pub fn get_playlist_limit(&self) -> u64 {
        match self.playlist_limit {
            Some(n) if n > 0 => n.min(MAX_PLAYLIST_LIMIT),
            _ => DEFAULT_PLAYLIST_LIMIT,
        }
    }

    pub fn get_database_url(&self) -> String {
        self.database_url
            .as_ref()
            .cloned()
            .unwrap_or(DEFAULT_DB_URL.to_string())
    }
}

/// User data, which is stored and accessible in all command invocations
#[derive(Clone)]
pub struct DataInner {
    pub bot_settings: BotConfig,
    pub start_time: SystemTime,
    // Why is Arc needed? Why dashmap instead of Mutex<HashMap>?
    #[cfg(feature = "crack-activity")]
    pub user_activity_map: Arc<dashmap::DashMap<UserId, Activity>>,
    #[cfg(feature = "crack-activity")]
    pub activity_user_map: Arc<dashmap::DashMap<String, dashmap::DashSet<UserId>>>,
    // Why not Arc here?
    /// Who owns playback per guild. Written ONLY by `claim_playback` /
    /// `release_playback`, which are called from inside the methods that move
    /// `gp_games` -- see `music/lease.rs`. Do not write it anywhere else.
    pub playback_owners: dashmap::DashMap<serenity::GuildId, crate::music::lease::PlaybackOwner>,
    /// Per-guild queue-mutation mutex. Held for milliseconds; see `lock_queue`.
    pub queue_locks: dashmap::DashMap<serenity::GuildId, Arc<tokio::sync::Mutex<()>>>,
    /// Per-guild floating status message; see `messaging::status`.
    pub status_slots: dashmap::DashMap<
        serenity::GuildId,
        Arc<tokio::sync::Mutex<crate::messaging::status::StatusSlot>>,
    >,
    /// Per-guild notice of tracks that failed to play; see
    /// `messaging::track_failed`.
    pub failure_notices: dashmap::DashMap<
        serenity::GuildId,
        Arc<tokio::sync::Mutex<crate::messaging::track_failed::NoticeSlot>>,
    >,
    /// The now-playing buttons' press debounce; see `messaging::buttons`.
    pub np_presses: crate::messaging::throttle::Throttle<(GuildId, UserId)>,
    pub event_log_async: EventLogAsync,
    // Why Option instead of Arc here? Certainly it's an indirection to allow for an uninitialized state
    // to exist, but why not just use a default value? If it's necessary to wrap the type is that newtype better
    // or worse than using an Option?
    pub db_channel: Option<Sender<MetadataMsg>>,
    pub database_pool: Option<sqlx::PgPool>,
    pub http_client: reqwest::Client,
    //RwLock, then Mutex, why?
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, guild::settings::GuildSettings>>>,
    pub guild_cache_map: Arc<Mutex<HashMap<GuildId, guild::cache::GuildCache>>>,
    pub id_cache_map: dashmap::DashMap<u64, guild::cache::GuildCache>,
    pub guild_command_msg_queue: dashmap::DashMap<GuildId, Vec<MessageOrReplyHandle>>,
    pub guild_cnt_map: dashmap::DashMap<GuildId, u64>,
    /// Guilty pleasure games, one per guild. See `commands::music::gp`.
    pub gp_games: dashmap::DashMap<GuildId, commands::music::gp::GpGame>,
    /// Where `gp_games` are written down, when there is a database. `None` runs
    /// the game in memory only. See `commands::music::gp_persist`.
    pub gp_persist:
        Option<tokio::sync::mpsc::UnboundedSender<commands::music::gp_persist::GpPersist>>,
    /// Where queue audit events go (see `music::audit`). `None` without a
    /// database, and then `emit` logs them instead.
    pub audit_tx: Option<tokio::sync::mpsc::Sender<crate::music::audit::AuditEvent>>,
    // No arc, but we need a lifetime?
    // What fundemental limitation comes up that must be solved by this?
    pub ct_client: CrackTrackClient<'static>,
    pub songbird: Arc<Songbird>,
    /// The autoplay recommender. ONE per process (Ruling 42): the MusicBrainz
    /// 1/sec gate and a provider's disabled flag both live
    /// inside it, and a second instance would quietly reset them. `None` when
    /// no recommender could be built.
    pub musicreco: Option<Arc<crack_musicreco::MusicReco>>,
    /// Pending autoplay recommendations per guild. One call yields up to 20
    /// tracks, which is what keeps musicatlas inside its daily budget.
    pub autoplay_buffer: Arc<crate::music::autoplay::AutoplayBuffer>,
    /// The live voice rooms; see `rooms::registry`.
    pub rooms: crate::rooms::registry::RoomRegistry,
    /// The mod log's settings; see `modlog::registry`.
    pub modlog: crate::modlog::registry::ModLogRegistry,
    /// Per-server credits settings; see `economy::registry`.
    pub economy: Arc<crate::economy::registry::EconomyRegistry>,
}

impl std::fmt::Debug for DataInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        result.push_str(&format!("bot_settings: {:?}\n", self.bot_settings));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!("id_cache_map: {:?}\n", self.id_cache_map));
        result.push_str(&format!("guild_cache_map: {:?}\n", self.guild_cache_map));
        result.push_str(&format!("event_log: {:?}\n", self.event_log_async));
        result.push_str(&format!("database_pool: {:?}\n", self.database_pool));
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

    /// Set the CrackTrack client for the data.
    pub fn with_ct_client(&self, ct_client: CrackTrackClient<'static>) -> Self {
        Self {
            ct_client,
            ..self.clone()
        }
    }

    /// Set the Songbird instance for the data.
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
                    .expect("Should have a file object to write too??? (three bcz real confused).")
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
            ct_client: CrackTrackClient::default(),
            songbird: Songbird::serenity(), // Initialize with an uninitialized Songbird instance
            bot_settings: Default::default(),
            playback_owners: Default::default(),
            queue_locks: Default::default(),
            status_slots: Default::default(),
            failure_notices: Default::default(),
            np_presses: crate::messaging::throttle::Throttle::new(
                crate::messaging::buttons::NP_PRESS_WINDOW,
            ),
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
            id_cache_map: dashmap::DashMap::default(),
            guild_command_msg_queue: Default::default(),
            guild_cnt_map: Default::default(),
            gp_games: Default::default(),
            gp_persist: None,
            audit_tx: None,
            http_client: http_utils::get_client().clone(),
            event_log_async: EventLogAsync::default(),
            database_pool: None,
            db_channel: None,
            musicreco: None,
            autoplay_buffer: Arc::default(),
            rooms: Default::default(),
            modlog: Default::default(),
            economy: Default::default(),
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
    /// The two fields the economy reads, as a handle that does not name
    /// `Data`. The registry is shared (same `Arc`), not copied.
    pub fn economy_data(&self) -> crate::economy::EconomyData {
        crate::economy::EconomyData {
            database_pool: self.database_pool.clone(),
            economy: self.economy.clone(),
        }
    }

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

    /// Record one dislike against the newest play of `source_url` in the guild.
    ///
    /// Errors with `NoPlayLogged` when no play of it is logged yet (the play
    /// log is written asynchronously and may lag), and `NoDatabasePool`
    /// without a database.
    pub async fn downvote_track(
        &self,
        guild_id: GuildId,
        source_url: &str,
    ) -> Result<TrackReaction, CrackedError> {
        let pool = self.get_db_pool()?;
        let play_log_id = PlayLog::last_play_of_url(&pool, guild_id.get() as i64, source_url)
            .await?
            .ok_or(CrackedError::NoPlayLogged)?;
        let id = i32::try_from(play_log_id).map_err(|_| CrackedError::NoPlayLogged)?;
        TrackReaction::insert(&pool, id).await?;
        TrackReaction::add_dislike(&pool, id).await
    }

    pub async fn add_reply_handle_to_cache(
        &self,
        guild_id: GuildId,
        handle: MessageOrReplyHandle,
    ) -> Option<MessageOrReplyHandle> {
        let mut guild_msg_cache = self.guild_command_msg_queue.entry(guild_id).or_default();
        guild_msg_cache.push(handle.clone());
        Some(handle)
    }

    /// Remove and return a message from the cache based on the guild_id and timestamp.
    pub async fn remove_msg_from_cache(
        &self,
        guild_id: GuildId,
        ts: DateTime<Utc>,
    ) -> Option<Message> {
        self.id_cache_map
            .get_mut(&guild_id.into())
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
            .or_insert_with(|| GuildSettings::new(guild_id.to_ref(), None, None))
            .add_denied_music_user(user.to_ref())
            .await
    }

    /// Check if a user is allowed to use the music commands.
    pub async fn check_music_permissions(&self, guild_id: GuildId, user: UserId) -> bool {
        if let Some(settings) = self.guild_settings_map.read().await.get(&guild_id).cloned() {
            settings
                .get_music_permissions()
                .map(|x| x.is_user_allowed(&user.to_ref()))
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
    use serde_json::json;

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

    #[tokio::test]
    async fn test_with_data_inner() {
        let data = DataInner::default();
        let new_data = data.with_bot_settings(BotConfig::default());
        assert_eq!(json!(new_data.bot_settings), json!(BotConfig::default()));

        let guild_settings = GuildSettingsMapParam::default();
        let new_data = new_data.with_guild_settings_map(guild_settings);
        assert!(new_data.guild_settings_map.read().await.is_empty());
    }

    #[tokio::test]
    async fn check_music_permissions_reads_refs() {
        let data = Data::default();
        let mut settings = GuildSettings::new(GuildId::new(5).to_ref(), None, None);
        settings
            .add_denied_music_user(UserId::new(7).to_ref())
            .await
            .unwrap();
        data.guild_settings_map
            .write()
            .await
            .insert(GuildId::new(5), settings);
        assert!(
            !data
                .check_music_permissions(GuildId::new(5), UserId::new(7))
                .await
        );
        assert!(
            data.check_music_permissions(GuildId::new(5), UserId::new(8))
                .await
        );
        assert!(
            data.check_music_permissions(GuildId::new(6), UserId::new(7))
                .await
        );
    }

    #[tokio::test]
    async fn economy_data_carries_the_pool_and_shares_the_registry() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://localhost/unused")
            .unwrap();
        let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
        let handle = data.economy_data();
        assert!(handle.database_pool.is_some());
        assert!(Arc::ptr_eq(&handle.economy, &data.economy));
        assert!(Data::default().economy_data().database_pool.is_none());
    }
}

#[cfg(test)]
mod playlist_limit_tests {
    use super::*;

    #[test]
    fn bot_config_defaults_to_a_300_track_playlist_limit() {
        assert_eq!(BotConfig::default().get_playlist_limit(), 300);
    }

    #[test]
    fn a_zero_playlist_limit_means_the_default() {
        let config = BotConfig {
            playlist_limit: Some(0),
            ..Default::default()
        };
        assert_eq!(config.get_playlist_limit(), 300);
    }

    #[test]
    fn a_playlist_limit_above_the_ceiling_is_capped() {
        let config = BotConfig {
            playlist_limit: Some(5000),
            ..Default::default()
        };
        assert_eq!(config.get_playlist_limit(), MAX_PLAYLIST_LIMIT);
        assert_eq!(config.get_playlist_limit(), 1000);
    }

    #[test]
    fn a_configured_playlist_limit_is_used() {
        let config = BotConfig {
            playlist_limit: Some(1000),
            ..Default::default()
        };
        assert_eq!(config.get_playlist_limit(), 1000);
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    /// The config is logged at startup. A password must not survive the trip
    /// through `Display`. See #473.
    #[test]
    fn display_redacts_the_database_password() {
        let config = BotConfig {
            database_url: Some(
                "postgres://cracktunes:sup3rs3cr3t@postgres:5432/cracktunes".to_string(),
            ),
            ..Default::default()
        };

        let rendered = format!("{}", config);

        assert!(
            !rendered.contains("sup3rs3cr3t"),
            "Display leaked the password:\n{rendered}"
        );
        assert!(
            rendered.contains("<redacted>"),
            "Display did not redact at all:\n{rendered}"
        );
    }
}

#[cfg(test)]
mod downvote_tests {
    use super::*;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    async fn play(pool: &sqlx::PgPool, url: &str) -> (i64, i64) {
        let metadata_id: i64 = sqlx::query_scalar(
            "INSERT INTO metadata (source_url) VALUES ($1) RETURNING id::bigint",
        )
        .bind(url)
        .fetch_one(pool)
        .await
        .unwrap();
        let log = PlayLog::create(pool, 1, 1, metadata_id).await.unwrap();
        (metadata_id, log.id)
    }

    /// 🪤 The original #207 bug: `downvote_track` took the newest *metadata*
    /// id of the guild and used it as a play_log id, ignoring the URL.
    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn downvote_track_dislikes_the_play_of_that_url(pool: sqlx::PgPool) {
        // Shift metadata ids away from play_log ids.
        for i in 0..3 {
            sqlx::query("INSERT INTO metadata (source_url) VALUES ($1)")
                .bind(format!("https://example.test/filler{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let (_, wanted) = play(&pool, "https://example.test/a").await;
        let (_, _newest_other) = play(&pool, "https://example.test/b").await;
        let data = Data(Arc::new(DataInner::default().with_database_pool(pool)));
        let reaction = data
            .downvote_track(GuildId::new(1), "https://example.test/a")
            .await
            .unwrap();
        assert_eq!(reaction.play_log_id, wanted);
        assert_eq!(reaction.dislikes, 1);
        let unlogged = data
            .downvote_track(GuildId::new(1), "https://example.test/never")
            .await;
        assert_eq!(unlogged.unwrap_err(), CrackedError::NoPlayLogged);
    }
}

/// Whether this binary was built with the CrackCoin devnet token. A plain `bool`
/// so callers need no `cfg` of their own.
pub const COIN_BUILD: bool = cfg!(feature = "crack-coin");
