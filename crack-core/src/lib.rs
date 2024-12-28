#![allow(internal_features)]
#![feature(fmt_internals)]
#![feature(formatting_options)]
pub mod commands;
pub mod config;
pub mod handlers;
// pub mod metrics;
#[cfg(feature = "crack-music")]
pub mod music;
pub mod sources;
#[cfg(test)]
pub mod test;

//#![feature(linked_list_cursors)]
use crate::handlers::event_log::LogEntry;
#[cfg(feature = "crack-activity")]
use ::serenity::all::Activity;
use chrono::{DateTime, Utc};
#[cfg(feature = "crack-gpt")]
use crack_gpt::GptContext;
use crack_testing::CrackTrackClient;
use crack_types::db::worker_pool::MetadataMsg;
use crack_types::db::{GuildEntity, PlayLog, TrackReaction};
use crack_types::errors::CrackedError;
use crack_types::guild::settings::get_log_prefix;
use crack_types::guild::settings::{GuildSettings, GuildSettingsMapParam};
use crack_types::guild::settings::{
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

// pub type Error = Box<dyn std::error::Error + Send + Sync>;
// pub type ArcTRwLock<T> = Arc<tokio::sync::RwLock<T>>;
// pub type ArcTMutex<T> = Arc<tokio::sync::Mutex<T>>;
// pub type ArcRwMap<K, V> = Arc<std::sync::RwLock<HashMap<K, V>>>;
// pub type ArcTRwMap<K, V> = Arc<tokio::sync::RwLock<HashMap<K, V>>>;
// pub type ArcMutDMap<K, V> = Arc<tokio::sync::Mutex<HashMap<K, V>>>;
// pub type CrackedResult<T> = Result<T, CrackedError>;
// pub type CrackedHowResult<T> = anyhow::Result<T, CrackedError>;
use crack_types::{CrackHowResult, CrackResult, Error};

// pub type Command = poise::Command<Data, CommandError>;
// pub type Context<'a> = poise::Context<'a, Data, CommandError>;
// pub type PrefixContext<'a> = poise::PrefixContext<'a, Data, CommandError>;
// pub type PartialContext<'a> = poise::PartialContext<'a, Data, CommandError>;
// pub type ApplicationContext<'a> = poise::ApplicationContext<'a, Data, CommandError>;

// pub type CommandError = Error;
// pub type CommandResult<E = Error> = Result<(), E>;
// pub type FrameworkContext<'a> = poise::FrameworkContext<'a, Data, CommandError>;

// /// data struct for the bot, which is stored and accessible in all command invocations
// #[derive(clone, debug)]
// pub struct data(pub arc<datainner>);

// /// impl [`deref`] for our custom [`data`] struct
// impl std::ops::deref for data {
//     type target = datainner;

//     fn deref(&self) -> &self::target {
//         &self.0
//     }
// }

// /// impl for our custom [`data`] struct
// impl data {
//     /// insert a guild into the guild settings map.
//     pub async fn insert_guild(
//         &self,
//         guild_id: guildid,
//         guild_settings: guildsettings,
//     ) -> option<guildsettings> {
//         self.guild_settings_map
//             .write()
//             .await
//             .insert(guild_id, guild_settings)
//     }

//     /// create a new data, calls default
//     pub async fn downvote_track(
//         &self,
//         guild_id: guildid,
//         _track: &str,
//     ) -> result<trackreaction, crackederror> {
//         let pool = self.get_db_pool()?;
//         let play_log_id =
//             playlog::get_last_played_by_guild_metadata(&pool, guild_id.into(), 1).await?;
//         let pool = self.database_pool.as_ref().unwrap();
//         let id = *play_log_id.first().unwrap() as i32;
//         let _ = trackreaction::insert(pool, id).await?;
//         trackreaction::add_dislike(pool, id).await
//     }

//     /// add a message to the cache
//     pub async fn add_msg_to_cache(&self, guild_id: guildid, msg: message) -> option<message> {
//         let now = chrono::utc::now();
//         self.add_msg_to_cache_ts(guild_id.into(), now, msg).await
//     }

//     /// add a message to the cache
//     pub async fn add_msg_to_cache_int(&self, id: u64, msg: message) -> option<message> {
//         let now = chrono::utc::now();
//         self.add_msg_to_cache_ts(id, now, msg).await
//     }

//     /// add msg to the cache with a timestamp.
//     pub async fn add_msg_to_cache_ts(
//         &self,
//         id: u64,
//         ts: datetime<utc>,
//         msg: message,
//     ) -> option<message> {
//         self.id_cache_map
//             .entry(id)
//             .or_default()
//             .time_ordered_messages
//             .insert(ts, msg)
//     }

//     pub async fn add_reply_handle_to_cache(
//         &self,
//         guild_id: guildid,
//         handle: messageorreplyhandle,
//     ) -> option<messageorreplyhandle> {
//         let mut guild_msg_cache = self.guild_command_msg_queue.entry(guild_id).or_default();
//         guild_msg_cache.push(handle.clone());
//         some(handle)
//     }

//     /// remove and return a message from the cache based on the guild_id and timestamp.
//     pub async fn remove_msg_from_cache(
//         &self,
//         guild_id: guildid,
//         ts: datetime<utc>,
//     ) -> option<message> {
//         self.id_cache_map
//             .get_mut(&guild_id.into())
//             .unwrap()
//             .time_ordered_messages
//             .remove(&ts)
//     }

//     /// add the guild settings for a guild.
//     pub async fn add_guild_settings(&self, guild_id: guildid, settings: guildsettings) {
//         self.guild_settings_map
//             .write()
//             .await
//             .insert(guild_id, settings);
//     }

//     /// set the guild settings for a guild and return a new copy.
//     pub fn with_guild_settings_map(&self, guild_settings: guildsettingsmapparam) -> self {
//         self(arc::new(self.0.with_guild_settings_map(guild_settings)))
//     }

//     /// get the database pool for the postgresql database.
//     pub fn get_db_pool(&self) -> result<sqlx::pgpool, crackederror> {
//         self.database_pool
//             .as_ref()
//             .ok_or(crackederror::nodatabasepool)
//             .cloned()
//     }

//     /// reload the guild settings from the database.
//     pub async fn reload_guild_settings(&self, guild_id: guildid) -> result<(), crackederror> {
//         let pool = self.get_db_pool()?;
//         let guild_entity = guildentity::get(&pool, guild_id.into())
//             .await?
//             .ok_or(crackederror::noguildid)?;
//         let guild_settings = guild_entity.get_settings(&pool).await?;
//         self.guild_settings_map
//             .write()
//             .await
//             .insert(guild_id, guild_settings);
//         //let settings = guildsettings::get_by_guild_id(&pool, guild_id.into()).await?;
//         //self.add_guild_settings(guild_id, settings).await;
//         ok(())
//     }

//     /// deny a user permission to use the music commands.
//     pub async fn add_denied_music_user(
//         &self,
//         guild_id: guildid,
//         user: userid,
//     ) -> crackedresult<bool> {
//         self.guild_settings_map
//             .write()
//             .await
//             .entry(guild_id)
//             .or_insert_with(guildsettings::default)
//             .add_denied_music_user(user)
//             .await
//     }

//     /// check if a user is allowed to use the music commands.
//     pub async fn check_music_permissions(&self, guild_id: guildid, user: userid) -> bool {
//         if let some(settings) = self.guild_settings_map.read().await.get(&guild_id).cloned() {
//             settings
//                 .get_music_permissions()
//                 .map(|x| x.is_user_allowed(user.get()))
//                 .unwrap_or(true)
//         } else {
//             true
//         }
//     }

//     /// push a message to the command message queue.
//     pub async fn push_latest_msg(
//         &self,
//         guild_id: guildid,
//         msg: messageorreplyhandle,
//     ) -> crackedresult<()> {
//         self.guild_command_msg_queue
//             .entry(guild_id)
//             .or_default()
//             .push(msg);
//         ok(())
//     }

//     /// forget all skip votes for a guild
//     // this is used when a track ends, or when a user leaves the voice channel.
//     // this is to prevent users from voting to skip a track, then leaving the voice channel.
//     // todo: should this be moved to a separate module? or should it be moved to a separate file?
//     pub async fn forget_skip_votes(&self, guild_id: guildid) -> result<(), error> {
//         let _res = self
//             .guild_cache_map
//             .lock()
//             .await
//             .entry(guild_id)
//             .and_modify(|cache| cache.current_skip_votes = hashset::new())
//             .or_default();

//         ok(())
//     }

//     pub async fn with_bot_settings(&self, bot_settings: botconfig) -> self {
//         self(arc::new(self.0.with_bot_settings(bot_settings)))
//     }

//     pub fn with_songbird(&self, songbird: arc<songbird::songbird>) -> self {
//         self(self.arc_inner().with_songbird(songbird).into())
//     }

//     pub fn arc_inner(&self) -> arc<datainner> {
//         into::into(self.0.clone())
//     }
// }

// #[cfg(test)]
// mod lib_test {
//     use super::*;
//     use serde_json::json;

//     #[test]
//     fn test_phone_code_data() {
//         let data = phonecodedata::load().unwrap();
//         let country_names = data.country_names;
//         let phone_codes = data.phone_codes;
//         let country_by_phone_code = data.country_by_phone_code;

//         assert_eq!(country_names.get("us"), some(&"united states".to_string()));
//         assert_eq!(phone_codes.get("is"), some(&"354".to_string()));
//         let want = &vec!["ca".to_string(), "um".to_string(), "us".to_string()];
//         let got = country_by_phone_code.get("1").unwrap();
//         // this would be cheaper using a heap or tree
//         assert!(got.iter().all(|x| want.contains(x)));
//         assert!(want.iter().all(|x| got.contains(x)));
//     }

//     /// test the creation of a default eventlog
//     #[tokio::test]
//     async fn test_event_log_default() {
//         let event_log = eventlogasync::default();
//         let file = event_log.lock().await;
//         assert_eq!(file.metadata().unwrap().len(), 0);
//     }

//     /// test the creation and printing of camkickconfig
//     #[test]
//     fn test_display_cam_kick_config() {
//         let cam_kick = camkickconfig::default();
//         let want = r#"timeout:       0
// guild_id:      0
// chan_id:       0
// dc_msg:        "you have been violated for being cammed down for too long."
// msg_on_deafen: false
// msg_on_mute:   false
// msg_on_dc:     false
// "#;
//         assert_eq!(cam_kick.to_string(), want);
//     }

//     #[tokio::test]
//     async fn test_with_data_inner() {
//         let data = datainner::default();
//         let new_data = data.with_bot_settings(botconfig::default());
//         assert_eq!(json!(new_data.bot_settings), json!(botconfig::default()));

//         let guild_settings = guildsettingsmapparam::default();
//         let new_data = new_data.with_guild_settings_map(guild_settings);
//         assert!(new_data.guild_settings_map.read().await.is_empty());
//     }
// }
