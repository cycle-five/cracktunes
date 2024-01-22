use serenity::all::GuildId;

use crate::{Context, Data, Error};

/// Get the current `premium` setting for the guild.
pub fn get_premium(data: &Data, guild_id: GuildId) -> bool {
    let guild_settings_map = data.guild_settings_map.read().unwrap();
    let guild_settings = guild_settings_map.get(&guild_id).unwrap();
    guild_settings.premium
}

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral, aliases("get_premium_status"))]
pub async fn premium(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let res = get_premium(data, guild_id);

    ctx.say(format!("Premium status: {}", res))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
