use ::http::response::Builder;
///
use mockall::automock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};

#[cfg(feature = "crack-tracing")]
use tracing::{debug, error, instrument};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ScamalyticsCredits {
    #[serde(default)]
    pub used: String,
    #[serde(default)]
    pub remaining: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    #[serde(rename = "very high")]
    VeryHigh,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyType {
    #[default]
    #[serde(rename = "0")]
    None,
    VPN,
    TOR,
    DCH,
    PUB,
    WEB,
    SES,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    #[default]
    #[serde(rename = "")]
    Unknown,
    Dialup,
    Isdn,
    Cable,
    Dsl,
    Fttx,
    Wireless,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ScamalyticsResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub score: String,
    #[serde(default)]
    pub risk: RiskLevel,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub credits: ScamalyticsCredits,
    #[serde(default)]
    pub exec: String,
    #[serde(default)]
    pub ip_country_code: String,
    #[serde(default)]
    pub ip_state_name: String,
    #[serde(default)]
    pub ip_city: String,
    #[serde(default)]
    pub ip_postcode: String,
    #[serde(default)]
    pub ip_geolocation: String,
    #[serde(default)]
    pub ip_country_name: String,
    #[serde(rename = "ISP Name", default)]
    pub isp_name: String,
    #[serde(rename = "ISP Fraud Score", default)]
    pub isp_fraud_score: String,
    #[serde(default)]
    pub proxy_type: ProxyType,
    #[serde(default)]
    pub connection_type: ConnectionType,
    #[serde(rename = "Organization Name", default)]
    pub organization_name: String,
}

#[derive(Debug)]
pub enum ScamalyticsError {
    RequestError(String),
    ApiError(String),
    InvalidResponse(String),
}

impl std::fmt::Display for ScamalyticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScamalyticsError::RequestError(e) => write!(f, "Request error: {}", e),
            ScamalyticsError::ApiError(e) => write!(f, "API error: {}", e),
            ScamalyticsError::InvalidResponse(e) => write!(f, "Invalid response: {}", e),
        }
    }
}

impl std::error::Error for ScamalyticsError {}

impl Default for ScamalyticsError {
    fn default() -> Self {
        ScamalyticsError::RequestError(String::new())
    }
}

use reqwest::{Error as ReqwestError, Response};

// Add this trait to abstract the HTTP client functionality
#[cfg_attr(test, automock)]
#[async_trait::async_trait]
pub trait HttpClient: std::fmt::Debug + Send + Sync + 'static {
    async fn get(&self, url: &str) -> Result<Response, ReqwestError>;
}

// Implement the trait for reqwest::Client
#[async_trait::async_trait]
impl HttpClient for Client {
    async fn get(&self, url: &str) -> Result<Response, ReqwestError> {
        self.get(url).send().await
    }
}

// Updated ScamalyticsClient to use the trait
#[derive(Debug)]
pub struct ScamalyticsClient {
    hostname: String,
    username: String,
    api_key: String,
    client: Box<dyn HttpClient>,
}

impl Default for ScamalyticsClient {
    fn default() -> Self {
        Self {
            hostname: String::new(),
            username: String::new(),
            api_key: String::new(),
            client: Box::new(Client::new()),
        }
    }
}

impl ScamalyticsClient {
    pub fn new(hostname: String, username: String, api_key: String) -> Self {
        Self {
            hostname,
            username,
            api_key,
            client: Box::new(Client::new()),
        }
    }

    pub fn new_with_client(
        hostname: String,
        username: String,
        api_key: String,
        client: Box<dyn HttpClient>,
    ) -> Self {
        Self {
            hostname,
            username,
            api_key,
            client,
        }
    }

    #[cfg_attr(feature = "crack-tracing", instrument(skip(self)))]
    pub async fn check_ip(
        &self,
        ip: &str,
        test_mode: bool,
    ) -> Result<ScamalyticsResponse, ScamalyticsError> {
        #[cfg(feature = "crack-tracing")]
        debug!("Checking IP: {}", ip);

        let url = format!(
            "https://{}/{}/?ip={}&key={}&test={}",
            self.hostname,
            self.username,
            ip,
            self.api_key,
            if test_mode { "1" } else { "0" }
        );

        #[cfg(feature = "crack-tracing")]
        debug!("Making request to Scamalytics API");

        let response = self.client.get(&url).await.map_err(|e| {
            #[cfg(feature = "crack-tracing")]
            error!("Request failed: {}", e);
            ScamalyticsError::RequestError(e.to_string())
        })?;

        if !response.status().is_success() {
            let error_msg = format!("API request failed with status: {}", response.status());
            #[cfg(feature = "crack-tracing")]
            error!("{}", error_msg);
            return Err(ScamalyticsError::InvalidResponse(error_msg));
        }

        let result = response.json::<ScamalyticsResponse>().await.map_err(|e| {
            #[cfg(feature = "crack-tracing")]
            error!("Failed to parse response: {}", e);
            ScamalyticsError::InvalidResponse(e.to_string())
        })?;

        if result.status != "ok" {
            let error_msg = result
                .error
                .unwrap_or_else(|| "Unknown API error".to_string());
            #[cfg(feature = "crack-tracing")]
            error!("API returned error: {}", error_msg);
            return Err(ScamalyticsError::ApiError(error_msg));
        }

        #[cfg(feature = "crack-tracing")]
        debug!("Successfully received and parsed response");

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    // use url::Url;
    use super::*;
    use ::http::response::Builder;
    use mockall::mock;
    use mockall::predicate::*;
    use poise::serenity_prelude::http;
    use reqwest::Response;
    use reqwest::ResponseBuilderExt;
    use reqwest::StatusCode;
    use reqwest::Url;
    use serde_json::json;
    use std::os::unix::fs::DirBuilderExt;

    #[test]
    fn test_from_http_response() {
        let url = Url::parse("http://example.com").unwrap();
        let response = Builder::new()
            .status(200)
            .url(url.clone())
            .body("foo")
            .unwrap();
        let response = Response::from(response);

        assert_eq!(response.status(), 200);
        assert_eq!(*response.url(), url);
    }

    #[tokio::test]
    async fn test_successful_ip_check() {
        let mut mock_client = MockHttpClient::new();

        // Create mock response data
        let response_data = json!({
            "status": "ok",
            "mode": "test",
            "ip": "167.99.90.43",
            "score": "99",
            "risk": "very high",
            "url": "https://scamalytics.com/ip/167.99.90.43",
            "credits": {
                "used": "123",
                "remaining": "12373845362"
            },
            "exec": "1.47ms",
            "ip_country_code": "US",
            "ip_state_name": "Illinois",
            "ip_city": "Chicago",
            "ip_postcode": "60666",
            "ip_geolocation": "41.8781,-87.6298",
            "ip_country_name": "United States",
            "ISP Name": "DigitalOcean, LLC",
            "ISP Fraud Score": "52",
            "proxy_type": "DCH",
            "connection_type": "cable",
            "Organization Name": "DigitalOcean"
        });

        // Set up the mock expectation
        mock_client
            .expect_get()
            .with(str::contains("167.99.90.43"))
            .returning(move |_| {
                Ok(Response::from(
                    Builder::new()
                        .status(StatusCode::OK)
                        .body(response_data.to_string())
                        .unwrap(),
                ))
            });

        let client = ScamalyticsClient::new_with_client(
            "api.example.com".to_string(),
            "testuser".to_string(),
            "testkey".to_string(),
            Box::new(mock_client),
        );

        let result = client.check_ip("167.99.90.43", true).await.unwrap();

        assert_eq!(result.status, "ok");
        assert_eq!(result.ip, "167.99.90.43");
        assert_eq!(result.score, "99");
        assert!(matches!(result.risk, RiskLevel::VeryHigh));
        assert_eq!(result.isp_name, "DigitalOcean, LLC");
        assert_eq!(result.isp_fraud_score, "52");
    }

    #[tokio::test]
    async fn test_api_error_response() {
        let mut mock_client = MockHttpClient::new();

        // Create mock error response
        let error_response = json!({
            "status": "error",
            "error": "Invalid API key",
            "mode": "test",
            "ip": "167.99.90.43"
        });

        mock_client.expect_get().returning(move |_| {
            Ok(Response::from(
                Builder::new()
                    .status(StatusCode::OK)
                    .body(error_response.to_string())
                    .unwrap(),
            ))
        });

        let client = ScamalyticsClient::new_with_client(
            "api.example.com".to_string(),
            "testuser".to_string(),
            "invalid_key".to_string(),
            Box::new(mock_client),
        );

        let result = client.check_ip("167.99.90.43", true).await;

        assert!(matches!(result, Err(ScamalyticsError::ApiError(_))));
        if let Err(ScamalyticsError::ApiError(msg)) = result {
            assert_eq!(msg, "Invalid API key");
        }
    }

    #[tokio::test]
    async fn test_http_error_response() {
        let mut mock_client = MockHttpClient::new();

        mock_client.expect_get().returning(|_| {
            Ok(Response::from(
                Builder::new()
                    .status(StatusCode::UNAUTHORIZED)
                    .body("Unauthorized".to_string())
                    .unwrap(),
            ))
        });

        let client = ScamalyticsClient::new_with_client(
            "api.example.com".to_string(),
            "testuser".to_string(),
            "testkey".to_string(),
            Box::new(mock_client),
        );

        let result = client.check_ip("167.99.90.43", true).await;

        assert!(matches!(result, Err(ScamalyticsError::InvalidResponse(_))));
    }

    #[test]
    fn test_default_implementations() {
        let response = ScamalyticsResponse::default();
        assert_eq!(response.status, "");
        assert_eq!(response.score, "");

        let client = ScamalyticsClient::default();
        assert_eq!(client.hostname, "");
        assert_eq!(client.username, "");
        assert_eq!(client.api_key, "");
    }
}
