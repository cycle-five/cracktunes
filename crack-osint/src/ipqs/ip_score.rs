use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "crack-tracing")]
use tracing::{debug, error, instrument};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct IpqsResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fraud_score: i32,
    #[serde(default)]
    pub country_code: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub city: String,
    #[serde(rename = "ISP", default)]
    pub isp: String,
    #[serde(default)]
    pub asn: i32,
    #[serde(default)]
    pub operating_system: String,
    #[serde(default)]
    pub browser: String,
    #[serde(default)]
    pub organization: String,
    #[serde(default)]
    pub is_crawler: bool,
    #[serde(default)]
    pub mobile: bool,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub proxy: bool,
    #[serde(default)]
    pub vpn: bool,
    #[serde(default)]
    pub tor: bool,
    #[serde(default)]
    pub active_vpn: bool,
    #[serde(default)]
    pub active_tor: bool,
    #[serde(default)]
    pub device_brand: String,
    #[serde(default)]
    pub device_model: String,
    #[serde(default)]
    pub recent_abuse: bool,
    #[serde(default)]
    pub bot_status: bool,
    #[serde(default)]
    pub connection_type: String,
    #[serde(default)]
    pub abuse_velocity: String,
    #[serde(default)]
    pub zip_code: String,
    #[serde(default)]
    pub latitude: f64,
    #[serde(default)]
    pub longitude: f64,
    #[serde(default)]
    pub request_id: String,
}

#[derive(Debug)]
pub enum IpqsError {
    RequestError(String),
    InvalidResponse(String),
}
impl Default for IpqsError {
    fn default() -> Self {
        IpqsError::RequestError(String::new())
    }
}

impl std::fmt::Display for IpqsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpqsError::RequestError(e) => write!(f, "Request error: {}", e),
            IpqsError::InvalidResponse(e) => write!(f, "Invalid response: {}", e),
        }
    }
}

impl std::error::Error for IpqsError {}

#[derive(Debug, Clone)]
pub struct IpqsClient {
    api_key: String,
    client: Client,
}

impl Default for IpqsClient {
    fn default() -> Self {
        Self {
            api_key: String::default(),
            client: Client::new(),
        }
    }
}

impl IpqsClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    pub fn new_with_client(api_key: String, client: Client) -> Self {
        Self { api_key, client }
    }

    #[cfg_attr(feature = "crack-tracing", instrument(skip(self, params)))]
    pub async fn check_ip(
        &self,
        ip: &str,
        params: Option<HashMap<String, String>>,
    ) -> Result<IpqsResponse, IpqsError> {
        #[cfg(feature = "crack-tracing")]
        debug!("Checking IP: {}", ip);

        let url = format!(
            "https://ipqualityscore.com/api/json/ip/{}/{}",
            self.api_key, ip
        );

        let request = self.client.get(&url);

        let request = if let Some(params) = params {
            #[cfg(feature = "crack-tracing")]
            debug!("Adding query parameters: {:?}", params);
            request.query(&params)
        } else {
            request
        };

        let response = request.send().await.map_err(|e| {
            #[cfg(feature = "crack-tracing")]
            error!("Request failed: {}", e);
            IpqsError::RequestError(e.to_string())
        })?;

        if !response.status().is_success() {
            let error_msg = format!("API request failed with status: {}", response.status());
            #[cfg(feature = "crack-tracing")]
            error!("{}", error_msg);
            return Err(IpqsError::InvalidResponse(error_msg));
        }

        #[cfg(feature = "crack-tracing")]
        debug!("Successfully received response");

        response.json::<IpqsResponse>().await.map_err(|e| {
            #[cfg(feature = "crack-tracing")]
            error!("Failed to parse response: {}", e);
            IpqsError::RequestError(e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ip_check() {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();

        let ipqs = IpqsClient::new_with_client("your_api_key".to_string(), client);

        let mut params = HashMap::new();
        params.insert(
            "user_agent".to_string(),
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_7_2 like Mac OS X)".to_string(),
        );
        params.insert("user_language".to_string(), "en-US".to_string());
        params.insert("strictness".to_string(), "1".to_string());

        let result = ipqs
            .check_ip("fe80::4d90:b5d1:ddc8:ec14", Some(params))
            .await;

        match result {
            Ok(response) => {
                assert!(response.success);
                // Add more assertions as needed
            },
            Err(e) => panic!("Test failed: {}", e),
        }
    }

    #[test]
    fn test_default_implementations() {
        let response = IpqsResponse::default();
        assert_eq!(response.success, false);
        assert_eq!(response.fraud_score, 0);
        assert_eq!(response.message, "");

        let client = IpqsClient::default();
        assert_eq!(client.api_key, "");
    }
}
