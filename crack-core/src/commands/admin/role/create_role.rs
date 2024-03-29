use serenity::builder::EditRole;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Name of the role to create"] role_name: String,
) -> Result<(), Error> {
    let (_guild_id, guild) = match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            (guild.id, guild)
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    };

    match guild
        .create_role(&ctx, EditRole::new().name(role_name))
        .await
    {
        Err(e) => {
            // Handle error, send error message
            send_response_poise(
                ctx,
                CrackedMessage::Other(format!("Failed to create role: {}", e)),
                true,
            )
            .await
            .map(|_| ())
        }
        Ok(role) => {
            // Send success message
            send_response_poise(
                ctx,
                CrackedMessage::RoleCreated {
                    role_name: role.name.clone(),
                    role_id: role.id,
                },
                true,
            )
            .await
            .map(|_| ())
        }
    }
}
