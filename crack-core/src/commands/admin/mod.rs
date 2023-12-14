pub mod audit_logs;
pub mod authorize;
pub mod ban;
pub mod broadcast_voice;
pub mod create_text_channel;
pub mod create_voice_channel;
pub mod deafen;
pub mod deauthorize;
pub mod debug;
pub mod defend;
pub mod delete_channel;
pub mod get_active;
pub mod invite_tracker;
pub mod kick;
pub mod message_cache;
pub mod move_users;
pub mod mute;
pub mod random_mute_lol;
pub mod role;
pub mod set_vc_size;
pub mod timeout;
pub mod unban;
pub mod unmute;

use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_response_poise, Context,
    Error,
};
pub use audit_logs::*;
pub use authorize::*;
pub use ban::*;
pub use broadcast_voice::*;
pub use create_text_channel::*;
pub use create_voice_channel::*;
pub use deafen::*;
pub use deauthorize::*;
pub use debug::*;
pub use defend::*;
pub use delete_channel::*;
pub use get_active::*;
pub use invite_tracker::track_invites;
pub use kick::*;
pub use message_cache::*;
pub use move_users::*;
pub use mute::*;
pub use random_mute_lol::*;
pub use role::*;
pub use set_vc_size::*;
pub use timeout::*;
pub use unban::*;
pub use unmute::*;

/// Admin commands.
#[poise::command(
    prefix_command,
    //slash_command,
    subcommands(
        "audit_logs",
        "authorize",
        "ban",
        "broadcast_voice",
        "create_text_channel",
        "create_voice_channel",
        "deafen",
        "defend",
        "deauthorize",
        "delete_channel",
        "track_invites",
        "kick",
        "mute",
        "message_cache",
        "move_users_to",
        "unban",
        "unmute",
        "random_mute",
        "get_active_vcs",
        "set_vc_size",
        "role",
    ),
    ephemeral,
    owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn admin(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    ctx.say("You found the admin command").await?;

    Ok(())
}

/// Delete category.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn delete_category(ctx: Context<'_>, category_name: String) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let category = guild
                .channels(&ctx)
                .await?
                .into_iter()
                .find(|c| c.1.name == category_name);
            if let Some(category) = category {
                if let Err(e) = category.1.delete(&ctx).await {
                    // Handle error, send error message
                    send_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Failed to delete category: {}", e)),
                    )
                    .await?;
                } else {
                    // Send success message
                    send_response_poise(
                        ctx,
                        CrackedMessage::Other(format!("Category deleted: {}", category_name)),
                    )
                    .await?;
                }
            } else {
                send_response_poise(
                    ctx,
                    CrackedMessage::Other("Category not found.".to_string()),
                )
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
