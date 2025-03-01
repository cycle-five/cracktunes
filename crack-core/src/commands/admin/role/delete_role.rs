use poise::ReplyHandle;
use serenity::all::{Role, RoleId};
use std::borrow::Cow;

use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply_owned, Context,
    Error,
};

/// Delete role.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    prefix_command,
    slash_command,
    ephemeral
)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "Role to delete."] mut role: Role,
) -> Result<(), Error> {
    role.delete(ctx.http(), None).await.map_err(Into::into)
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

#[cfg(not(tarpaulin_include))]
/// Delete role helper.
pub async fn delete_role_by_id_helper(
    ctx: Context<'_>,
    role_id: u64,
) -> Result<ReplyHandle<'_>, CrackedError> {
    let role_id = RoleId::new(role_id);
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let mut role = guild_id
        .roles(ctx.http())
        .await?
        .into_iter()
        .find(|r| r.id == role_id)
        .ok_or(CrackedError::RoleNotFound(role_id))?;
    role.delete(ctx.http(), None).await?;
    // Send success message
    // let role_name: Cow<'_, String> = Cow::Owned(role.name.to_string());
    let role_name = role.name;
    send_reply_owned(
        ctx,
        CrackedMessage::RoleDeleted { role_id, role_name },
        true,
    )
    .await
}
