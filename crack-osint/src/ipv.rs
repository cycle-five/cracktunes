// use cracktunes::messaging::message::CrackedMessage;
// use cracktunes::utils::send_reply;
use crate::{send_response_poise, Context, CrackedMessage, Error};
use std::net::IpAddr;

/// Determine if an IP address is IPv4 or IPv6.
///
/// If the IP address is valid, this command determines if it's IPv4 or IPv6
/// and sends a response with this information. If the IP address is not
/// valid, this command sends an error message.
///
#[poise::command(prefix_command, hide_in_help)]
pub async fn ipv(ctx: Context<'_>, ip_address: String) -> Result<(), Error> {
    match ip_address.parse::<IpAddr>() {
        Ok(ip_addr) => match ip_addr {
            IpAddr::V4(_) => {
                send_ip_version_response(&ctx, &ip_address, "IPv4").await?;
            },
            IpAddr::V6(_) => {
                send_ip_version_response(&ctx, &ip_address, "IPv6").await?;
            },
        },
        Err(_) => {
            send_error_response(&ctx, &ip_address).await?;
        },
    }
    Ok(())
}

async fn send_error_response(ctx: Context<'_>, ip_address: &str) -> Result<(), Error> {
    send_reply(&ctx, CrackedMessage::InvalidIP(ip_address.to_string())).await?;
    Ok(())
}

async fn send_ip_version_response(
    ctx: Context<'_>,
    ip_address: &str,
    version: &str,
) -> Result<(), Error> {
    send_reply(
        &ctx,
        CrackedMessage::IPVersion(format!("The IP address {} is {}", ip_address, version)),
    )
    .await?;
    Ok(())
}
