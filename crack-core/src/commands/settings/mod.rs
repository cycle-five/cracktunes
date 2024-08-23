use crate::poise_ext::MessageInterfaceCtxExt;
use crate::{Context, CrackedMessage, Error};

pub mod get;
pub mod prefix;
pub mod print_settings;
pub mod set;
pub mod toggle;

use crate::commands::help;
pub use get::get;
pub use prefix::*;
pub use print_settings::*;
pub use set::set;
pub use toggle::*;

/// Settings commands
#[poise::command(
    prefix_command,
    slash_command,
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
pub async fn settings(
    ctx: Context<'_>,
    #[flag]
    #[description = "Shows the help menu for this command"]
    help: bool,
) -> Result<(), Error> {
    if help {
        help::wrapper(ctx).await?;
    }

    ctx.send_reply(CrackedMessage::CommandFound(String::from("settings")), true)
        .await?;

    Ok(())
}

pub fn commands() -> Vec<crate::Command> {
    vec![settings()].into_iter().collect()
    // .chain(set::commands())
    // .chain(    get::commands())
}

pub fn sub_commands() -> Vec<crate::Command> {
    vec![]
        .into_iter()
        .chain(set::commands())
        .chain(get::commands())
        .collect()
}
