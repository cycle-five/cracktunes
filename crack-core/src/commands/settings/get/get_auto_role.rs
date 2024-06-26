use serenity::all::GuildId;
use serenity::model::id::RoleId;

use crate::commands::CrackedError;
use crate::guild::operations::GuildSettingsOperations;
use crate::{messaging::message::CrackedMessage, utils::send_reply};
use crate::{Context, Data, Error};

/// Get the auto role for the server.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    aliases("auto_role")
)]
pub async fn get_auto_role(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show the help menu for this command."]
    flag: bool,
) -> Result<(), Error> {
    if flag {
        return crate::commands::help::wrapper(ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    get_auto_role_internal(data, guild_id)
        .await
        .map_or_else(
            || {
                send_reply(
                    &ctx,
                    CrackedMessage::Other("No auto role set for this server.".to_string()),
                    true,
                )
            },
            |role_id| {
                send_reply(
                    &ctx,
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
    data.get_auto_role(guild_id)
        .await
        .map(|role_id| RoleId::from(role_id))
}
