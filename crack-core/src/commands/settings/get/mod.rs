use crate::{Context, Error};

pub mod all;
pub mod get_auto_role;
pub mod get_idle_timeout;
pub mod get_premium;
pub mod get_volume;
pub mod get_welcome_settings;
pub mod log_channels;

pub use all::*;
pub use get_auto_role::*;
pub use get_idle_timeout::*;
pub use get_premium::*;
pub use get_volume::*;
pub use get_welcome_settings::*;
pub use log_channels::*;
// pub use prefix::*;
// pub use print_settings::*;
// pub use self_deafen::*;
// pub use set_all_log_channel::*;
// pub use set_idle_timeout::*;

/// Settings-get commands
#[poise::command(
    prefix_command,
    subcommands(
        "all",
        "all_log_channel",
        "auto_role",
        "premium",
        "join_leave_log_channel",
        "welcome_settings",
        "idle_timeout",
        "volume",
        // "self_deafen",
    ),
    ephemeral,
    owners_only
)]

/// Get settings
#[cfg(not(tarpaulin_include))]
pub async fn get(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("");

    ctx.say("You found the settings-get command").await?;

    Ok(())
}
