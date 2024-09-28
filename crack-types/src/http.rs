use crate::Error;
use reqwest::Client;
use url::Url;

/// Parse a URL string into a URL object.
pub fn parse_url(url: &str) -> Result<Url, Error> {
    Url::parse(url).map_err(Into::into)
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(client: Client, url: &str) -> Result<String, Error> {
    resolve_final_url2(client, parse_url(url)?)
        .await
        .map(|x| x.to_string())
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url2(client: Client, url: Url) -> Result<Url, Error> {
    // Make a GET request, which will follow redirects by default
    let response = client.get(url.to_string()).send().await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url)
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_resolve_final_url() {
        let client = reqwest::Client::new();
        let url = "https://httpbin.org/redirect-to?url=https://example.com";
        let final_url = resolve_final_url(client, url).await.unwrap();
        assert_eq!(final_url.as_str(), "https://example.com/");
    }

    #[tokio::test]
    async fn test_resolve_final_url2() {
        let client = reqwest::Client::new();
        let url = Url::parse("https://httpbin.org/redirect-to?url=https://example.com").unwrap();
        let final_url = resolve_final_url2(client, url).await.unwrap();
        assert_eq!(final_url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_parse_url() {
        let url = "https://example.com/";
        let parsed_url = parse_url(url).unwrap();
        assert_eq!(parsed_url.as_str(), url);
    }

    #[test]
    fn test_parse_url_invalid() {
        let url = "https://example.com:foo";
        let parsed_url = parse_url(url);
        assert!(parsed_url.is_err());
    }
}
