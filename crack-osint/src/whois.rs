// #![cfg(feature = "whois")]
use whois_rust::{WhoIs, WhoIsLookupOptions};

use crack_core::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

/// Fetch and display WHOIS information about a domain.
#[poise::command(prefix_command, hide_in_help)]
pub async fn whois(ctx: Context<'_>, domain: String) -> Result<(), Error> {
    let whois = WhoIs::from_string(domain.clone())?;
    let options = WhoIsLookupOptions::from_string(domain)?;
    let result = whois.lookup(options)?;

    // The result is a string containing the WHOIS record
    // You can send the result as is, or parse it to extract specific information
    send_response_poise(ctx, CrackedMessage::DomainInfo(result)).await?;

    Ok(())
}
