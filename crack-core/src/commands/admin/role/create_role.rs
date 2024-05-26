use serenity::builder::EditRole;

use crate::commands::ConvertToEmptyResult;
use crate::commands::EmptyResult;
use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
//use crate::Error;

/// Create role.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn create(
    ctx: Context<'_>,
    #[description = "Name of the role to create"] role_name: String,
) -> EmptyResult {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;

    let role = guild_id
        .create_role(&ctx, EditRole::new().name(role_name))
        .await?;

    send_response_poise(
        ctx,
        CrackedMessage::RoleCreated {
            role_name: role.name.clone(),
            role_id: role.id,
        },
        true,
    )
    .await
    .convert()
}
