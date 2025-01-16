use crate::HttpClient;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "crack-tracing")]
use tracing::{debug, error, instrument};

#[derive(Debug, Serialize, Deserialize, Default)]
#[allow(clippy::struct_excessive_bools)]
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
            IpqsError::RequestError(e) => write!(f, "Request error: {e}"),
            IpqsError::InvalidResponse(e) => write!(f, "Invalid response: {e}"),
        }
    }
}

impl std::error::Error for IpqsError {}

#[derive(Debug)]
pub struct IpqsClient {
    api_key: String,
    client: Box<dyn HttpClient>,
}

impl Default for IpqsClient {
    fn default() -> Self {
        Self {
            api_key: String::default(),
            client: Box::new(Client::new()),
        }
    }
}

impl IpqsClient {
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Box::new(Client::new()),
        }
    }

    #[must_use]
    pub fn new_with_client(api_key: String, client: Client) -> Self {
        Self {
            api_key,
            client: Box::new(client),
        }
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

        let response = self
            .client
            .get_with_headers(&url, params.unwrap_or_default())
            .await
            .map_err(|e| {
                #[cfg(feature = "crack-tracing")]
                error!("Request failed: {}", e);
                IpqsError::RequestError(e.to_string())
            })?;

        // let request = if let Some(params) = params {
        //     #[cfg(feature = "crack-tracing")]
        //     debug!("Adding query parameters: {:?}", params);
        //     request.query(&params)
        // } else {
        //     request
        // };

        // let response = request.send().await.map_err(|e| {
        //     #[cfg(feature = "crack-tracing")]
        //     error!("Request failed: {}", e);
        //     IpqsError::RequestError(e.to_string())
        // })?;

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
    use crate::MockHttpClient;
    use mockall::predicate::*;
    use reqwest::{Response, StatusCode};
    use std::convert::TryFrom;

    #[tokio::test]
    async fn test_successful_ip_check() {
        let mut mock_client = MockHttpClient::new();

        // Create a successful response
        let expected_response = IpqsResponse {
            success: true,
            fraud_score: 25,
            country_code: "US".to_string(),
            city: "San Francisco".to_string(),
            ..Default::default()
        };

        mock_client
            .expect_get_with_headers()
            .with(
                eq("https://ipqualityscore.com/api/json/ip/test_key/1.1.1.1"),
                always(),
            )
            .returning(move |_, _| {
                Ok(Response::from(
                    http::Response::builder()
                        .status(200)
                        .body(serde_json::to_string(&expected_response).unwrap())
                        .unwrap(),
                ))
            });

        let ipqs = IpqsClient {
            api_key: "test_key".to_string(),
            client: Box::new(mock_client),
        };

        let result = ipqs.check_ip("1.1.1.1", None).await.unwrap();

        assert!(result.success);
        assert_eq!(result.fraud_score, 25);
        assert_eq!(result.country_code, "US");
        assert_eq!(result.city, "San Francisco");
    }

    // #[tokio::test]
    // async fn test_request_error() {
    //     let mut mock_client = MockHttpClient::new();

    //     mock_client.expect_get_with_headers().returning(|_, _| {
    //         // let builder = http::Response::builder().status(500);
    //         // let response = builder.body("Connection refused").unwrap();
    //         // Err(reqwest::Error::from(response))
    //         Ok(Response::from(
    //             http::Response::builder()
    //                 .status(500)
    //                 .body("Connection refused".to_string())
    //                 .unwrap(),
    //         ))
    //     });

    //     let ipqs = IpqsClient {
    //         api_key: "test_key".to_string(),
    //         client: Box::new(mock_client),
    //     };

    //     let result = ipqs.check_ip("1.1.1.1", None).await;

    //     assert!(matches!(result, Err(IpqsError::RequestError(_))));
    // }

    #[tokio::test]
    async fn test_invalid_response() {
        let mut mock_client = MockHttpClient::new();

        mock_client.expect_get_with_headers().returning(|_, _| {
            Ok(Response::from(
                http::Response::builder()
                    .status(403)
                    .body("Invalid API key".to_string())
                    .unwrap(),
            ))
        });

        let ipqs = IpqsClient {
            api_key: "invalid_key".to_string(),
            client: Box::new(mock_client),
        };

        let result = ipqs.check_ip("1.1.1.1", None).await;

        assert!(matches!(result, Err(IpqsError::InvalidResponse(_))));
    }

    #[test]
    fn test_default_implementations() {
        let response = IpqsResponse::default();
        assert!(!response.success);
        assert_eq!(response.fraud_score, 0);
        assert_eq!(response.message, "");

        let client = IpqsClient::default();
        assert_eq!(client.api_key, "");
    }
}
