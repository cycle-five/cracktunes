use crate::errors::CrackedError;

pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // Make a GET request, which will follow redirects by default
    let response = reqwest::get(url).await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.as_str().to_string())
}
