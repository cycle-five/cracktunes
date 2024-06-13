pub mod assign_role;
pub mod create_role;
pub mod delete_role;

pub use assign_role::*;
pub use create_role::*;
pub use delete_role::*;

use crate::{Context, Error};
/// Role commands.
#[poise::command(
    prefix_command,
    //slash_command,
    subcommands(
        "create",
        "delete",
        "delete_by_id",
        "assign",
        "assign_ids",
    ),
    ephemeral,
    owners_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn role(ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Role command called");

    ctx.say("You found the role command").await?;

    Ok(())
}

pub fn role_commands() -> [crate::Command; 5] {
    [assign(), assign_ids(), create(), delete(), delete_by_id()]
}
