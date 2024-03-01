use crack_core::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_response_poise, Context,
    Error,
};
use poise::serenity_prelude::GuildId;
use reqwest::Url;
use serde::Deserialize;

#[derive(Deserialize)]
struct ScanResult {
    result_url: String,
}

/// Scan a website for viruses or malicious content.
///
/// Other scanning options include VirusTotal, Google Safe Browsing, Metadefender, etc.
///

#[poise::command(prefix_command, hide_in_help)]
pub async fn scan(ctx: Context<'_>, url: String) -> Result<(), Error> {
    let guild_id_opt = ctx.guild_id();
    let channel_id = ctx.channel_id();

    let message = scan_url(url).await?;

    // Send the response to the user
    send_response_poise(ctx, CrackedMessage::ScanResult { result: message })
        .await
        .map(|m| ctx.data().add_msg_to_cache(ctx.guild_id().unwrap(), m))
}

/// Scan a website for viruses or malicious content.
pub async fn scan_url(url: String) -> Result<String, Error> {
    // Validate the provided URL
    if !url_validator(&url) {
        // Handle invalid URL
        return Err(CrackedError::Other("Invalid URL provided.").into());
    }

    // Perform the scan and retrieve the result
    let scan_result = perform_scan(&url).await?;

    // Format the result into a user-friendly message
    let message = format_scan_result(&scan_result);

    // Send the response to the user
    Ok(message)
}

/// Perform the scan using VirusTotal API.
/// # Arguments
/// * `url` - The URL of the website to be scanned.
/// curl --request POST \
///     --url https://www.virustotal.com/api/v3/urls \
///     --form url=<Your URL here> \
///     --header 'x-apikey: <your API key>'
pub async fn perform_scan(url: &str) -> Result<ScanResult, Error> {
    // URL to submit the scan request to VirusTotal
    let api_url = format!("https://www.virustotal.com/api/v3/urls");
    // Retrieve the API key from the environment variable
    let api_key = std::env::var("VIRUSTOTAL_API_KEY")
        .map_err(|_| CrackedError::Other("VIRUSTOTAL_API_KEY"))?;

    // Set up the API request with headers, including the API key
    let client = reqwest::Client::new();
    let response = client
        .post(&api_url)
        .header("x-apikey", api_key)
        .form(&[("url", url)])
        .send()
        .await?;

    // Process the response
    let scan_result: ScanResult = response.json().await?;

    Ok(scan_result)
}

fn url_validator(url: &str) -> bool {
    // Using the Url cracktunes to parse and validate the URL
    //url::Url::parse(url).is_ok()
    Url::parse(url).is_ok()
}

fn format_scan_result(scan_result: &ScanResult) -> String {
    // Formatting the scan result into a user-friendly message
    format!(
        "Scan submitted successfully! Result URL: {}",
        scan_result.result_url
    )
}
