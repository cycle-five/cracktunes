use chrono::{DateTime, Utc};
// use poise::futures_util::AsyncWriteExt;
use poise::serenity_prelude as serenity;
use serde::{ser::SerializeStruct, Serialize, Serializer};
use serenity::model::{channel::Message, id::GuildId, id::UserId};
use songbird::Songbird;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Write,
    path::Path,
    sync::Arc,
    time::SystemTime,
};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};

use crate::{
    cache::GuildCache,
    client::CrackTrackClient,
    config::BotConfig,
    http_utils,
    settings::{get_log_prefix, GuildSettings, GuildSettingsMapParam},
    ArcTMutex, CrackedError, CrackedResult, Error, GuildEntity, MessageOrReplyHandle, MetadataMsg,
    PlayLog, TrackReaction, TypeMapKey,
};

#[derive(Debug)]
pub struct LogEntry<T: Serialize> {
    pub name: String,
    pub notes: String,
    pub event: T,
}

impl<T: Serialize> Serialize for LogEntry<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let n = if self.notes.is_empty() { 2 } else { 3 };
        let mut state = serializer.serialize_struct("LogEntry", n)?;
        state.serialize_field("name", &self.name)?;
        if !self.notes.is_empty() {
            state.serialize_field("notes", &self.notes)?;
        }
        state.serialize_field("event", &self.event)?;
        state.end()
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
    pub authorized_users: HashSet<u64>,
    // Why not Arc here?
    pub join_vc_tokens: dashmap::DashMap<GuildId, Arc<tokio::sync::Mutex<()>>>,
    //pub phone_data: PhoneCodeData,
    pub event_log_async: EventLogAsync,
    // Why Option instead of Arc here? Certainly it's an indirection to allow for an uninitialized state
    // to exist, but why not just use a default value? If it's necessary to wrap the type is that newtype better
    // or worse than using an Option?
    pub db_channel: Option<Sender<MetadataMsg>>,
    pub database_pool: Option<sqlx::PgPool>,
    pub http_client: reqwest::Client,
    //RwLock, then Mutex, why?
    pub guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>,
    pub guild_cache_map: Arc<Mutex<HashMap<GuildId, GuildCache>>>,
    pub id_cache_map: dashmap::DashMap<u64, GuildCache>,
    pub guild_command_msg_queue: dashmap::DashMap<GuildId, Vec<MessageOrReplyHandle>>,
    pub guild_cnt_map: dashmap::DashMap<GuildId, u64>,
    // Option inside?
    #[cfg(feature = "crack-gpt")]
    pub gpt_ctx: Arc<RwLock<Option<GptContext>>>,
    // No arc, but we need a lifetime?
    // What fundemental limitation comes up that must be solved by this?
    pub ct_client: Option<CrackTrackClient<'static>>,
    pub songbird: Arc<Songbird>,
}

impl std::fmt::Debug for DataInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        //result.push_str(&format!("phone_data: {:?}\n", self.phone_data));
        result.push_str(&format!("bot_settings: {:?}\n", self.bot_settings));
        result.push_str(&format!("authorized_users: {:?}\n", self.authorized_users));
        result.push_str(&format!(
            "guild_settings_map: {:?}\n",
            self.guild_settings_map
        ));
        result.push_str(&format!("id_cache_map: {:?}\n", self.id_cache_map));
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

    /// Set the CrackTrack client for the data.
    pub fn with_ct_client(&self, ct_client: CrackTrackClient<'static>) -> Self {
        Self {
            ct_client: Some(ct_client),
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
            #[cfg(feature = "crack-gpt")]
            gpt_ctx: Arc::new(RwLock::new(None)),
            ct_client: None,
            songbird: Songbird::serenity(), // Initialize with an uninitialized Songbird instance
            //phone_data: PhoneCodeData::default(),
            bot_settings: Default::default(),
            join_vc_tokens: Default::default(),
            authorized_users: Default::default(),
            guild_settings_map: Arc::new(RwLock::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
            id_cache_map: dashmap::DashMap::default(),
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
        self.add_msg_to_cache_ts(guild_id.into(), now, msg).await
    }

    /// Add a message to the cache
    pub async fn add_msg_to_cache_int(&self, id: u64, msg: Message) -> Option<Message> {
        let now = chrono::Utc::now();
        self.add_msg_to_cache_ts(id, now, msg).await
    }

    /// Add msg to the cache with a timestamp.
    pub async fn add_msg_to_cache_ts(
        &self,
        id: u64,
        ts: DateTime<Utc>,
        msg: Message,
    ) -> Option<Message> {
        self.id_cache_map
            .entry(id)
            .or_default()
            .time_ordered_messages
            .insert(ts, msg)
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

    /// Reload the guild settings from the database.
    pub async fn reload_guild_settings(&self, guild_id: GuildId) -> Result<(), CrackedError> {
        let pool = self.get_db_pool()?;
        let guild_entity = GuildEntity::get(&pool, guild_id.into())
            .await?
            .ok_or(CrackedError::NoGuildId)?;
        let guild_settings = guild_entity.get_settings(&pool).await?;
        self.guild_settings_map
            .write()
            .await
            .insert(guild_id, guild_settings);
        //let settings = GuildSettings::get_by_guild_id(&pool, guild_id.into()).await?;
        //self.add_guild_settings(guild_id, settings).await;
        Ok(())
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
    use crate::CamKickConfig;
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
}
