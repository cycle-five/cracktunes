use serenity::builder::EditRole;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::create_response_poise;
use crate::Context;
use crate::Error;
/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create_role(ctx: Context<'_>, role_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            match guild
                .create_role(&ctx, EditRole::new().name(role_name))
                .await
            {
                Err(e) => {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to create role: {}", e)),
                    )
                    .await?;
                }
                Ok(role) => {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::RoleCreated {
                            role_name: role.name.clone(),
                            role_id: role.id,
                        },
                    )
                    .await?;
                }
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
