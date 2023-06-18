use std::time::Duration;

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

use poise::serenity_prelude::GuildId;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct CamKickConfig {
    pub cammed_down_timeout: Duration,
    pub guild_id: GuildId,
    pub poll_secs: u64,
    pub dc_message: String,
}

impl Default for CamKickConfig {
    fn default() -> Self {
        Self {
            cammed_down_timeout: Duration::from_secs(0),
            guild_id: GuildId(0),
            poll_secs: 60,
            dc_message: "You have been disconnected for being cammed down for too long."
                .to_string(),
        }
    }
}
#[derive(Deserialize, Clone)]
pub struct BotConfig {
    pub video_status_poll_interval: Option<Duration>,
    // Cammed down kicking config
    pub cam_kick: Vec<CamKickConfig>,
    pub sys_log_channel_id: Option<u64>,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            video_status_poll_interval: None,
            cam_kick: vec![],
            sys_log_channel_id: None,
        }
    }
}
#[derive(Deserialize, Clone)]
pub struct Data {
    pub bot_settings: BotConfig,
}
