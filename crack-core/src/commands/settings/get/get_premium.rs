use serenity::all::GuildId;

use crate::{Context, Data, Error};

/// Get the current `premium` setting for the guild.
pub async fn get_premium(data: &Data, guild_id: GuildId) -> bool {
    let guild_settings_map = data.guild_settings_map.read().await;
    let guild_settings = guild_settings_map.get(&guild_id).unwrap();
    guild_settings.premium
}

/// Get the current `premium` setting for the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral,
    aliases("get_premium_status")
)]
pub async fn premium(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let res = get_premium(data, guild_id).await;

    ctx.say(format!("Premium status: {}", res))
        .await
        .map_err(|e| e.into())
        .map(|_| ())
}
