use crack_core::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};
use reqwest::Url;

pub async fn fetch_wayback_snapshot(url: &str) -> Result<String, Error> {
    // Validate the URL
    let url = Url::parse(url)?;

    // Construct the URL for the CDX Server API request
    let api_url = format!(
        "http://web.archive.org/cdx/search/cdx?url={}&output=json&limit=1",
        url
    );

    let client = reqwest::ClientBuilder::new().use_rustls_tls().build()?;

    // Send the API request
    let response: Vec<Vec<String>> = client.get(&api_url).send().await?.json().await?;

    // The first item in the response is the field names, so we get the second item for the first snapshot
    let snapshot = &response.get(1).ok_or("No snapshots found")?;

    // Construct the URL for the snapshot
    let snapshot_url = format!("http://web.archive.org/web/{}id_/{}/", snapshot[1], url);

    Ok(snapshot_url)
}

/// Fetch a historical snapshot of a given URL from the Wayback Machine.
#[poise::command(prefix_command, hide_in_help)]
pub async fn wayback(
    ctx: Context<'_>,
    #[description = "url to retreive snapshot of"] url: String,
) -> Result<(), Error> {
    match fetch_wayback_snapshot(&url).await {
        Ok(snapshot_url) => {
            // Send the snapshot URL as the command's response
            send_reply(&ctx, CrackedMessage::WaybackSnapshot { url: snapshot_url }).await?;
            Ok(())
        },
        Err(e) => Err(e),
    }
}
