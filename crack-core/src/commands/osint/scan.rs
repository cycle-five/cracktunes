use crate::messaging::message::CrackedMessage;
use crack_osint::osint::scan;

/// Scan a website for viruses or malicious content.
///
/// Other scanning options include VirusTotal, Google Safe Browsing, Metadefender, etc.
///
/// # Arguments
/// * `url` - The URL of the website to be scanned.
/// curl --request POST \
///     --url https://www.virustotal.com/api/v3/urls \
///     --form url=<Your URL here> \
///     --header 'x-apikey: <your API key>'
#[poise::command(prefix_command, hide_in_help)]
pub async fn scan(ctx: Context<'_>, url: String) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap_or()?;

    let message = scan(ctx, url).await;

    // Send the response to the user
    send_response_poise(ctx, CrackedMessage::ScanResult { result: message })
        .await
        .map(|m| ctx.data().add_msg_to_cache(ctx.guild_id().unwrap(), m))
}
