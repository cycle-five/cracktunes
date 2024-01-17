use serenity::all::GuildId;

use crate::{Context, Data, Error};

/// Get the current `volume` and `old_volume` setting for the guild.
pub fn get_volume(data: &Data, guild_id: GuildId) -> (float, float) {
    let guild_settings_map = data.guild_settings_map.read().unwrap();
    let guild_settings = guild_settings_map.get(&guild_id).unwrap();
    (guild_settings.volume, guild_settings.old_volume)
}

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn volume(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let (vol, old_vol) = get_volume(data, guild_id);

    ctx.say(format!("vol: {}, old_vol: {}", vol, old_vol))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
