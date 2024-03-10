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

    let res2 = client
        .clone()
        .fetch_detailed_scan_result(&res.data.links.item)
        .await?;
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
    use tokio;

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
        // let my_client = MyClient { client };
        let client = VirusTotalClient::new("asdf");
        let url = "https://www.google.com".to_string();
        let result = scan_url(&client, url).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_url_validator() {
        let url = "https://www.google.com";
        assert!(url_validator(url));
    }
}
