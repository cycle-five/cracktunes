use reqwest::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VirusTotalApiResponse {
    pub data: Data,
    pub meta: Meta,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Data {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub links: Links,
    pub attributes: Attributes,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Links {
    #[serde(rename = "self")]
    pub self_: String,
    pub item: String,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EngineResult {
    pub method: String,
    pub engine_name: String,
    pub category: String,
    pub result: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Meta {
    url_info: UrlInfo,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UrlInfo {
    id: String,
    url: String,
}

#[derive(Debug, Clone)]
pub struct VirusTotalClient {
    pub client: reqwest::Client,
    pub api_key: String,
    pub api_url: String,
}

impl VirusTotalClient {
    pub fn new(api_key: &str) -> Self {
        let client = reqwest::Client::new();
        let api_url = "https://www.virustotal.com/api/v3/urls".to_string();
        Self {
            client,
            api_key: api_key.to_string(),
            api_url,
        }
    }

    pub async fn fetch_initial_scan_result(
        self,
        url: &str,
    ) -> Result<VirusTotalApiResponse, Error> {
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
            .json::<VirusTotalApiResponse>()
            .await?;

        Ok(initial_response)
    }

    pub fn format_initial_scan_result(self, initial_response: VirusTotalApiResponse) -> String {
        let result_url = initial_response.data.links.item;
        format!("Scan result: {}", result_url)
    }

    pub async fn fetch_detailed_scan_result(
        self,
        analysis_id: &str,
    ) -> Result<VirusTotalApiResponse, Error> {
        let detailed_api_url =
            format!("https://www.virustotal.com/api/v3/analyses/{}", analysis_id);
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

    pub fn format_detailed_scan_result(detailed_response: VirusTotalApiResponse) -> String {
        let stats = detailed_response.data.attributes.stats;
        format!(
            "Malicious: {}\nSuspicious: {}\nUndetected: {}\nHarmless: {}\nTimeout: {}",
            stats.malicious, stats.suspicious, stats.undetected, stats.harmless, stats.timeout
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Example JSON response (simplified for brevity)
    const TEST_JSON: &str = r#"{
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

    #[tokio::test]
    async fn test_api_response_deserialization() {
        let api_response: VirusTotalApiResponse = serde_json::from_str(TEST_JSON).unwrap();
        assert_eq!(api_response.data.id, "example_id");
        assert_eq!(api_response.data.attributes.stats.undetected, 19);
        assert_eq!(api_response.meta.url_info.url, "https://google.com/");
    }
}
