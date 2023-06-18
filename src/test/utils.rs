use config_file::FromConfigFile;
use std::time::Duration;

use poise::serenity_prelude::GuildId;

use crate::{utils::get_human_readable_timestamp, BotConfig};

#[test]
fn test_get_human_readable_timestamp() {
    let duration = Duration::from_secs(53);
    let result = get_human_readable_timestamp(Some(duration));
    assert_eq!(result, "00:53");

    let duration = Duration::from_secs(3599);
    let result = get_human_readable_timestamp(Some(duration));
    assert_eq!(result, "59:59");

    let duration = Duration::from_secs(96548);
    let result = get_human_readable_timestamp(Some(duration));
    assert_eq!(result, "26:49:08");

    let result = get_human_readable_timestamp(Some(Duration::MAX));
    assert_eq!(result, "∞");

    let result = get_human_readable_timestamp(None);
    assert_eq!(result, "∞");
}

#[test]
fn test_load_config() {
    let config = match BotConfig::from_config_file("./../../cracktunes.toml") {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("Using default config: {}", error);
            BotConfig::default()
        }
    };

    println!("config: {:?}", config);

    assert_eq!(config.cam_kick.len(), 2);
    assert_eq!(config.cam_kick[0].guild_id, *GuildId(0).as_u64());
}
