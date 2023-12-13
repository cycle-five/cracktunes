use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use serenity::all::{Member, Role};

/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn assign_role(
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
