use crate::guild::settings::DEFAULT_PREFIX;
use crate::guild::settings::DEFAULT_VOLUME_LEVEL;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    sync::{Arc, Mutex},
};

pub mod client;
pub mod commands;
pub mod connection;
pub mod errors;
pub mod guild;
pub mod handlers;
pub mod messaging;
pub mod sources;
pub mod utils;

#[cfg(test)]
pub mod test;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Arc<Data>, Error>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CamKickConfig {
    pub cammed_down_timeout: u64,
    pub guild_id: u64,
    pub channel_id: u64,
    pub dc_message: String,
}

impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            cammed_down_timeout: 30,
            guild_id: 0,
            channel_id: 0,
            dc_message: "You have been disconnected for being cammed down for too long."
                .to_string(),
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
        write!(f, "{}", result)
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BotConfig {
    pub video_status_poll_interval: u64,
    pub authorized_users: Vec<u64>,
    // Cammed down kicking config
    pub cam_kick: Vec<CamKickConfig>,
    pub sys_log_channel_id: u64,
    pub self_deafen: bool,
    pub volume: f32,
    pub guild_settings_map: Vec<guild::settings::GuildSettings>,
    pub prefix: Vec<u8>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: 20,
            authorized_users: vec![],
            cam_kick: vec![],
            sys_log_channel_id: 0,
            self_deafen: true,
            volume: 0.2,
            guild_settings_map: vec![],
            prefix: DEFAULT_PREFIX.to_string().into_bytes(),
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
        result.push_str(&format!("authorized_users: {:?}\n", self.authorized_users));
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
            String::from_utf8(self.prefix.clone()).unwrap()
        ));
        write!(f, "{}", result)
    }
}

impl BotConfig {
    pub fn get_prefix(&self) -> String {
        String::from_utf8(self.prefix.clone()).unwrap()
    }
}

/// User data, which is stored and accessible in all command invocations
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Data {
    pub bot_settings: BotConfig,
    // TODO: Make this a HashMap, pointing to a settings struct containiong
    // user priviledges, etc
    pub authorized_users: HashSet<u64>,
    pub volume: Arc<Mutex<f32>>,
    pub guild_settings_map: Arc<Mutex<HashMap<u64, guild::settings::GuildSettings>>>,
    #[serde(skip)]
    pub guild_cache_map: Arc<Mutex<HashMap<u64, guild::cache::GuildCache>>>,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            bot_settings: Default::default(),
            authorized_users: Default::default(),
            volume: Arc::new(Mutex::new(DEFAULT_VOLUME_LEVEL)),
            guild_settings_map: Arc::new(Mutex::new(HashMap::new())),
            guild_cache_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
