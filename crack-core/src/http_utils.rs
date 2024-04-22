use crate::errors::CrackedError;
use serenity::all::{ChannelId, GuildId, Http, UserId};

use once_cell::sync::Lazy;
use reqwest::Client;

static CLIENT: Lazy<Client> = Lazy::new(|| {
    println!("Creating a new client..."); // Optional: for demonstration
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .build()
        .expect("Failed to build reqwest client")
});

pub fn get_client() -> &'static Client {
    &CLIENT
}

/// Get a new reqwest client with consistent settings.
pub fn new_reqwest_client() -> &'static Client {
    &CLIENT
}

/// Get the bot's user ID.
#[cfg(not(tarpaulin_include))]
pub async fn get_bot_id(http: &Http) -> Result<UserId, CrackedError> {
    let tune_titan_id = UserId::new(1124707756750934159);
    let rusty_bot_id = UserId::new(1111844110597374042);
    let cracktunes_id = UserId::new(1115229568006103122);
    let bot_id = http.get_current_user().await?.id;

    // If the bot is tune titan or rusty bot, return cracktunes ID
    if bot_id == tune_titan_id || bot_id == rusty_bot_id {
        Ok(cracktunes_id)
    } else {
        Ok(bot_id)
    }
}

/// Get the username of a user from their user ID, returns "Unknown" if an error occurs.
#[cfg(not(tarpaulin_include))]
pub async fn http_to_username_or_default(http: &Http, user_id: UserId) -> String {
    match http.get_user(user_id).await {
        Ok(x) => x.name,
        Err(e) => {
            tracing::error!("http.get_user error: {}", e);
            "Unknown".to_string()
        },
    }
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // FIXME: This is definitely not efficient, we want ot reuse this client.
    // Make a GET request, which will follow redirects by default
    let client = new_reqwest_client();
    let response = client.get(url).send().await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.as_str().to_string())
}

/// Gets the guild_name for a channel_id.
#[cfg(not(tarpaulin_include))]
pub async fn get_guild_name(http: &Http, channel_id: ChannelId) -> Result<String, CrackedError> {
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
#[cfg(not(tarpaulin_include))]
pub async fn get_guild_name_from_guild_id(
    http: &Http,
    guild_id: GuildId,
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
