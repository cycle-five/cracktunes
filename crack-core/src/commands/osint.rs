use crate::utils::send_channel_message;
pub use crate::{
    messaging::message::CrackedMessage,
    utils::{send_response_poise, SendMessageParams},
    Context, Error, Result,
};
use crack_osint::scan_url;
use crack_osint::VirusTotalClient;
use poise::CreateReply;
use std::sync::Arc;

/// Osint Commands
#[poise::command(
    prefix_command,
    slash_command,
    subcommands(
        // "ip",
        // "ipv",
        // "paywall",
        // "socialmedia",
        // "wayback",
        // "whois",
        // "checkpass",
        // "phlookup",
        // "phcode",
        "scan",
    ),
)]
pub async fn osint(ctx: Context<'_>) -> Result<(), Error> {
    let guild_name = ctx
        .guild()
        .map(|x| x.name.clone())
        .unwrap_or("DMs".to_string());

    let msg_str = format!("Osint found in {guild_name}!");
    let msg = ctx
        .send(CreateReply::default().content(msg_str.clone()))
        .await?
        .into_message()
        .await?;
    ctx.data().add_msg_to_cache(ctx.guild_id().unwrap(), msg);
    tracing::warn!("{}", msg_str.clone());

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::commands::osint;

    #[test]
    fn it_works() {
        osint();
    }
}

/// Scan a website for viruses or malicious content.
///
/// Other scanning options include VirusTotal, Google Safe Browsing, Metadefender, etc.
///
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command)]
pub async fn scan(ctx: Context<'_>, url: String) -> Result<(), Error> {
    // let guild_id_opt = ctx.guild_id();
    // let api_url = VIRUSTOTAL_API_URL.to_string();
    // Retrieve the API key from the environment variable
    let api_key = std::env::var("VIRUSTOTAL_API_KEY")
        .map_err(|_| crate::CrackedError::Other("VIRUSTOTAL_API_KEY"))?;
    let channel_id = ctx.channel_id();
    let client = VirusTotalClient::new(&api_key);

    let message = scan_url(&client, url).await?;
    let message = CrackedMessage::ScanResult { result: message };

    let params = SendMessageParams {
        channel: channel_id,
        as_embed: true,
        ephemeral: false,
        reply: true,
        msg: message,
    };

    let _msg = send_channel_message(Arc::new(ctx.http()), params).await?;
    Ok(())
}
