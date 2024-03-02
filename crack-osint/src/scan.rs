use crack_core::{
    errors::CrackedError,
    messaging::message::CrackedMessage,
    utils::{send_channel_message, SendMessageParams},
    Context, Error,
};
use poise::serenity_prelude::GuildId;
use reqwest::Url;
// use reqwest_mock::Client;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;

const VIRUSTOTAL_API_URL: &str = "https://www.virustotal.com/api/v3/urls";

// pub struct MyClient<C: Client> {
//     client: C,
// }

// //pub fn new_client() -> MyClient<Client> {
// pub fn new_client() -> MyClient<DirectClient> {
//     MyClient {
//         client: reqwest::Client::new(),
//     }
// }

// #[cfg(test)]
// pub fn test_client(path: &str) -> MyClient<ReplayClient> {
//     MyClient {
//         client: ReplayClient::new(path),
//     }
// }

#[derive(Deserialize)]
pub struct ScanResult {
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
    let client = reqwest::Client::new();

    let message = scan_url(url, client).await?;
    let message = CrackedMessage::ScanResult { result: message };

    let params = SendMessageParams {
        channel: channel_id,
        as_embed: true,
        ephemeral: false,
        reply: true,
        msg: message,
    };

    send_channel_message(Arc::new(ctx.http()), params)
        .await
        .map(|m| {
            ctx.data()
                .add_msg_to_cache(guild_id_opt.unwrap_or(GuildId::new(1)), m);
        })
}

/// Scan a website for viruses or malicious content.
//pub async fn scan_url<C: Client>(url: String, client: MyClient<C>) -> Result<String, Error> {
pub async fn scan_url(url: String, client: Client) -> Result<String, Error> {
    // Validate the provided URL
    if !url_validator(&url) {
        // Handle invalid URL
        return Err(CrackedError::Other("Invalid URL provided.").into());
    }

    // Perform the scan and retrieve the result
    let scan_result = perform_scan(&url, client).await?;

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
//pub async fn perform_scan<C: Client>(url: &str, client: MyClient<C>) -> Result<ScanResult, Error> {
pub async fn perform_scan(url: &str, client: Client) -> Result<ScanResult, Error> {
    // URL to submit the scan request to VirusTotal
    let api_url = VIRUSTOTAL_API_URL.to_string();
    // Retrieve the API key from the environment variable
    let api_key = std::env::var("VIRUSTOTAL_API_KEY")
        .map_err(|_| CrackedError::Other("VIRUSTOTAL_API_KEY"))?;

    // let form = reqwest::multipart::Form::new().text("url", url);
    let mut map = std::collections::HashMap::new();
    map.insert("url", url);

    // Set up the API request with headers, including the API key
    let response = client
        .post(&api_url)
        .header("x-apikey", api_key)
        .form(&map)
        //.body(Body::from(form))
        .send()
        .await?;

    // // Process the response
    // let asdf = response.body().await?;

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

#[cfg(test)]
mod test {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_scan_url() {
        // let client = reqwest_mock::ReplayClient::new(VIRUSTOTAL_API_URL, RecordMode::ReplayOnly);
        // let my_client = MyClient { client };
        let client = reqwest::Client::new();
        let url = "https://www.google.com".to_string();
        let result = scan_url(url, client).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_perform_scan() {
        // let my_client = test_client(VIRUSTOTAL_API_URL);
        let my_client = reqwest::Client::new();
        let url = "https://www.google.com";
        let result = perform_scan(url, my_client).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_url_validator() {
        let url = "https://www.google.com";
        assert!(url_validator(url));
    }

    #[test]
    fn test_format_scan_result() {
        let scan_result = ScanResult {
            result_url: "https://www.virustotal.com/url/scan/result".to_string(),
        };
        let result = format_scan_result(&scan_result);
        assert_eq!(
            result,
            "Scan submitted successfully! Result URL: https://www.virustotal.com/url/scan/result"
        );
    }
}
