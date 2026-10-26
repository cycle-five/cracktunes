use crate::{Context, Error};

pub mod toggle_autopause;

pub use toggle_autopause::*;

/// Settings-toggle commands
#[poise::command(
    prefix_command,
    subcommands("toggle_autopause"),
    ephemeral,
    owners_only
)]
pub async fn toggle(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("You found the settings-toggle command").await?;

    Ok(())
}

/// Get all settings-toggle commands
pub fn commands() -> Vec<crate::Command> {
    vec![toggle_autopause()]
}
