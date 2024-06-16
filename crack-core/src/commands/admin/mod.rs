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

use crate::{Context, Error};
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

use crate::commands::sub_help as help;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
/// Admin commands.
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
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
        "kick",
        "rename_all",
        "mute",
        "message_cache",
        "move_users_to",
        "unban",
        "undeafen",
        "unmute",
        "random_mute",
        "get_active_vcs",
        "set_vc_size",
        "role",
        "timeout",
        "help"
    ),
    ephemeral,
    // owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn admin(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    let msg = CrackedMessage::Other("Admin command called".to_string());
    send_reply(&ctx, msg, true).await?;

    Ok(())
}

/// List of all the admin commands.
pub fn admin_commands() -> Vec<crate::Command> {
    role::role_commands().into_iter().collect()
}
