use config_file::FromConfigFile;
use poise::serenity_prelude::GuildId;
use std::{num::NonZeroU64, time::Duration};

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
    let config = BotConfig::from_config_file("./src/test/cracktunes.toml").unwrap();

    println!("config: {:?}", config);

    assert_eq!(config.cam_kick.len(), 2);
    assert_eq!(config.cam_kick[0].guild_id, 0);
    assert_eq!(config.guild_settings_map.len(), 2);
    assert_eq!(
        config.guild_settings_map[0].welcome_settings.is_some(),
        true
    );
    assert_eq!(
        config.guild_settings_map[1].welcome_settings.is_some(),
        false
    );
}
