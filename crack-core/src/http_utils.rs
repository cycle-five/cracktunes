use crate::errors::CrackedError;

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // Make a GET request, which will follow redirects by default
    let response = reqwest::get(url).await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.as_str().to_string())
}

/// Gets the guild_name for a channel_id.
pub async fn get_guild_name(
    http: &serenity::http::Http,
    channel_id: serenity::model::id::ChannelId,
) -> Result<String, CrackedError> {
    channel_id
        .to_channel(http)
        .await?
        .guild()
        .map(|x| x.guild_id)
        .ok_or(CrackedError::NoGuildForChannelId(channel_id))?
        .to_partial_guild(http)
        .await
        .map(|x| x.name)
        .map_err(|e| e.into())
}

// Get the guild name from the guild id and an http client.
pub async fn get_guild_name_from_guild_id(
    http: &serenity::http::Http,
    guild_id: serenity::model::id::GuildId,
) -> Result<String, CrackedError> {
    guild_id
        .to_partial_guild(http)
        .await
        .map(|x| x.name)
        .map_err(|e| e.into())
}

#[cfg(test)]
mod test {
    use crate::http_utils::resolve_final_url;

    #[tokio::test]
    async fn test_resolve_final_url() {
        let url = "https://example.com";

        let final_url = resolve_final_url(url).await.unwrap();
        // assert_eq!(final_url, "https://example.com/");
        assert_eq!(final_url, "https://example.com/");
    }
}
