// pub mod checkpass;
pub mod ip;
// pub mod ipv;
// pub mod paywall;
// pub mod phcode;
// pub mod phlookup;
// pub mod scan;
// pub mod socialmedia;
// pub mod wayback;
// pub mod whois;

// pub use checkpass::*;
// pub use crack_core::PhoneCodeData;
pub use ip::*;
// pub use ipv::*;
// pub use paywall::*;
// pub use phcode::*;
// pub use phlookup::*;
// pub use scan::*;
// pub use socialmedia::*;
// pub use wayback::*;
// pub use whois::*;

pub use crack_core::{
    messaging::message::CrackedMessage, utils::send_response_poise, Context, Error, Result,
};

/// Osint Commands
#[poise::command(
    prefix_command,
    subcommands(
        "ip",
        // "ipv",
        // "paywall",
        // "socialmedia",
        // "wayback",
        // "whois",
        // "checkpass",
        // "phlookup",
        // "phcode",
        // "scan"
    ),
    owners_only
)]
pub async fn osint(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Osint command called");

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::osint;

    #[test]
    fn it_works() {
        osint();
    }
}
