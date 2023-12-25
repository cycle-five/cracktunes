use crate::errors::CrackedError;

pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // Make a GET request, which will follow redirects by default
    let response = reqwest::get(url).await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.as_str().to_string())
}

pub async fn get_guild_name(
    http: &serenity::http::Http,
    channel_id: serenity::model::id::ChannelId,
) -> Result<String, CrackedError> {
    channel_id
        .to_channel(http)
        .await?
        .guild()
        .map(|x| x.guild_id)
        .ok_or(CrackedError::Other("No guild found for channel"))?
        .to_partial_guild(http)
        .await
        .map(|x| x.name)
        .map_err(|e| e.into())
}
