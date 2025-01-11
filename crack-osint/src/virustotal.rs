//use poise::serenity_prelude::json::hashmap_to_json_map;
use reqwest::{Client, Error};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt::{Display, Formatter, Result as FmtResult};

use crate::VIRUSTOTAL_API_ANALYSES_URL;

/// `VirusTotal` API response.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VirusTotalApiResponse {
    pub data: VTResData,
    pub meta: Meta,
}

/// Implementation of [`VirusTotalApiResponse`].
impl VirusTotalApiResponse {
    /// Takes a `VirusTotalApiResponse` and returns a new one without the results map.
    #[must_use]
    pub fn without_results_map(&self) -> Self {
        let mut new_self = self.clone();
        new_self.data.attributes.results = HashMap::new();
        new_self
    }
}

/// Initial response from the `VirusTotal` API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VirusTotalApiInitialResponse {
    pub data: VTResInitialData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VTResData {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub links: Links,
    pub attributes: Attributes,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VTResInitialData {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub links: LinksInitial,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Links {
    #[serde(rename = "self")]
    pub self_: String,
    pub item: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinksInitial {
    #[serde(rename = "self")]
    pub self_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Attributes {
    pub stats: Stats,
    pub date: u64,
    pub results: HashMap<String, EngineResult>,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Stats {
    pub malicious: u32,
    pub suspicious: u32,
    pub undetected: u32,
    pub harmless: u32,
    pub timeout: u32,
}

impl Display for Stats {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(
            f,
            "Malicious: {}\n Suspicious: {}\n Undetected: {}\n Harmless: {}\n Timeout: {}\n",
            self.malicious, self.suspicious, self.undetected, self.harmless, self.timeout
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EngineResult {
    pub method: String,
    pub engine_name: String,
    pub category: String,
    pub result: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Meta {
    pub url_info: UrlInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UrlInfo {
    pub id: String,
    pub url: String,
}

/// A client for interacting with the `VirusTotal` API.
#[derive(Debug, Clone)]
pub struct VirusTotalClient {
    pub client: reqwest::Client,
    pub api_key: String,
    pub api_url: String,
}

/// Implement the Default trait for `VirusTotalClient`.
impl Default for VirusTotalClient {
    fn default() -> Self {
        let client = reqwest::Client::new();
        let api_key = std::env::var("VIRUSTOTAL_API_KEY").expect("VIRUSTOTAL_API_KEY not set");
        let api_url = "https://www.virustotal.com/api/v3/urls".to_string();
        Self {
            client,
            api_key,
            api_url,
        }
    }
}

impl VirusTotalClient {
    /// Create a new `VirusTotalClient` with the given API key and reqwest Client.
    #[must_use]
    pub fn new(api_key: &str, client: Client) -> Self {
        let api_url = "https://www.virustotal.com/api/v3/urls".to_string();
        Self {
            client,
            api_key: api_key.to_string(),
            api_url,
        }
    }

    /// Fetch the initial scan result for a given URL.
    /// # Returns
    /// A [`Result`] containing the [`VirusTotalApiInitialResponse`] if successful.
    /// # Errors
    /// Returns an [`Error`] if the request fails or the response is not valid JSON.
    pub async fn fetch_initial_scan_result(
        self,
        url: &str,
    ) -> Result<VirusTotalApiInitialResponse, Error> {
        let mut map = std::collections::HashMap::new();
        map.insert("url", url);

        // Set up the API request with headers, including the API key
        let initial_response = self
            .client
            .post(&self.api_url)
            .header("x-apikey", self.api_key)
            .form(&map)
            //.body(Body::from(form))
            .send()
            .await?
            .json::<VirusTotalApiInitialResponse>()
            .await?;

        Ok(initial_response)
    }

    /// Format the initial scan result into a human-readable string.
    #[must_use]
    pub fn format_initial_scan_result(self, initial_response: &VirusTotalApiResponse) -> String {
        // let result_url = initial_response.data.links.self_;
        let result_str = match serde_json::to_string_pretty(initial_response) {
            Ok(s) => s,
            Err(_) => "Error formatting JSON".to_string(),
        };
        format!("Scan result: {result_str}")
    }

    /// Fetch an analysis report for a given analysis ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The API request fails
    /// - The response cannot be parsed as JSON
    pub async fn fetch_analysis_report(
        self,
        analysis_id: &str,
    ) -> Result<VirusTotalApiResponse, Error> {
        let detailed_api_url = format!("{VIRUSTOTAL_API_ANALYSES_URL}/{analysis_id}");
        let detailed_response = self
            .client
            .get(&detailed_api_url)
            .header("x-apikey", self.api_key)
            .send()
            .await?
            .json::<VirusTotalApiResponse>()
            .await?;
        Ok(detailed_response)
    }

    /// Fetch the detailed scan result for a given analysis ID.
    /// # Returns
    /// A [`Result`] containing the detailed scan result if successful.
    /// # Errors
    /// Returns an [`Error`] if the request fails or the response is not valid JSON.
    pub async fn fetch_detailed_scan_result(
        self,
        analysis_id: &str,
    ) -> Result<VirusTotalApiResponse, Error> {
        let detailed_api_url = format!("{VIRUSTOTAL_API_ANALYSES_URL}/{analysis_id}");
        let detailed_response = self
            .client
            .get(&detailed_api_url)
            .header("x-apikey", self.api_key)
            .send()
            .await?
            .json::<VirusTotalApiResponse>()
            .await?;
        Ok(detailed_response)
    }

    #[must_use]
    pub fn format_detailed_scan_result(detailed_response: VirusTotalApiResponse) -> String {
        let stats = detailed_response.data.attributes.stats;
        let map = detailed_response.data.attributes.results;
        let j1 = map
            .iter()
            .map(|(k, v)| format!("{k}: {v:?}",))
            .collect::<Vec<_>>()
            .join(", ");
        // let j1 = serde_json::to_string_pretty(hashmap_to_json_map(map).to_string())?;
        let j2 = json!({
            "Malicious": stats.malicious,
            "Suspicious":stats.suspicious,
            "Undetected":stats.undetected,
            "Harmless": stats.harmless,
            "Timeout": stats.timeout,
        });
        let formatted = serde_json::to_string_pretty(&j2).unwrap_or("EMPTY".to_string());
        format!("{formatted}\n{j1}")
    }
}

#[cfg(test)]
mod test {
    use serde_json::json;

    // Example JSON response (simplified for brevity)

    #[test]
    fn test_api_response_value() {
        let test_json: serde_json::Value = json!({
                "data": {
                    "id": "example_id",
                    "type": "analysis",
                    "links": {
                        "self": "https://example.com/self",
                        "item": "https://example.com/item",
                    },
                    "attributes": {
                        "stats": {
                            "malicious": 0,
                            "suspicious": 0,
                            "undetected": 19,
                            "harmless": 74,
                            "timeout": 0,
                        },
                        "date": 1_709_984_912,
                        "results": {},
                        "status": "completed",
                    },
                },
                "meta": {
                    "url_info": {
                        "id": "example_url_id",
                        "url": "https://google.com/",
                    },
                },
        });
        let api_response: VirusTotalApiResponse =
            serde_json::from_value(test_json.clone()).unwrap();
        assert_eq!(api_response.data.id, "example_id");
        assert_eq!(api_response.data.attributes.stats.undetected, 19);
        assert_eq!(api_response.meta.url_info.url, "https://google.com/");
    }

    const TEST_JSON_DETAILED: &str = r#"{
        "data": {
            "id": "example_id",
            "type": "analysis",
            "links": {
                "self": "https://example.com/self",
                "item": "https://example.com/item"
            },
            "attributes": {
                "stats": {
                    "malicious": 0,
                    "suspicious": 0,
                    "undetected": 19,
                    "harmless": 74,
                    "timeout": 0
                },
                "date": 1709984912,
                "results": {},
                "status": "completed"
            }
        },
        "meta": {
            "url_info": {
                "id": "example_url_id",
                "url": "https://google.com/"
            }
        }
    }"#;

    use crate::VirusTotalApiResponse;

    #[tokio::test]
    async fn test_api_response_deserialization() {
        let api_response: VirusTotalApiResponse = serde_json::from_str(TEST_JSON_DETAILED).unwrap();
        assert_eq!(api_response.data.id, "example_id");
        assert_eq!(api_response.data.attributes.stats.undetected, 19);
        assert_eq!(api_response.meta.url_info.url, "https://google.com/");
    }
}
