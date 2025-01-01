use crate::{
    //commands::sub_help as help,
    http_utils,
    http_utils::{CacheHttpExt, SendMessageParams},
    messaging::message::CrackedMessage,
    poise_ext::PoiseContextExt,
    Context,
    Error,
};
use crack_osint::IPQSClient;
use crack_osint::{check_password_pwned, VirusTotalClient};
use crack_osint::{get_scan_result, scan_url};
use poise::CreateReply;
use serenity::small_fixed_array::FixedString;
use std::result::Result;
use std::str::FromStr;

/// Osint Commands
#[poise::command(
    category = "OsInt",
    prefix_command,
    slash_command,
    subcommands(
        // "ip",
        // "ipv",
        // "paywall",
        // "socialmedia",
        // "wayback",
        // "whois",
        // "phlookup",
        // "phcode",
        "checkpass",
        "scan",
        "virustotal_result",
        "ipqs_ip_score",
        //"help",
    ),
)]
pub async fn osint(ctx: Context<'_>) -> Result<(), Error> {
    let guild_name = ctx
        .guild()
        .map(|x| x.name.clone())
        .unwrap_or(FixedString::from_str("DMs").expect("wtf?"));

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

/// Check if a password has been pwned.
#[poise::command(prefix_command, hide_in_help)]
pub async fn checkpass(ctx: Context<'_>, password: String) -> Result<(), Error> {
    let client = http_utils::get_client();
    let pwned = check_password_pwned(client, &password).await?;
    let message = if pwned {
        CrackedMessage::PasswordPwned
    } else {
        CrackedMessage::PasswordSafe
    };

    ctx.send_reply_embed(message).await?;

    Ok(())
}

/// Check if a password has been pwned.
#[poise::command(category = "OsInt", slash_command)]
pub async fn ipqs_ip_score(ctx: Context<'_>, ip: String) -> Result<(), Error> {
    // Get reqwest client from the context
    let data = ctx.data();
    let client = &data.http_client;
    // Get the IPQS API key from the environment
    let api_key =
        std::env::var("IPQS_API_KEY").map_err(|_| crate::CrackedError::Other("IPQS_API_KEY"))?;
    let client = IPQSClient::new_with_client(api_key, client.clone());
    Ok(())
}

/// Get the list of commands available in the osint category.
pub fn commands() -> Vec<crate::Command> {
    vec![scan(), virustotal_result(), checkpass(), ipqs_ip_score()]
}

#[cfg(test)]
mod test {
    use crate::commands::osint;

    #[test]
    fn it_works() {
        osint();
    }
}
