use crate::{Context, Error};

pub mod self_deafen;

pub use self_deafen::*;

/// Settings-toggle commands
#[poise::command(prefix_command, subcommands("self_deafen",), ephemeral, owners_only)]
#[cfg(not(tarpaulin_include))]
pub async fn toggle(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("");

    ctx.say("You found the settings-toggle command").await?;

    Ok(())
}
