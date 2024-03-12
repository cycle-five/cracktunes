use crate::virustotal::{VirusTotalApiResponse, VirusTotalClient};
use ipinfo::{IpError, IpErrorKind};
use reqwest::Url;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

const _VIRUSTOTAL_API_URL: &str = "https://www.virustotal.com/api/v3/urls";

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
            Some("Invalid URL provided"),
        )));
    }

    // Perform the scan and retrieve the result
    let res = client.clone().fetch_initial_scan_result(&url).await?;

    //tracing::info!("Scan resrlt: {}", serde_json::ser::to_string_pretty(&res)?);
    println!("Scan resrlt: {}", serde_json::ser::to_string_pretty(&res)?);

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

/// Perform the scan using VirusTotal API.
/// # Arguments
/// * `url` - The URL of the website to be scanned.
/// curl --request POST \
///     --url https://www.virustotal.com/api/v3/urls \
///     --form url=<Your URL here> \
///     --header 'x-apikey: <your API key>'
//pub async fn perform_scan<C: Client>(url: &str, client: MyClient<C>) -> Result<ScanResult, Error> {
// pub async fn perform_scan(url: &str, client: Client) -> Result<ScanResult, Error> {
//     // URL to submit the scan request to VirusTotal
//     let api_url = VIRUSTOTAL_API_URL.to_string();
//     // Retrieve the API key from the environment variable
//     let api_key = std::env::var("VIRUSTOTAL_API_KEY")
//         .map_err(|_| CrackedError::Other("VIRUSTOTAL_API_KEY"))?;

//     // let form = reqwest::multipart::Form::new().text("url", url);
//     let mut map = std::collections::HashMap::new();
//     map.insert("url", url);

//     // Set up the API request with headers, including the API key
//     let response = client
//         .post(&api_url)
//         .header("x-apikey", api_key)
//         .form(&map)
//         //.body(Body::from(form))
//         .send()
//         .await?;

//     // // Process the response
//     // let asdf = response.body().await?;

//     let scan_result: VirusTotalApiResponse = response.json::<VirusTotalApiResponse>().await?;

//     Ok(scan_result)
// }

fn url_validator(url: &str) -> bool {
    // Using the Url cracktunes to parse and validate the URL
    //url::Url::parse(url).is_ok()
    Url::parse(url).is_ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_scan_url_error() {
        // let my_client = MyClient { client };
        let client = VirusTotalClient::new("asdf");
        let url = "https://www.google.com".to_string();
        let result = scan_url(&client, url).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_scan_url_success() {
        // Get API key from environment
        let api_key = std::env::var("VIRUSTOTAL_API_KEY").unwrap();
        let client = VirusTotalClient::new(&api_key);
        let url = "https://www.google.com".to_string();
        let result = scan_url(&client, url).await;
        println!("{:?}", result);
        assert_eq!(result.unwrap().data.attributes.stats.harmless, 74);
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
