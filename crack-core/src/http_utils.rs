use once_cell::sync::Lazy;
use reqwest::Client;
use std::future::Future;

use crate::errors::CrackedError;
use crate::messaging::{message::CrackedMessage, messages::UNKNOWN};
use crate::serenity::Color;
use serenity::all::{
    CacheHttp, ChannelId, CreateEmbed, CreateMessage, GuildId, Http, Message, UserId,
};

/// Parameter structure for functions that send messages to a channel.
#[derive(Debug, PartialEq)]
pub struct SendMessageParams {
    pub channel: ChannelId,
    pub as_embed: bool,
    pub ephemeral: bool,
    pub reply: bool,
    pub color: Color,
    pub cache_msg: bool,
    pub msg: CrackedMessage,
    pub embed: Option<CreateEmbed>,
}

impl Default for SendMessageParams {
    fn default() -> Self {
        SendMessageParams {
            channel: ChannelId::new(1),
            as_embed: true,
            ephemeral: false,
            reply: true,
            color: Color::BLUE,
            cache_msg: true,
            msg: CrackedMessage::Other(String::new()),
            embed: None,
        }
    }
}

impl SendMessageParams {
    pub fn new(msg: CrackedMessage) -> Self {
        Self {
            msg,
            ..Default::default()
        }
    }

    pub fn with_as_embed(self, as_embed: bool) -> Self {
        Self { as_embed, ..self }
    }

    pub fn with_ephemeral(self, ephemeral: bool) -> Self {
        Self { ephemeral, ..self }
    }

    pub fn with_reply(self, reply: bool) -> Self {
        Self { reply, ..self }
    }

    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }

    pub fn with_msg(self, msg: CrackedMessage) -> Self {
        Self { msg, ..self }
    }

    pub fn with_channel(self, channel: ChannelId) -> Self {
        Self { channel, ..self }
    }

    pub fn with_cache_msg(self, cache_msg: bool) -> Self {
        Self { cache_msg, ..self }
    }

    pub fn with_embed(self, embed: CreateEmbed) -> Self {
        Self {
            embed: Some(embed),
            ..self
        }
    }
}

/// Extension trait for CacheHttp to add some utility functions.
pub trait CacheHttpExt {
    fn cache(&self) -> Option<impl CacheHttp>;
    fn http(&self) -> Option<&Http>;
    fn get_bot_id(&self) -> impl Future<Output = Result<UserId, CrackedError>> + Send;
    fn user_id_to_username_or_default(&self, user_id: UserId) -> String;
    fn channel_id_to_guild_name(
        &self,
        channel_id: ChannelId,
    ) -> impl Future<Output = Result<String, CrackedError>> + Send;
    fn send_channel_message(
        &self,
        params: SendMessageParams,
    ) -> impl Future<Output = Result<serenity::model::channel::Message, CrackedError>> + Send;
    fn guild_name_from_guild_id(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = Result<String, CrackedError>> + Send;
}

/// Implement the CacheHttpExt trait for any type that implements CacheHttp.
impl<T: CacheHttp> CacheHttpExt for T {
    fn cache(&self) -> Option<impl CacheHttp> {
        Some(self)
    }

    fn http(&self) -> Option<&Http> {
        Some(self.http())
    }

    async fn get_bot_id(&self) -> Result<UserId, CrackedError> {
        get_bot_id(self).await
    }

    fn user_id_to_username_or_default(&self, user_id: UserId) -> String {
        cache_to_username_or_default(self, user_id)
    }

    async fn channel_id_to_guild_name(
        &self,
        channel_id: ChannelId,
    ) -> Result<String, CrackedError> {
        get_guild_name(self, channel_id).await
    }

    /// Sends a message to a channel.
    #[cfg(not(tarpaulin_include))]
    async fn send_channel_message(
        &self,
        params: SendMessageParams,
    ) -> Result<Message, CrackedError> {
        let channel = params.channel;
        let content = format!("{}", params.msg);
        let msg = if params.as_embed {
            let embed = CreateEmbed::default().description(content);
            CreateMessage::new().add_embed(embed)
        } else {
            CreateMessage::new().content(content)
        };
        channel.send_message(self, msg).await.map_err(Into::into)
    }

    async fn guild_name_from_guild_id(&self, guild_id: GuildId) -> Result<String, CrackedError> {
        guild_name_from_guild_id(self, guild_id).await
    }
}

/// This is a hack to get around the fact that we can't use async in statics. Is it?
static CLIENT: Lazy<Client> = Lazy::new(|| {
    println!("Creating a new reqwest client...");
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .build()
        .expect("Failed to build reqwest client")
});

/// Get a reference to the lazy, static, global reqwest client.
pub fn get_client() -> &'static Client {
    &CLIENT
}

/// Initialize the static, global reqwest client.
pub async fn init_http_client() -> Result<(), CrackedError> {
    let client = get_client().clone();
    let res = client.get("https://httpbin.org/ip").send().await?;
    tracing::info!("HTTP client initialized successfully: {:?}", res);
    Ok(())
}
/// Get the bot's user ID.
#[cfg(not(tarpaulin_include))]
pub async fn get_bot_id(cache_http: impl CacheHttp) -> Result<UserId, CrackedError> {
    let tune_titan_id = UserId::new(1124707756750934159);
    let rusty_bot_id = UserId::new(1111844110597374042);
    let cracktunes_id = UserId::new(1115229568006103122);
    let bot_id = match cache_http.cache() {
        Some(cache) => cache.current_user().id,
        None => {
            tracing::warn!("cache_http.cache() returned None");
            return Err(CrackedError::Other("cache_http.cache() returned None"));
        },
    };

    // If the bot is tune titan or rusty bot, return cracktunes ID
    if bot_id == tune_titan_id || bot_id == rusty_bot_id {
        Ok(cracktunes_id)
    } else {
        Ok(bot_id)
    }
}

/// Get the username of a user from their user ID, returns "Unknown" if an error occurs.
#[cfg(not(tarpaulin_include))]
pub fn cache_to_username_or_default(cache_http: impl CacheHttp, user_id: UserId) -> String {
    // let asdf = cache.cache()?.user(user_id);

    match cache_http.cache() {
        Some(cache) => match cache.user(user_id) {
            Some(x) => x.name.clone(),
            None => {
                tracing::warn!("cache.user returned None");
                UNKNOWN.to_string()
            },
        },
        None => {
            tracing::warn!("cache_http.cache() returned None");
            UNKNOWN.to_string()
        },
    }
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // FIXME: This is definitely not efficient, we want ot reuse this client.
    // Make a GET request, which will follow redirects by default

    let client = get_client();
    let response = client.get(url).send().await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.as_str().to_string())
}

/// Gets the guild_name for a channel_id.
#[cfg(not(tarpaulin_include))]
pub async fn get_guild_name(
    cache_http: &impl CacheHttp,
    channel_id: ChannelId,
) -> Result<String, CrackedError> {
    channel_id
        .to_channel(cache_http)
        .await?
        .guild()
        .map(|x| x.guild_id)
        .ok_or(CrackedError::NoGuildForChannelId(channel_id))?
        .to_partial_guild(cache_http)
        .await
        .map(|x| x.name)
        .map_err(Into::into)
}

// Get the guild name from the guild id and an http client.
#[cfg(not(tarpaulin_include))]
pub async fn guild_name_from_guild_id(
    cache_http: impl CacheHttp,
    guild_id: GuildId,
) -> Result<String, CrackedError> {
    guild_id
        .to_partial_guild(cache_http)
        .await
        .map(|x| x.name)
        .map_err(Into::into)
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
