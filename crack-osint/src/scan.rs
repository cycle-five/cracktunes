use crate::virustotal::{VirusTotalApiResponse, VirusTotalClient};
use ipinfo::{IpError, IpErrorKind};
use reqwest::Url;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const _VIRUSTOTAL_API_URL: &str = "https://www.virustotal.com/api/v3/urls";

/// Get the scan result for a given id.
pub async fn get_scan_result(
    client: &VirusTotalClient,
    id: String,
) -> Result<VirusTotalApiResponse, Error> {
    client
        .clone()
        .fetch_analysis_report(&id)
        .await
        .map_err(|e| e.into())
}

/// Scan a website for viruses or malicious content.
//pub async fn scan_url<C: Client>(url: String, client: MyClient<C>) -> Result<String, Error> {
pub async fn scan_url(
    client: &VirusTotalClient,
    url: String,
) -> Result<VirusTotalApiResponse, Error> {
    println!("in scan_url");
    // Validate the provided URL
    if !url_validator(&url) {
        // Handle invalid URL
        return Err(Box::new(IpError::new(
            IpErrorKind::ParseError,
            Some("Invalid URL provided: Did you remember the 'http(s)://'?"),
        )));
    }

    // Perform the scan and retrieve the result
    let res = client.clone().fetch_initial_scan_result(&url).await?;

    tracing::info!("Scan result: {}", serde_json::ser::to_string_pretty(&res)?);

    let res2 = client
        .clone()
        .fetch_detailed_scan_result(&res.data.id)
        .await?;

    //tracing::info!(
    println!(
        "Detailed scan result: {}",
        serde_json::to_string_pretty(&res2)?
    );
    // Format the result into a user-friendly message
    let message = res2;

    // Send the response to the user
    Ok(message)
}

/// Validate the provided URL
fn url_validator(url: &str) -> bool {
    // Using the Url cracktunes to parse and validate the URL
    // let test_url = if !url.starts_with("http://") && !url.starts_with("https://") {
    //     format!("https://{}", url)
    // } else {
    //     url.to_string()
    // };
    tracing::info!("url_validator: {}", url);
    Url::parse(url).is_ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_scan_url_error() {
        let reqwest_client = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .build()
            .unwrap();
        let client = VirusTotalClient::new("asdf", reqwest_client);
        let url = "https://www.google.com".to_string();
        let result = scan_url(&client, url).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scan_url_success() {
        // Get API key from environment
        let api_key = std::env::var("VIRUSTOTAL_API_KEY").unwrap_or("NO_KEY".to_string());
        let reqwest_client = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .build()
            .unwrap();

        let client = VirusTotalClient::new(&api_key, reqwest_client);
        let url = "https://www.google.com".to_string();
        let result = scan_url(&client, url).await;
        println!("{:?}", result);
        if api_key != "NO_KEY" {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_url_validator() {
        let url = "https://www.google.com";
        assert!(url_validator(url));
    }

    #[test]
    fn test_url_validator_valid_url() {
        assert!(url_validator("https://www.example.com"));
    }

    #[test]
    fn test_url_validator_invalid_url() {
        assert!(!url_validator("invalid_url"));
    }

    // #[test]
    // fn test_format_scan_result() {
    //     let scan_result = ScanResult {
    //         result_url: "https://urlscan.io/result/123456".to_string(),
    //     };

    //     let formatted_result = format_scan_result(&scan_result);
    //     assert_eq!(
    //         formatted_result,
    //         "Scan submitted successfully! Result URL: https://urlscan.io/result/123456"
    //     );
    // }
}
