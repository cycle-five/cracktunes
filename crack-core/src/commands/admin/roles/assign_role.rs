use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::{Member, Role};
use serenity::builder::EditRole;

/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
#[cfg(not(tarpaulin_include))]
pub async fn create_role(
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
