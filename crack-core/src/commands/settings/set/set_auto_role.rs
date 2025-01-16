use poise::serenity_prelude::Mentionable;
use serenity::all::{Role, RoleId};

use crate::{
    guild::operations::GuildSettingsOperations, http_utils::SendMessageParams,
    messaging::message::CrackedMessage, poise_ext::PoiseContextExt, Context, Error,
};
use crack_types::CrackedError;

/// Set the auto role for the server.
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn auto_role_id(
    ctx: Context<'_>,
    #[description = "The id of the role to assign to new users."] auto_role_id_str: String,
    #[flag]
    #[description = "Show the help menu for this command"]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    let auto_role_id = match auto_role_id_str.parse::<u64>() {
        Ok(x) => x,
        Err(e) => {
            ctx.say(format!("Failed to parse role id: {e}")).await?;
            return Ok(());
        },
    };
    let role = RoleId::from(auto_role_id);

    auto_role_internal(ctx, role).await
}

/// Set the auto role for the server.
#[poise::command(
    category = "Settings",
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "MANAGE_ROLES"
)]
pub async fn auto_role(
    ctx: Context<'_>,
    #[description = "The role to assign to new users"] auto_role: Role,
    #[flag]
    #[description = "Show the help menu for this command"]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    auto_role_internal(ctx, auto_role.id).await
}

/// Set the auto role for the server.
pub async fn auto_role_internal(ctx: Context<'_>, auto_role: RoleId) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mention = auto_role.mention();

    ctx.data().set_auto_role(guild_id, auto_role.get()).await;
    let res = ctx
        .data()
        .get_guild_settings(guild_id)
        .await
        .ok_or(CrackedError::NoGuildSettings)?;
    res.save(&ctx.data().database_pool.clone().unwrap()).await?;

    //ctx.say(format!("Auto role set to {}", mention)).await?;
    let params =
        SendMessageParams::new(CrackedMessage::Other(format!("Auto role set to {mention}")));
    ctx.send_message(params)
        .await
        .map(|_| ())
        .map_err(Into::into)
}
