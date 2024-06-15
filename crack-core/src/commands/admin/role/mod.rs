pub mod assign_role;
pub mod create_role;
pub mod delete_role;

pub use assign_role::*;
pub use create_role::*;
pub use delete_role::*;

pub use crate::utils;
pub use crate::ContextExt;

use crate::commands::sub_help as help;
use crate::{Context, Error};

/// Role commands.
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("create", "delete", "delete_by_id", "assign", "assign_ids", "help"),
    ephemeral,
    hide_in_help = true
)]
#[cfg(not(tarpaulin_include))]
pub async fn role(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Role command called");

    ctx.send_found_command("role".to_string()).await?;

    Ok(())
}

pub fn role_commands() -> [crate::Command; 5] {
    [assign(), assign_ids(), create(), delete(), delete_by_id()]
}
