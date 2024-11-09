use config_file::FromConfigFile;
use crack_core::BotConfig;
use crack_types::get_human_readable_timestamp;
use poise::serenity_prelude::GuildId;
use std::time::Duration;

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
#[ignore]
fn test_load_config() {
    let config = match BotConfig::from_config_file("./src/test/cracktunes.toml") {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Error loading config: {:?}", e);
            panic!("Error loading config: {:?}", e);
        },
    };

    println!("config: {:?}", config);

    let cam_kick = config.cam_kick.unwrap();
    let guild_settings_map = match config.guild_settings_map {
        Some(map) => map,
        None => {
            tracing::error!("guild_settings_map is None");
            panic!("guild_settings_map is None");
        },
    };

    assert_eq!(cam_kick.len(), 2);
    assert_eq!(cam_kick[0].guild_id, GuildId::new(1).get());
    assert_eq!(guild_settings_map.len(), 2);
    assert_eq!(guild_settings_map[0].welcome_settings.is_some(), true);
    assert_eq!(guild_settings_map[1].welcome_settings.is_some(), false);
}

#[test]
fn test_load_config2() {
    let config = BotConfig::from_config_file("./src/test/cracktunes2.toml").unwrap();

    println!("config: {:?}", config);

    // Test defaults here, this should be empty.
}
