use crate::{create_response_poise, Context, CrackedMessage, Error};
use ipinfo::{IpDetails, IpInfo, IpInfoConfig};
use std::net::IpAddr;

/// Fetch and display information about an IP address.
///
/// If the IP address is valid, this command fetches information about the IP
/// address and sends a response with this information. If the IP address is not
/// valid, this command sends an error message.
///
#[poise::command(prefix_command)]
pub async fn ip(ctx: Context<'_>, ip_address: String) -> Result<(), Error> {
    // Validate the IP address
    if ip_address.parse::<IpAddr>().is_err() {
        // The IP address is not valid
        // Send an error message
        send_error_response(ctx, &ip_address).await?;
        return Ok(());
    }

    // The IP address is valid
    // Fetch information about the IP address
    let ip_details = fetch_ip_info(&ip_address).await?;

    // Send a response with the IP information
    send_ip_details_response(ctx, &ip_details).await?;

    Ok(())
}

async fn fetch_ip_info(ip_address: &str) -> Result<IpDetails, Error> {
    let config = IpInfoConfig::default(); // .token("YOUR_TOKEN");
    let mut client = IpInfo::new(config)?;
    let ip_info = client.lookup(ip_address).await?;
    Ok(ip_info)
}

async fn send_error_response(ctx: Context<'_>, ip_address: &str) -> Result<(), Error> {
    create_response_poise(ctx, CrackedMessage::InvalidIP(ip_address.to_string())).await?;
    Ok(())
}

async fn send_ip_details_response(ctx: Context<'_>, ip_details: &IpDetails) -> Result<(), Error> {
    create_response_poise(
        ctx,
        CrackedMessage::IPDetails(format!("IP Details: {:?}", ip_details)),
    )
    .await?;
    Ok(())
}

// /// Fetch and display information about an IP address.
// ///
// /// If the IP address is valid, this command fetches information about the IP
// /// address and sends a response with this information. If the IP address is not
// /// valid, this command sends an error message.
// ///
// /// This command supports both slash commands and prefix commands, and can only be
// /// used in a guild channel. It has an alias "IP".
// #[poise::command(prefix_command, slash_command, aliases("IP"), guild_only)]
// pub async fn ip(ctx: Context<'_>, ip_address: String) -> Result<(), Error> {
//     // Validate the IP address
//     if !is_valid_ip(&ip_address) {
//         create_response_poise(ctx, CrackedMessage::InvalidIP).await?;
//         return Ok(());
//     }

//     let ip_info = fetch_ip_info(&ip_address).await;
//     create_response_poise(ctx, CrackedMessage::IPInformation(ip_info)).await?;

//     Ok(())
// }

// fn is_valid_ip(_ip_address: &str) -> bool {
//     // Add logic to check if the IP address is valid
//     // Here's a trivial example of what this function could return:
//     // This function would return `true` if the `ip_address` is a valid IP, and `false` otherwise
//     true
// }

// async fn fetch_ip_info(ip_address: &str) -> String {
//     // Here, you'd typically make a request to an API to fetch the IP information.
//     // Since I can't make network requests in this environment, I'll return a placeholder string.
//     // Here's an example of what this function could return:
//     // "{ 'ip': '8.8.8.8', 'type': 'IPv4', 'continent_code': 'NA', 'continent_name': 'North America',
//     // 'country_code': 'US', 'country_name': 'United States', 'region_code': 'CA', 'region_name': 'California',
//     // 'city': 'Mountain View', 'zip': '94043', 'latitude': 37.4192, 'longitude': -122.0574,
//     // 'geoname_id': 5375480, 'capital': 'Washington D.C.', 'languages': [{'code': 'en', 'name': 'English',
//     // 'native': 'English'}], 'country_flag': 'http://assets.ipstack.com/flags/us.svg', 'country_flag_emoji': '🇺🇸',
//     // 'country_flag_emoji_unicode': 'U+1F1FA U+1F1F8', 'calling_code': '1', 'is_eu': false }"
//     format!(
//         "Information for IP address {}: IP information goes here",
//         ip_address
//     )
// }
