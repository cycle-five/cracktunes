use crate::{messaging::message::CrackedMessage, utils::create_response_poise, Context, Error};
use reqwest::Url;

/// Fetch a historical snapshot of a given URL from the Wayback Machine.
#[poise::command(slash_command, prefix_command)]
pub async fn wayback(
    ctx: Context<'_>,
    #[description = "url to retreive snapshot of"] url: String,
) -> Result<(), Error> {
    // Validate the URL
    let url = Url::parse(&url)?;

    // Construct the URL for the CDX Server API request
    let api_url = format!(
        "http://web.archive.org/cdx/search/cdx?url={}&output=json&limit=1",
        url
    );

    // Send the API request
    let response: Vec<Vec<String>> = reqwest::get(&api_url).await?.json().await?;

    // The first item in the response is the field names, so we get the second item for the first snapshot
    let snapshot = &response.get(1).ok_or("No snapshots found")?;

    // Construct the URL for the snapshot
    let snapshot_url = format!("http://web.archive.org/web/{}id_/{}/", snapshot[1], url);

    // Send the snapshot URL as the command's response
    create_response_poise(ctx, CrackedMessage::WaybackSnapshot { url: snapshot_url }).await?;

    Ok(())
}
