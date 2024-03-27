use crate::{guild::settings::GuildSettings, Context, Error};
use serenity::all::GuildId;
use std::sync::RwLock;
use std::{collections::HashMap, sync::Arc};

/// Get the current `volume` and `old_volume` setting for the guild.
pub fn get_volume(
    guild_settings_map: Arc<RwLock<HashMap<GuildId, GuildSettings>>>,
    guild_id: GuildId,
) -> (f32, f32) {
    let guild_settings_map = guild_settings_map.read().unwrap();
    let guild_settings = guild_settings_map.get(&guild_id).unwrap();
    (guild_settings.volume, guild_settings.old_volume)
}

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn volume(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let (vol, old_vol) = get_volume(data.guild_settings_map.clone(), guild_id);

    ctx.say(format!("vol: {}, old_vol: {}", vol, old_vol))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}

#[cfg(test)]
mod test {
    use crate::guild::settings::DEFAULT_VOLUME_LEVEL;

    #[tokio::test]
    async fn test_volume() {
        use super::get_volume;
        use crate::guild::settings::GuildSettings;
        use serenity::model::id::GuildId;
        use std::sync::{Arc, RwLock};
        let guild_settings_map = Arc::new(RwLock::new(std::collections::HashMap::new()));

        let guild_id = GuildId::new(1);
        let _guild_settings = guild_settings_map
            .write()
            .unwrap()
            .entry(guild_id)
            .or_insert(GuildSettings::new(guild_id, Some("!"), None));
        let (vol, old_vol) = get_volume(guild_settings_map.clone(), guild_id);
        assert_eq!(vol, DEFAULT_VOLUME_LEVEL);
        assert_eq!(old_vol, DEFAULT_VOLUME_LEVEL);

        let (vol, old_vol) = get_volume(guild_settings_map.clone(), guild_id);
        assert_eq!(vol, DEFAULT_VOLUME_LEVEL);
        assert_eq!(old_vol, DEFAULT_VOLUME_LEVEL);
    }
}
