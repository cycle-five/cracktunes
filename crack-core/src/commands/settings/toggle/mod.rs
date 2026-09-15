use crate::{Context, Error};

pub mod self_deafen;
pub mod toggle_autopause;

pub use self_deafen::*;
pub use toggle_autopause::*;

/// Settings-toggle commands
#[poise::command(
    prefix_command,
    subcommands("selfdeafen", "toggle_autopause"),
    ephemeral,
    owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn toggle(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("You found the settings-toggle command").await?;

    Ok(())
}

/// Get all settings-toggle commands
pub fn commands() -> Vec<crate::Command> {
    vec![selfdeafen(), toggle_autopause()]
}
