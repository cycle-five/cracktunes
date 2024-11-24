use crate::commands::help;
use crate::{Context, Error};
use serenity::all::{GuildId, Member, Role, RoleId, UserId};

/// Assign role.
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    prefix_command,
    slash_command,
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn assign(
    ctx: Context<'_>,
    #[description = "Role to assign"] role: Role,
    #[description = "Member to assign the role to"] member: Member,
    #[flag]
    #[description = "Show help menu"]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    member
        .add_role(ctx.http(), role.id, None)
        .await
        .map(|_| ())
        .map_err(|e| e.into())
}

/// Assign role.
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    prefix_command,
    slash_command,
    hide_in_help = true,
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn assign_ids(
    ctx: Context<'_>,
    #[description = "GuildId related to"] guild_id: GuildId,
    #[description = "RoleId to assign"] role_id: RoleId,
    #[description = "UserId to assign role to"] user_id: UserId,
    #[description = "Show help menu"] help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }

    let member = guild_id.member(&ctx, user_id).await?;
    member
        .add_role(ctx.http(), role_id, None)
        .await
        .map(|_| ())
        .map_err(|e| e.into())
}
