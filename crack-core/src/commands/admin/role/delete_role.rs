use poise::ReplyHandle;
use serenity::all::{Role, RoleId};

use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply, Context, Error,
};

/// Delete role.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    prefix_command,
    hide_in_help = true,
    ephemeral
)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Role to delete."] mut role: Role,
) -> Result<(), Error> {
    role.delete(&ctx).await.map_err(Into::into)
}

/// Delete role by id
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    prefix_command,
    hide_in_help = true,
    ephemeral
)]
pub async fn delete_by_id(
    ctx: Context<'_>,
    #[description = "RoleId to delete."] role_id: RoleId,
) -> Result<(), Error> {
    delete_role_by_id_helper(ctx, role_id.into())
        .await
        .map_err(Into::into)
        .map(|_| ())
}

/// Delete role helper.
pub async fn delete_role_by_id_helper(
    ctx: Context<'_>,
    role_id: u64,
) -> Result<ReplyHandle<'_>, CrackedError> {
    let role_id = RoleId::new(role_id);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mut role = guild_id
        .roles(&ctx)
        .await?
        .into_iter()
        .find(|r| r.0 == role_id)
        .ok_or(CrackedError::RoleNotFound(role_id))?;
    role.1.delete(&ctx).await?;
    // Send success message
    send_reply(
        ctx,
        CrackedMessage::RoleDeleted {
            role_name: role.1.name.clone(),
            role_id,
        },
        true,
    )
    .await
}
