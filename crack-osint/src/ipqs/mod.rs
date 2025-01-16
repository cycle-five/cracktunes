pub mod ip_score;
pub use ip_score::*;
pub mod scamalytics;
pub use scamalytics::*;

use async_trait::async_trait;
use mockall::automock;
use reqwest::{Client, Error as ReqwestError, Response};
use std::collections::HashMap;

// Add this trait to abstract the HTTP client functionality
#[cfg_attr(test, automock)]
#[async_trait]
pub trait HttpClient: std::fmt::Debug + Send + Sync + 'static {
    /// Get's the response from the given URL.
    /// # Arguments
    /// * `url` - A string slice that holds the URL to get the response from.
    /// # Returns
    /// * A Result containing the Response or a ReqwestError.
    /// # Errors
    /// * A ReqwestError if the request fails.
    /// # Example
    /// ```
    /// use std::sync::Arc;
    /// use reqwest::Client;
    /// use crate::ipqs::HttpClient;
    /// 
    /// async fn example() -> Result<(), reqwest::Error> {
    ///     let client = Arc::new(Client::new());
    ///     let response = client.get("https://api.example.com/data").await?;
    ///     assert!(response.status().is_success());
    ///     Ok(())
    /// }
    /// ```
    async fn get(&self, url: &str) -> Result<Response, ReqwestError>;
    /// Get's the response from the given URL with query parameters.
    /// # Arguments
    /// * `url` - A string slice that holds the URL to get the response from.
    /// * `params` - A HashMap containing query parameters to be added to the URL.
    /// # Returns
    /// * A Result containing the Response or a ReqwestError.
    /// # Errors
    /// * A ReqwestError if the request fails.
    /// # Example
    /// ```
    /// use std::sync::Arc;
    /// use std::collections::HashMap;
    /// use reqwest::Client;
    /// use crate::ipqs::HttpClient;
    /// 
    /// async fn example() -> Result<(), reqwest::Error> {
    ///     let client = Arc::new(Client::new());
    ///     let mut params = HashMap::new();
    ///     params.insert("key".to_string(), "value".to_string());
    ///     let response = client.get_with_headers("https://api.example.com/data", params).await?;
    ///     assert!(response.status().is_success());
    ///     Ok(())
    /// }
    /// ```
    async fn get_with_headers(
        &self,
        url: &str,
        params: HashMap<String, String>,
    ) -> Result<Response, ReqwestError>;
}

// Implement the trait for reqwest::Client
#[async_trait]
impl HttpClient for Client {
    async fn get(&self, url: &str) -> Result<Response, ReqwestError> {
        self.get(url).send().await
    }

    async fn get_with_headers(
        &self,
        url: &str,
        params: HashMap<String, String>,
    ) -> Result<Response, ReqwestError> {
        self.get(url).query(&params).send().await
    }
}
