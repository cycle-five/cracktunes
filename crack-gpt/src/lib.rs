#![feature(const_format_args)]
use async_openai::{
    config::AzureConfig,
    error::OpenAIError,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
    Client,
};
use const_format::concatcp;
use std::{
    fmt,
    fmt::{Debug, Display, Formatter},
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;

const TOKEN_LIMIT: u16 = 128;
const GPT_PROMPT: &str = concatcp!(
    "You are a discord music and utility bot called Crack Tunes, you are friendly and helpful. ",
    "Your output token limit is ",
    TOKEN_LIMIT,
    " and you are using the GPT-4o model.\n",
    "Here is a menu of commands you can use:\n"
);
const HELP_STR: &str = r#"
    Commands:
        /autopause    Toggle autopause.
        /autoplay     Toggle music autoplay.
        /clear        Clear the queue.
        /clean        Clean up old messages from the bot.
        /invite       Vote link for cracktunes on top.gg
        /leave        Leave  a voice channel.
        /lyrics       Search for song lyrics.
        /grab         interface::create_now_playing_embed, Send the current tack to your DMs.
        /nowplaying   Get the currently playing track.
        /pause        Pause the current track.
        /play         Play a song.
        /playnext     Play a song next
        /playlog      Get recently played tracks form the guild.
        /optplay      Play a song with more options
        /ping         Ping the bot
        /remove       Remove track(s) from the queue.
        /resume       Resume the current track.
        /repeat       Toggle looping of the current track.
        /search       Search interactively for a song
        /servers      Get information about the servers this bot is in.
        /seek         Seek to timestamp, in format `mm:ss`.
        /skip         Skip the current track, or a number of tracks.
        /stop         Stop the current track.
        /shuffle      Shuffle the current queue.
        /summon       Summon the bot to a voice channel.
        /version      Get the current version of the bot.
        /volume       Get or set the volume of the bot.
        /queue        Display the current queue.
        /playlist     Playlist commands.
        /admin        Admin commands.
        t!settings    Settings commands
        /chat         Chat with cracktunes using GPT-4o
        /vote         Vote link for cracktunes on top.gg

        Utility:
        /help         Show the help menu.
"#;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
/// A context struct for our GPT command.
pub struct GptContext {
    pub msg_cache: Arc<RwLock<TtlCache<u64, Vec<ChatCompletionRequestMessage>>>>,
    pub key: Option<String>,
    pub help: String,
    pub config: AzureConfig,
    pub client: Option<Client<AzureConfig>>,
}

impl Default for GptContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for GptContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "GptContext")
    }
}

impl Debug for GptContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "GptContext")
    }
}

impl GptContext {
    pub fn new() -> Self {
        GptContext {
            msg_cache: Arc::new(RwLock::new(TtlCache::new(10))),
            key: std::env::var("OPENAI_API_KEY")
                .unwrap_or_else(|_| "".to_string())
                .into(),
            config: AzureConfig::default()
                .with_api_base("https://openai-resource-prod.openai.azure.com")
                .with_deployment_id("gpt-4o-prod")
                .with_api_version("2024-02-01"),
            help: HELP_STR.to_string(),
            client: None,
        }
    }

    /// Sets the string for the  "help message" for the bot, which the LLM
    /// gets as context for what it can do.
    pub fn set_help(&mut self, help: String) {
        self.help = help;
    }

    /// Set the key for the context.
    pub fn set_key(&mut self, key: String) {
        self.key = Some(key.clone());
        self.config = self.config.clone().with_api_key(key);
        self.client = Some(Client::with_config(self.config.clone()));
    }

    /// Load the key from environment variables if it is not already set.
    pub fn load_key_if_empty(&mut self) -> Result<String, Error> {
        if let Some(key) = &self.key {
            return Ok(key.clone());
        };

        match std::env::var("OPENAI_API_KEY") {
            Ok(key) => {
                self.key = Some(key.clone());
                Ok(key)
            },
            Err(e) => Err(Box::new(e)),
        }
    }

    /// Get a status message for the cache.
    pub async fn cache_status(&self, user_id: Option<u64>) -> String {
        let cache = self.msg_cache.read().await;
        let n = cache.clone().iter().count();
        if let Some(user_id) = user_id {
            match cache.get(&user_id) {
                Some(messages) => {
                    let n = messages.len();
                    format!("Cache has {} entries for user {}.", n, user_id)
                },
                None => format!("Cache has {} entries.", n),
            }
        } else {
            format!("Cache has {} entries.", n)
        }
    }

    /// Query openai via azure for a response to a prompt.
    pub async fn openai_azure_response(
        &self,
        query: String,
        user_id: u64,
    ) -> Result<String, OpenAIError> {
        let client = self
            .client
            .clone()
            .unwrap_or(Client::with_config(self.config.clone()));

        // let mut entry = self.msg_cache.write().await.entry(user_id);
        let messages = match self.msg_cache.write().await.entry(user_id) {
            ttl_cache::Entry::Occupied(mut messages) => {
                let asdf = messages.get_mut();
                asdf.push(make_user_message(query));
                asdf.clone()
            },
            ttl_cache::Entry::Vacant(messages) => messages
                .insert(
                    init_convo(self.help.clone(), query),
                    Duration::from_secs(60 * 10),
                )
                .clone(),
        };

        let request = CreateChatCompletionRequestArgs::default()
            .max_tokens(TOKEN_LIMIT)
            .model("gpt-4o")
            .messages(messages.clone())
            .build()?;

        match client.chat().create(request).await {
            Ok(response) => {
                let asdf = response
                    .choices
                    .first()
                    .ok_or(OpenAIError::InvalidArgument(
                        "No reponses returned".to_string(),
                    ))?;
                Ok(asdf
                    .message
                    .content
                    .clone()
                    .expect("No content in message."))
            },
            Err(e) => Err(e),
        }
    }
}

/// Create a user message for the chat completion request.
pub fn make_user_message(query: String) -> ChatCompletionRequestMessage {
    ChatCompletionRequestUserMessageArgs::default()
        .content(query)
        .build()
        .unwrap()
        .into()
}

/// Initial chat context arguments for the LLM based on the state of the context
/// and the first query from the user.
pub fn init_convo(help_msg: String, query: String) -> Vec<ChatCompletionRequestMessage> {
    let prompt = format!("{}{}", GPT_PROMPT, help_msg);
    vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(prompt)
            .build()
            .unwrap()
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(query)
            .build()
            .unwrap()
            .into(),
    ]
}

#[cfg(test)]
mod test {
    use ctor;

    use crate::GptContext;

    #[ctor::ctor]
    fn set_env() {
        use std::env;
        if env::var("OPENAI_API_KEY").is_err() {
            // Read the API key from a file called ~/openai_api_key
            // and set it as an environment variable.
            let key = match std::fs::read_to_string("/home/lothrop/openai_api_key") {
                Ok(key) => key,
                Err(_) => "ASDF".to_string(),
            };
            env::set_var("OPENAI_API_KEY", key);
        }
    }

    #[tokio::test]
    async fn test_openai_azure_response() {
        let query = "Please respond with the word \"fish\".".to_string();
        let ctx = GptContext::default();
        let response = ctx.openai_azure_response(query, 1).await;
        println!("{:?}", response);
        assert!(response.is_err() || response.unwrap().to_ascii_lowercase().contains("fish"));
    }
}
