// pub mod admin;
pub mod authorize;
pub mod ban;
pub mod broadcast_voice;
pub mod create_role;
pub mod create_text_channel;
pub mod create_voice_channel;
pub mod deafen;
pub mod deauthorize;
pub mod delete_channel;
pub mod get_settings;
pub mod kick;
pub mod mute;
pub mod print_settings;
pub mod set_all_log_channel;
pub mod set_all_log_channel_data;
pub mod set_all_log_channel_old_data;
pub mod set_prefix;
pub mod set_welcome_settings;
pub mod unban;
pub mod unmute;

// pub use admin::*;
use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::create_response_poise,
    Context, Error,
};
pub use authorize::*;
pub use ban::*;
pub use broadcast_voice::*;
pub use create_role::*;
pub use create_text_channel::*;
pub use create_voice_channel::*;
pub use deafen::*;
pub use deauthorize::*;
pub use delete_channel::*;
pub use get_settings::*;
pub use kick::*;
pub use mute::*;
pub use print_settings::*;
use serenity::all::{Role, RoleId};
pub use set_all_log_channel::*;
pub use set_all_log_channel_data::*;
pub use set_all_log_channel_old_data::*;
pub use set_prefix::*;
pub use set_welcome_settings::*;
pub use unban::*;
pub use unmute::*;
// use chrono::NaiveTime;
// use date_time_parser::TimeParser;

/// Admin commands.
#[poise::command(
    prefix_command,
    //slash_command,
    subcommands(
        "authorize",
        "ban",
        "broadcast_voice",
        "create_role",
        "create_text_channel",
        "create_voice_channel",
        "deafen",
        "deauthorize",
        "delete_channel",
        "get_settings",
        "kick",
        "mute",
        "print_settings",
        "set_all_log_channel",
        "set_prefix",
        "set_welcome_settings",
        "unban",
        "unmute",
    ),
    ephemeral,
    owners_only
)]
pub async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    Ok(())
}

/// Delete role.
#[poise::command(prefix_command, , owners_only, ephemeral)]
pub async fn delete_role(ctx: Context<'_>, role_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let role = guild
                .roles(&ctx)
                .await?
                .into_iter()
                .find(|r| r.name == role_name);
            if let Some(role) = role {
                if let Err(e) = role.delete(&ctx).await {
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
                            role_name: role_name.clone(),
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
                CrackedError::Other("This command can only be used in a guild.".to_string()).into(),
            );
        }
    }
    Ok(())
}

/// Delete category.
#[poise::command(prefix_command, , owners_only, ephemeral)]
pub async fn delete_category(ctx: Context<'_>, category_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let category = guild
                .channels(&ctx)
                .await?
                .into_iter()
                .find(|c| c.name == category_name);
            if let Some(category) = category {
                if let Err(e) = category.delete(&ctx).await {
                    // Handle error, send error message
                    create_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to delete category: {}", e)),
                    )
                    .await?;
                } else {
                    // Send success message
                    create_response_poise(
                        ctx,
                        CrackedMessage::CategoryDeleted {
                            category_name: category_name.clone(),
                        },
                    )
                    .await?;
                }
            } else {
                create_response_poise(
                    ctx,
                    CrackedMessage::Other("Category not found.".to_string()),
                )
                .await?;
            }
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.".to_string()).into(),
            );
        }
    }
    Ok(())
}

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_role(
    ctx: Context<'_>,
    #[description = "Role to delete."] mut role: Role,
) -> Result<(), Error> {
    role.delete(&ctx).await.map_err(Into::into)
}

#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_role_by_id(
    ctx: Context<'_>,
    #[description = "RoleId to delete."] mut role_id: RoleId,
) -> Result<(), Error> {
    delete_role_by_id(ctx, role_id.into()).await
}

/// Delete role.
pub async fn delete_role_by_id(ctx: Context<'_>, role_id: u64) -> Result<(), Error> {
    let role_id = RoleId::new(role_id);
    match ctx.guild_id() {
        Some(guild) => {
            let mut role = guild
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
