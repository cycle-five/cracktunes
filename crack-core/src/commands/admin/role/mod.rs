pub mod assign_role;
pub mod create_role;
pub mod delete_role;

pub use assign_role::*;
pub use create_role::*;
pub use delete_role::*;

pub use crate::poise_ext::ContextExt;
pub use crate::utils;

use crate::commands::help;
use crate::{Context, Error};

/// Role commands.
#[poise::command(
    category = "Admin",
    required_permissions = "ADMINISTRATOR",
    prefix_command,
    slash_command,
    subcommands("create", "delete", "delete_by_id", "assign", "assign_ids"),
    ephemeral
)]
#[cfg(not(tarpaulin_include))]
pub async fn role(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }

    ctx.send_found_command("admin role".to_string()).await?;

    Ok(())
}

pub fn role_commands() -> [crate::Command; 5] {
    [assign(), assign_ids(), create(), delete(), delete_by_id()]
}
