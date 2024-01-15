use crate::{Context, Error};

pub mod get;
pub mod prefix;
pub mod print_settings;
pub mod self_deafen;
pub mod set;
pub mod set_auto_role;
pub mod set_idle_timeout;
pub mod set_welcome_settings;

pub use get::*;
pub use prefix::*;
pub use print_settings::*;
pub use self_deafen::*;
pub use set::*;
pub use set_auto_role::*;
pub use set_idle_timeout::*;
pub use set_welcome_settings::*;

/// Settings commands
#[poise::command(
    prefix_command,
    subcommands(
        "set",
        "get",
        "add_prefix",
        "clear_prefixes",
        "print_settings",
        "set_idle_timeout",
        "set_welcome_settings",
        "set_auto_role",
        "set_premium",
        "toggle_self_deafen",
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
