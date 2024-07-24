pub mod unban;

pub use unban::*;

pub use crate::poise_ext::ContextExt;
pub use crate::utils;

use crate::{Command, Context, Error};

/// User admin commands.
#[poise::command(
    category = "Admin",
    prefix_command,
    slash_command,
    //subcommands("unban"),
    ephemeral,
    hide_in_help = true
)]
#[cfg(not(tarpaulin_include))]
pub async fn user(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Role command called");

    ctx.send_found_command("admin user".to_string()).await?;

    Ok(())
}

pub fn user_commands() -> [Command; 2] {
    [unban(), unban_by_user_id()]
}
