use crack_types::{CrackedError, CrackedResult};
#[cfg(test)]
use mockall::automock;
use reqwest::{Client, Error as ReqwestError, Response};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "crack-tracing")]
use tracing::{debug, error, instrument, warn};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ScamalyticsCredits {
    #[serde(default)]
    pub used: i32,
    #[serde(default)]
    pub remaining: i32,
    #[serde(default)]
    pub seconds_elapsed_since_last_sync: i32,
    #[serde(default)]
    last_sync_timestamp_utc: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    #[serde(rename = "very high")]
    VeryHigh,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
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

#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
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
    pub score: i32,
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
            ScamalyticsError::RequestError(e) => write!(f, "Request error: {e}"),
            ScamalyticsError::ApiError(e) => write!(f, "API error: {e}"),
            ScamalyticsError::InvalidResponse(e) => write!(f, "Invalid response: {e}"),
        }
    }
}

impl std::error::Error for ScamalyticsError {}

impl Default for ScamalyticsError {
    fn default() -> Self {
        ScamalyticsError::RequestError(String::new())
    }
}

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
    #[must_use]
    pub fn new(hostname: String, username: String, api_key: String) -> Self {
        Self {
            hostname,
            username,
            api_key,
            client: Box::new(Client::new()),
        }
    }

    #[must_use]
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
        debug!("Making request to Scamalytics API {url}");
        println!("Making request to Scamalytics API {url}");

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

        #[cfg(feature = "crack-tracing")]
        debug!("response: {response:?}");

        // println!("response: {response:?}");
        // let raw = response.text().await.unwrap();
        // println!("response: {raw:?}");
        // let result: ScamalyticsResponse = serde_json::from_str(&raw).map_err(|e| {
        //     #[cfg(feature = "crack-tracing")]
        //     error!("Failed to parse response: {}", e);
        //     ScamalyticsError::InvalidResponse(e.to_string())
        // })?;

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

/// Gets an environment variable or returns an error if it's not set.
///   Arguments:
/// - [`&str`]: The name of the environment variable to retrieve
///   Errors:
/// - [`MissingEnvVar`]: The environment variable is not set
#[allow(dead_code)]
fn check_get_env_var(key: &str) -> CrackedResult<String> {
    if let Ok(val) = std::env::var(key) {
        Ok(val)
    } else {
        #[cfg(feature = "crack-tracing")]
        warn!("Environment variable {key} not set");
        Err(CrackedError::MissingEnvVar(key.to_string()))
    }
}

#[cfg(test)]
mod tests {
    // use url::Url;
    use super::*;
    use ::http::response::Builder;

    use mockall::predicate::*;

    use reqwest::Response;
    use reqwest::ResponseBuilderExt;
    use reqwest::StatusCode;
    use reqwest::Url;
    use serde_json::json;

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

        // Create mock response data that exactly matches our struct's format
        let response_data = ScamalyticsResponse {
            status: "ok".to_string(),
            error: None,
            mode: "live".to_string(),
            ip: "8.8.8.8".to_string(),
            score: 0,
            risk: RiskLevel::Low,
            url: "https://scamalytics.com/ip/8.8.8.8".to_string(),
            credits: ScamalyticsCredits {
                used: 4,
                remaining: 4996,
                seconds_elapsed_since_last_sync: 20,
                last_sync_timestamp_utc: "2025-01-08 20:38:01".to_string(),
                note: "Credits used and remaining are approximate values.".to_string(),
            },
            exec: "3.37 ms".to_string(),
            ip_country_code: "US".to_string(),
            ip_state_name: "California".to_string(),
            ip_city: "Mountain View".to_string(),
            ip_postcode: "94043".to_string(),
            ip_geolocation: "37.4223,-122.085".to_string(),
            ip_country_name: "United States".to_string(),
            isp_name: "Google LLC".to_string(),
            isp_fraud_score: "2".to_string(),
            proxy_type: ProxyType::DCH,
            connection_type: ConnectionType::Unknown,
            organization_name: "Level 3".to_string(),
        };

        // Convert to JSON string
        let response_json = serde_json::to_string(&response_data).unwrap();

        // Set up the mock expectation with reqwest Response directly
        mock_client
            .expect_get()
            .with(str::contains("8.8.8.8"))
            .returning(move |_| {
                Ok(Response::from(
                    Builder::new()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(response_json.clone())
                        .unwrap(),
                ))
            });

        let client = ScamalyticsClient::new_with_client(
            "api.example.com".to_string(),
            "testuser".to_string(),
            "testkey".to_string(),
            Box::new(mock_client),
        );

        let result = client.check_ip("8.8.8.8", true).await.unwrap();

        // Verify all fields match exactly
        assert_eq!(result.status, response_data.status);
        assert_eq!(result.ip, response_data.ip);
        assert_eq!(result.score, response_data.score);
        assert_eq!(result.risk, response_data.risk);
        assert_eq!(result.isp_name, response_data.isp_name);
        assert_eq!(result.isp_fraud_score, response_data.isp_fraud_score);
        assert_eq!(result.proxy_type, response_data.proxy_type);
        assert_eq!(result.connection_type, response_data.connection_type);
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
        assert_eq!(response.score, 0);
        assert_eq!(response.risk, RiskLevel::Low);
        assert_eq!(response.credits.used, 0);
        assert_eq!(response.proxy_type, ProxyType::None);

        let client = ScamalyticsClient::default();
        assert_eq!(client.hostname, "");
        assert_eq!(client.username, "");
        assert_eq!(client.api_key, "");
    }

    #[tokio::test]
    async fn test_live_ip_check() {
        // Skip if we're in CI environment
        if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
            return;
        }

        // Get API credentials from environment

        let Ok(api_key) = check_get_env_var("SCAMALYTICS_API_KEY") else {
            println!("Skipping test: SCAMALYTICS_API_KEY not set");
            return;
        };

        let Ok(api_host) = check_get_env_var("SCAMALYTICS_API_HOST") else {
            println!("Skipping test: SCAMALYTICS_API_HOST not set");
            return;
        };

        let Ok(api_user) = check_get_env_var("SCAMALYTICS_API_USER") else {
            println!("Skipping test: SCAMALYTICS_API_USER not set");
            return;
        };

        let client = ScamalyticsClient::new(api_host, api_user, api_key);

        // Test with Google's public DNS IP
        let result = match client.check_ip("8.8.8.8", false).await {
            Ok(response) => response,
            Err(e) => {
                panic!("Live API call failed: {e}");
            },
        };

        // Verify response structure
        assert_eq!(result.status, "ok", "API response status should be 'ok'");
        assert_eq!(result.ip, "8.8.8.8", "IP address should match input");
        assert!(result.score >= 0, "Score should be non-negative");
        assert!(
            matches!(
                result.risk,
                RiskLevel::Low | RiskLevel::Medium | RiskLevel::High | RiskLevel::VeryHigh
            ),
            "Risk level should be valid"
        );
        assert!(!result.isp_name.is_empty(), "ISP name should not be empty");
        assert!(
            !result.ip_country_code.is_empty(),
            "Country code should not be empty"
        );
        assert!(
            result.credits.used >= 0,
            "Used credits should be non-negative"
        );
        assert!(
            result.credits.remaining >= 0,
            "Remaining credits should be non-negative"
        );
    }
}
