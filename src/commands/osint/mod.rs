pub mod checkpass;
pub mod ip;
pub mod ipv;
pub mod socialmedia;
pub mod wayback;
pub mod whois;

pub use checkpass::*;
pub use ip::*;
pub use ipv::*;
pub use socialmedia::*;
pub use wayback::*;
pub use whois::*;

use crate::{Context, Error};

/// Osint Commands
#[poise::command(
    prefix_command,
    subcommands("ip", "ipv", "socialmedia", "wayback", "whois"),
    hide_in_help,
    owners_only
)]
pub async fn osint(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Osint command called");

    Ok(())
}
