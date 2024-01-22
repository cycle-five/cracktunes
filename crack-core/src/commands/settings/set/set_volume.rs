use crate::{
    guild::settings::GuildSettingsMapParam,
    guild::settings::{GuildSettings, DEFAULT_PREFIX},
    Context, Error,
};
use serenity::all::GuildId;

/// Get the current `volume` and `old_volume` setting for the guild.
pub fn set_volume(
    guild_settings_map: GuildSettingsMapParam,
    guild_id: GuildId,
    vol: f32,
) -> (f32, f32) {
    let guild_settings = {
        let mut guild_settings_mut = guild_settings_map.write().unwrap();
        guild_settings_mut
            .entry(guild_id)
            .and_modify(|e| {
                e.volume = vol;
                e.old_volume = e.volume;
            })
            .or_insert(GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), None))
            .clone()
    };
    (guild_settings.volume, guild_settings.old_volume)
}

/// Set the volume for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn volume(
    ctx: Context<'_>,
    #[description = "Volume to set the bot settings to"] volume: f32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let (vol, old_vol) = {
        let guild_settings_map = ctx.data().guild_settings_map.clone();
        set_volume(guild_settings_map, guild_id, volume)
    };

    ctx.say(format!("vol: {}, old_vol: {}", vol, old_vol))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
