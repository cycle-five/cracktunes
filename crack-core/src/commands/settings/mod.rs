use crate::{Context, Error};

pub mod get_settings;
pub mod prefix;
pub mod print_settings;
pub mod set_all_log_channel;
pub mod set_all_log_channel_data;
pub mod set_all_log_channel_old_data;
pub mod set_auto_role;
pub mod set_idle_timeout;
pub mod set_premium;
pub mod set_welcome_settings;

pub use get_settings::*;
pub use prefix::*;
pub use print_settings::*;
pub use set_all_log_channel::*;
pub use set_all_log_channel_data::*;
pub use set_all_log_channel_old_data::*;
pub use set_auto_role::*;
pub use set_idle_timeout::*;
pub use set_premium::*;
pub use set_welcome_settings::*;

/// Settings commands
#[poise::command(
    prefix_command,
    subcommands(
        "add_prefix",
        "clear_prefixes",
        "get_settings",
        "print_settings",
        "set_idle_timeout",
        "set_all_log_channel",
        "set_log_channel_for_guild",
        "set_welcome_settings",
        "set_auto_role",
        "set_premium",
    ),
    ephemeral,
    owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Settings command called");

    ctx.say("You found the settings command").await?;

    Ok(())
}
