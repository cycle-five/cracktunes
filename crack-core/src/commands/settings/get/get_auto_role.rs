use serenity::all::GuildId;
use serenity::model::id::RoleId;

use crate::{messaging::message::CrackedMessage, utils::send_reply};
use crate::{Context, Data, Error};

/// Get the auto role for the server.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral,
    aliases("get_auto_role")
)]
pub async fn auto_role(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    get_auto_role_internal(data, guild_id)
        .await
        .map_or_else(
            || {
                send_reply(
                    ctx,
                    CrackedMessage::Other("No auto role set for this server.".to_string()),
                    true,
                )
            },
            |role_id| {
                send_reply(
                    ctx,
                    CrackedMessage::Other(format!("Auto role: <@&{}>", role_id)),
                    true,
                )
            },
        )
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Get the auto role for the server.
pub async fn get_auto_role_internal(data: &Data, guild_id: GuildId) -> Option<RoleId> {
    let guild_settings_map = data.guild_settings_map.read().await;
    let guild_settings = guild_settings_map.get(&guild_id)?;
    guild_settings
        .welcome_settings
        .as_ref()
        .map(|x| x.auto_role.map(|y| y.into()).unwrap_or_default())
}
