use std::fmt::Display;

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
type Context<'a> = poise::Context<'a, Data, Error>;
// User data, which is stored and accessible in all command invocations

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct CamKickConfig {
    pub cammed_down_timeout: u64,
    pub guild_id: u64,
    pub poll_secs: u64,
    pub dc_message: String,
}

impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            cammed_down_timeout: 0, //Duration::from_secs(0),
            guild_id: 0,            //GuildId(0),
            poll_secs: 60,
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
        result.push_str(&format!("poll_secs: {:?}\n", self.poll_secs));
        result.push_str(&format!("dc_message: {:?}\n", self.dc_message));
        write!(f, "{}", result)
    }
}
#[derive(Deserialize, Clone, Debug)]
pub struct BotConfig {
    pub video_status_poll_interval: u64,
    // Cammed down kicking config
    pub cam_kick: Vec<CamKickConfig>,
    pub sys_log_channel_id: u64,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: 0,
            cam_kick: vec![],
            sys_log_channel_id: 0,
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
        result.push_str(&format!("cam_kick: {:?}\n", self.cam_kick));
        result.push_str(&format!(
            "sys_log_channel_id: {:?}\n",
            self.sys_log_channel_id
        ));
        write!(f, "{}", result)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Data {
    pub bot_settings: BotConfig,
}
