pub mod unban;

pub use unban::*;

pub use crate::utils;
pub use crate::ContextExt;

use crate::commands::sub_help as help;
use crate::{Command, Context, Error};

/// User admin commands.
#[poise::command(
    category = "Admin",
    prefix_command,
    slash_command,
    //subcommands("create", "delete", "delete_by_id", "assign", "assign_ids", "help"),
    subcommands("help"),
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
