use crate::{Context, Error};

// pub mod welcome;
pub mod set_all_log_channel;
pub mod set_auto_role;
pub mod set_idle_timeout;
pub mod set_join_leave_log_channel;
pub mod set_premium;
pub mod set_volume;

pub use set_all_log_channel::*;
pub use set_auto_role::*;
pub use set_idle_timeout::*;
pub use set_join_leave_log_channel::*;
pub use set_premium::*;
pub use set_volume::*;
// pub use welcome::*;
// pub use prefix::*;
// pub use print_settings::*;
// pub use self_deafen::*;
// pub use set_welcome_settings::*;

/// Settings-get commands
#[poise::command(
    prefix_command,
    subcommands(
        "log_channel_for_guild",
        "join_leave_log_channel",
        "all_log_channel",
        "premium",
        "volume",
        "auto_role",
        "idle_timeout"
        // "welcome",
        // "self_deafen",
        // "log_all",
        // "log_guild"
    ),
    ephemeral,
    owners_only
)]

/// Set settings
#[cfg(not(tarpaulin_include))]
pub async fn set(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("");

    ctx.say("You found the settings-set command").await?;

    Ok(())
}
