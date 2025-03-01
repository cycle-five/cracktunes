use once_cell::sync::Lazy;
use reqwest::Client;
use std::future::Future;

use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::messaging::{message::CrackedMessage, messages::UNKNOWN};
use crate::music::NewQueryType;
use crate::serenity::Color;
use crate::CrackedResult;
use serenity::all::{CacheHttp, ChannelId, CreateEmbed, CreateMessage, GuildId, Message, UserId};
use serenity::small_fixed_array::FixedString;

#[derive(Debug)]
/// Parameter structure for functions that send messages to a channel.
pub struct SendMessageParams<'a> {
    pub channel: ChannelId,
    pub as_embed: bool,
    pub ephemeral: bool,
    pub reply: bool,
    pub color: Color,
    pub cache_msg: bool,
    pub msg: CrackedMessage,
    pub embed: Option<CreateEmbed<'a>>,
}

/// Implement [`PartialEq`] for [`SendMessageParams`].
impl PartialEq for SendMessageParams<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.channel == other.channel
            && self.as_embed == other.as_embed
            && self.ephemeral == other.ephemeral
            && self.reply == other.reply
            && self.color == other.color
            && self.cache_msg == other.cache_msg
            && self.msg == other.msg
        // Note: We don't compare `embed` here
    }
}

/// Default implementation for [`SendMessageParams`].
impl Default for SendMessageParams<'_> {
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

/// Builder methods for [`SendMessageParams`].
// TODO: Do we want to use this pattern or the mutable one here?
impl<'a> SendMessageParams<'a> {
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

    pub fn with_embed(self, embed: Option<CreateEmbed<'a>>) -> Self {
        Self { embed, ..self }
    }
}

/// Extension trait for CacheHttp to add some utility functions.
pub trait CacheHttpExt {
    fn get_bot_id(&self) -> impl Future<Output = CrackedResult<UserId>> + Send;
    fn user_id_to_username_or_default(
        &self,
        user_id: UserId,
    ) -> impl Future<Output = String> + Send;
    fn channel_id_to_guild_name(
        &self,
        channel_id: ChannelId,
        guild_id: GuildId,
    ) -> impl Future<Output = CrackedResult<FixedString>> + Send;
    fn send_channel_message(
        &self,
        params: SendMessageParams,
    ) -> impl Future<Output = CrackedResult<Message>> + Send;
    fn guild_name_from_guild_id(
        &self,
        guild_id: GuildId,
    ) -> impl Future<Output = CrackedResult<FixedString>> + Send;
}

/// Implement the CacheHttpExt trait for any type that implements CacheHttp.
impl<T: CacheHttp> CacheHttpExt for T {
    async fn get_bot_id(&self) -> CrackedResult<UserId> {
        get_bot_id(self).await
    }

    async fn user_id_to_username_or_default(&self, user_id: UserId) -> String {
        cache_to_username_or_default(self, user_id).await
    }

    async fn channel_id_to_guild_name(
        &self,
        channel_id: ChannelId,
        guild_id: GuildId,
    ) -> CrackedResult<FixedString> {
        get_guild_name(self, channel_id, guild_id).await
    }

    /// Sends a message to a channel.
    #[cfg(not(tarpaulin_include))]
    async fn send_channel_message(&self, params: SendMessageParams<'_>) -> CrackedResult<Message> {
        let channel = params.channel;
        let content = format!("{}", params.msg);
        let msg = if params.as_embed {
            let embed = CreateEmbed::default().description(content);
            CreateMessage::new().add_embed(embed)
        } else {
            CreateMessage::new().content(content)
        };
        channel
            .send_message(self.http(), msg)
            .await
            .map_err(Into::into)
    }

    async fn guild_name_from_guild_id(
        &self,
        guild_id: GuildId,
    ) -> Result<FixedString, CrackedError> {
        guild_name_from_guild_id(self, guild_id).await
    }
}

/// This is a hack to get around the fact that we can't use async in statics. Is it?
static CLIENT: Lazy<Client> = Lazy::new(|| {
    println!("Creating a new reqwest client...");
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .expect("Failed to build reqwest client")
});

// /// This is a hack to get around the fact that we can't use async in statics. Is it?
// static CLIENT_OLD: Lazy<reqwest_old::Client> = Lazy::new(|| {
//     println!("Creating a new (old) reqwest client...");
//     reqwest_old::ClientBuilder::new()
//         .use_rustls_tls()
//         .cookie_store(true)
//         .build()
//         .expect("Failed to build reqwest client")
// });

/// Build a reqwest client with rustls.
pub fn build_client() -> Client {
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .expect("Failed to build reqwest client")
}

/// Get a reference to the lazy, static, global reqwest client.
pub fn get_client() -> &'static Client {
    &CLIENT
}

/// Get a reference to an old version client.
pub fn get_client_old() -> &'static reqwest::Client {
    &CLIENT
}

/// Initialize the static, global reqwest client.
pub async fn init_http_client() -> Result<(), CrackedError> {
    let client = get_client();
    let client_old = get_client_old();
    let res1 = client.get("https://httpbin.org/ip").send().await?;
    // This is really weird, it causes a bug if you don't implement the conversion
    // for the error type in both the new and old version of the library.
    let res2 = client_old.get("https://httpbin.org/ip").send().await?;
    let status1 = res1.status();
    let status2 = res2.status();
    // let body = res.text().await?;
    tracing::info!(
        "HTTP client initialized successfully: {:?}",
        status1.clone()
    );
    tracing::info!(
        "HTTP client initialized successfully: {:?}",
        status2.clone()
    );
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
pub async fn cache_to_username_or_default(cache_http: impl CacheHttp, user_id: UserId) -> String {
    match user_id.to_user(cache_http).await {
        Ok(x) => x.name.to_string(),
        Err(_) => {
            tracing::warn!("cache.user returned None");
            UNKNOWN.to_string()
        },
    }
}

/// Parse a URL string into a URL object.
pub async fn parse_url(url: &str) -> Result<url::Url, CrackedError> {
    url::Url::parse(url).map_err(Into::into)
}

/// Gets the final URL after following all redirects.
pub async fn resolve_final_url(url: &str) -> Result<String, CrackedError> {
    // FIXME: This is definitely not efficient, we want ot reuse this client.
    // Make a GET request, which will follow redirects by default

    let client = get_client();
    let response = client.get(url).send().await?;

    // Extract the final URL after following all redirects
    let final_url = response.url().clone();

    Ok(final_url.into())
}

/// Gets the guild_name for a channel_id.
#[cfg(not(tarpaulin_include))]
pub async fn get_guild_name(
    cache_http: &impl CacheHttp,
    channel_id: ChannelId,
    guild_id: GuildId,
) -> Result<FixedString, CrackedError> {
    channel_id
        .to_channel(cache_http, Some(guild_id))
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
) -> Result<FixedString, CrackedError> {
    guild_id
        .to_partial_guild(cache_http)
        .await
        .map(|x| x.name)
        .map_err(Into::into)
}

/// Check if the domain that we're playing from is banned.
// FIXME: This is borked.
pub fn check_banned_domains(
    _guild_settings: &GuildSettings,
    query_type: Option<NewQueryType>,
) -> CrackedResult<Option<NewQueryType>> {
    Ok(query_type)
    // if let Some(NewQueryType(QueryType::Keywords(_))) = query_type {
    //     if !guild_settings.allow_all_domains.unwrap_or(true)
    //         && (guild_settings.banned_domains.contains("youtube.com")
    //             || (guild_settings.banned_domains.is_empty()
    //                 && !guild_settings.allowed_domains.contains("youtube.com")))
    //     {
    //         Err(CrackedError::Other("youtube.com is banned"))
    //     } else {
    //         Ok(query_type)
    //     }
    // } else {
    //     Ok(query_type)
    // }
}

#[cfg(test)]
mod test {
    use crate::http_utils::resolve_final_url;

    #[tokio::test]
    async fn test_resolve_final_url() {
        let url = "https://example.com";

        let final_url = resolve_final_url(url).await.unwrap();
        assert_eq!(final_url, "https://example.com/");
    }

    #[test]
    fn test_build_send_message_params() {
        use crate::http_utils::SendMessageParams;
        use crate::messaging::message::CrackedMessage;
        use serenity::all::{ChannelId, Colour};

        let channel_id = ChannelId::new(1);
        let msg = CrackedMessage::Other("Hello, world!".to_string());
        let params = SendMessageParams::new(msg)
            .with_as_embed(true)
            .with_ephemeral(false)
            .with_reply(true)
            .with_color(Colour::BLUE)
            .with_cache_msg(true)
            .with_channel(channel_id)
            .with_embed(None);

        assert_eq!(params.channel, channel_id);
        assert_eq!(params.as_embed, true);
        assert_eq!(params.ephemeral, false);
        assert_eq!(params.reply, true);
        assert_eq!(params.color, Colour::BLUE);
        assert_eq!(params.cache_msg, true);
        assert_eq!(
            params.msg,
            CrackedMessage::Other("Hello, world!".to_string())
        );
        assert_eq!(params.embed.is_none(), true);
    }
}
