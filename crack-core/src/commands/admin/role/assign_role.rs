use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use serenity::all::{GuildId, Member, Role, RoleId, UserId};

/// Assign role.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn assign(
    ctx: Context<'_>,
    #[description = "Role to assign"] role: Role,
    #[description = "Member to assign the role to"] member: Member,
) -> Result<(), Error> {
    let _guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    member
        .add_role(&ctx, role)
        .await
        .map(|_| ())
        .map_err(|e| e.into())
}

/// Assign role.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn assign_ids(
    ctx: Context<'_>,
    #[description = "GuildId related to"] guild_id: GuildId,
    #[description = "RoleId to assign"] role_id: RoleId,
    #[description = "UserId to assign role to"] user_id: UserId,
) -> Result<(), Error> {
    // let _guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let member = guild_id.member(&ctx, user_id).await?;
    member
        .add_role(&ctx, role_id)
        .await
        .map(|_| ())
        .map_err(|e| e.into())
}
