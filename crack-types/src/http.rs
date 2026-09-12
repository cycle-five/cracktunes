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

    // 🪤 IGNORED, not deleted, and not gated on `env::var("CI")`. These two
    // reach httpbin.org -- a live third party -- so their pass/fail is owned by
    // someone else's uptime. They failed the v0.9.2 `coverage` run and again on
    // the v0.9.4 `Build` run, both times while passing locally, and both times
    // costing a real investigation before the cause turned out to be "httpbin
    // was unreachable from GitHub's runners".
    //
    // `#[ignore]` and not a CI guard: a guard that skips the body and returns
    // Ok is a VACUOUS PASS -- the suite reports green for a test that never
    // ran. An ignored test is reported as ignored, which is the honest answer.
    // Same reasoning as ct#448/#449 for the live-YouTube tests.
    //
    // Run them deliberately with `cargo test -p crack-types -- --ignored`.
    // The two `test_parse_url*` tests below are pure and keep running.
    #[ignore = "hits httpbin.org; a third party's uptime is not this repo's CI signal"]
    #[tokio::test]
    async fn test_resolve_final_url() {
        let client = reqwest::Client::new();
        let url = "https://httpbin.org/redirect-to?url=https://example.com";
        let final_url = resolve_final_url(client, url).await.unwrap();
        assert_eq!(final_url.as_str(), "https://example.com/");
    }

    #[ignore = "hits httpbin.org; a third party's uptime is not this repo's CI signal"]
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
