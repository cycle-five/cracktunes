use crate::{Context, Error};

pub mod all;

pub use all::*;
// pub use prefix::*;
// pub use print_settings::*;
// pub use self_deafen::*;
// pub use set_all_log_channel::*;
// pub use set_all_log_channel_data::*;
// pub use set_all_log_channel_old_data::*;
// pub use set_auto_role::*;
// pub use set_idle_timeout::*;
// pub use set_premium::*;
// pub use set_welcome_settings::*;

/// Settings-get commands
#[poise::command(
    prefix_command,
    subcommands(
        "all",
        // "premium",
        // "auto_role",
        // "welcome",
        // "self_deafen",
        // "log_all",
        // "log_guild"
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
