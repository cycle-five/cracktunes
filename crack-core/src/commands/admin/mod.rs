pub mod audit_logs;
pub mod authorize;
pub mod broadcast_voice;
pub mod create_text_channel;
pub mod create_voice_channel;
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
pub mod unmute;
pub mod user;

use crate::{Context, Error};
pub use audit_logs::*;
pub use authorize::*;
pub use broadcast_voice::*;
pub use create_text_channel::*;
pub use create_voice_channel::*;
pub use deauthorize::*;
pub use debug::*;
pub use defend::*;
pub use delete_channel::*;
pub use get_active::*;
pub use invite_tracker::track_invites;
pub use kick::changenicks;
pub use kick::*;
pub use message_cache::*;
pub use move_users::*;
pub use mute::*;
pub use random_mute_lol::*;
pub use role::*;
pub use set_vc_size::*;
pub use timeout::*;
pub use unmute::*;
pub use user::*;

use crate::commands::help;

/// Admin commands.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    required_bot_permissions = "ADMINISTRATOR",
    subcommands(
        "audit_logs",
        "authorize",
        "broadcast_voice",
        "create_text_channel",
        "create_voice_channel",
        "deafen",
        "defend",
        "deauthorize",
        "delete_channel",
        "kick",
        "changenicks",
        "mute",
        "message_cache",
        "move_users_to",
        "mute_others",
        "undeafen",
        "unmute",
        "random_mute",
        "get_active_vcs",
        "set_vc_size",
        "timeout",
        //"user",
        //"role",
    ),
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn admin(ctx: Context<'_>) -> Result<(), Error> {
    help::wrapper(ctx).await
}

/// List of all the admin commands.
pub fn commands() -> Vec<crate::Command> {
    vec![
        admin(),
        user(),
        role(),
        kick(),
        mute(),
        mute_others(),
        unmute(),
        timeout(),
        changenicks(),
        set_vc_size(),
    ]
    .into_iter()
    .collect()
}
