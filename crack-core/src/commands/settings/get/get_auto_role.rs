use serenity::all::GuildId;
use serenity::model::id::RoleId;

use crate::{messaging::message::CrackedMessage, utils::send_response_poise};
use crate::{Context, Data, Error};

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral, aliases("get_auto_role"))]
pub async fn auto_role(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    get_auto_role(data, guild_id)
        .map_or_else(
            || {
                send_response_poise(
                    ctx,
                    CrackedMessage::Other("No auto role set for this server.".to_string()),
                )
            },
            |role_id| {
                send_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Auto role: <@&{}>", role_id)),
                )
            },
        )
        .await
        .map(|_| ())
}

/// Get the auto role for the server.
pub fn get_auto_role(data: &Data, guild_id: GuildId) -> Option<RoleId> {
    let guild_settings_map = data.guild_settings_map.read().unwrap();
    let guild_settings = guild_settings_map.get(&guild_id)?;
    guild_settings
        .welcome_settings
        .as_ref()
        .map(|x| x.auto_role.map(|y| y.into()).unwrap_or_default())
}
