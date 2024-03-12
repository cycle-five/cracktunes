use crate::{Context, Error};

pub mod get;
pub mod prefix;
pub mod print_settings;
pub mod set;
pub mod toggle;

pub use get::get;
pub use prefix::*;
pub use print_settings::*;
pub use set::set;
pub use toggle::*;

/// Settings commands
#[poise::command(
    prefix_command,
    subcommands(
        "set",
        "get",
        "toggle",
        "add_prefix",
        "get_prefixes",
        "clear_prefixes",
        "print_settings",
    ),
    ephemeral,
    required_permissions = "ADMINISTRATOR"
)]
#[cfg(not(tarpaulin_include))]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Settings command called");

    ctx.say("You found the settings command").await?;

    Ok(())
}
