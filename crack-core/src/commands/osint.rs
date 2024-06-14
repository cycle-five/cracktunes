pub use crate::{
    http_utils::{CacheHttpExt, SendMessageParams},
    messaging::message::CrackedMessage,
    utils::send_reply,
    Context, Error,
};
use crack_osint::VirusTotalClient;
use crack_osint::{get_scan_result, scan_url};
use poise::CreateReply;
use std::result::Result;

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
        "virustotal_result",
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
    ctx.data()
        .add_msg_to_cache(ctx.guild_id().unwrap(), msg)
        .await;
    tracing::warn!("{}", msg_str.clone());

    Ok(())
}

/// Scan a website for viruses or malicious content.
///
/// Other scanning options include VirusTotal, Google Safe Browsing, Metadefender, etc.
///
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command)]
pub async fn scan(ctx: Context<'_>, url: String) -> Result<(), Error> {
    use crate::http_utils;

    ctx.reply("Scanning...").await?;
    tracing::info!("Scanning URL: {}", url);
    let api_key =
        std::env::var("VIRUSTOTAL_API_KEY").map_err(|_| crate::CrackedError::NoVirusTotalApiKey)?;
    let channel_id = ctx.channel_id();
    tracing::info!("channel_id: {}", channel_id);
    let client = VirusTotalClient::new(&api_key, http_utils::get_client().clone());

    tracing::info!("client: {:?}", client);

    let result = scan_url(&client, url).await?;
    tracing::info!(
        "Scan result: {}",
        serde_json::ser::to_string_pretty(&result)?
    );

    let message = if result.data.attributes.status == "queued" {
        let id = result.data.id;
        CrackedMessage::ScanResultQueued { id }
    } else {
        CrackedMessage::ScanResult { result }
    };

    let params = SendMessageParams {
        channel: channel_id,
        as_embed: true,
        ephemeral: false,
        reply: true,
        msg: message,
        ..Default::default()
    };

    let _msg = ctx.send_channel_message(params).await?;
    Ok(())
}

#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command)]
pub async fn virustotal_result(ctx: Context<'_>, id: String) -> Result<(), Error> {
    ctx.reply("Scanning...").await?;
    let api_key = std::env::var("VIRUSTOTAL_API_KEY")
        .map_err(|_| crate::CrackedError::Other("VIRUSTOTAL_API_KEY"))?;
    let channel_id = ctx.channel_id();
    tracing::info!("channel_id: {}", channel_id);
    let client = VirusTotalClient::new(&api_key, crate::http_utils::get_client().clone());

    tracing::info!("client: {:?}", client);

    let result = get_scan_result(&client, id.clone()).await?;

    let message = if result.data.attributes.status == "queued" {
        CrackedMessage::ScanResultQueued {
            id: result.meta.url_info.id.clone(),
        }
    } else {
        CrackedMessage::ScanResult { result }
    };

    let params = SendMessageParams {
        channel: channel_id,
        as_embed: true,
        ephemeral: false,
        reply: true,
        msg: message,
        ..Default::default()
    };

    let _msg = ctx.send_channel_message(params).await?;
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
