use serenity::all::{Role, RoleId};

use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::create_response_poise,
    Context, Error,
};

/// Delete role.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_role(
    ctx: Context<'_>,
    #[description = "Role to delete."] mut role: Role,
) -> Result<(), Error> {
    role.delete(&ctx).await.map_err(Into::into)
}

/// Delete role by id
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_role_by_id(
    ctx: Context<'_>,
    #[description = "RoleId to delete."] role_id: RoleId,
) -> Result<(), Error> {
    delete_role_by_id_helper(ctx, role_id.into()).await
}

/// Delete role helper.
pub async fn delete_role_by_id_helper(ctx: Context<'_>, role_id: u64) -> Result<(), Error> {
    let role_id = RoleId::new(role_id);
    match ctx.guild_id() {
        Some(guild) => {
            let role = guild
                .roles(&ctx)
                .await?
                .into_iter()
                .find(|r| r.0 == role_id);
            if let Some(mut role) = role {
                if let Err(e) = role.1.delete(&ctx).await {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to delete role: {}", e)),
                    )
                    .await?;
                } else {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::RoleDeleted {
                            role_name: role.1.name.clone(),
                            role_id,
                        },
                    )
                    .await?;
                }
            } else {
                create_response_poise(ctx, CrackedMessage::Other("Role not found.".to_string()))
                    .await?;
            }
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    }
    Ok(())
}
