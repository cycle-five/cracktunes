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
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use ttl_cache::TtlCache;

const TOKEN_LIMIT: u16 = 128;
const GPT_PROMPT: &str = concatcp!(
    "You are a discord music and utility bot called Crack Tunes, you are friendly and helpful.",
    "Here is a menu of commands you can use:\n",
);

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
            help: "chat: Chat with Crack Tunes using GPT-4o.".to_string(),
            client: None,
        }
    }

    pub fn set_help(&mut self, help: String) {
        self.help = help;
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

    /// Set the key for the context.
    pub fn set_key(&mut self, key: String) {
        self.key = Some(key.clone());
        self.config = self.config.clone().with_api_key(key);
        self.client = Some(Client::with_config(self.config.clone()));
    }

    async fn init_convo(&self, query: String) -> Vec<ChatCompletionRequestMessage> {
        let prompt = format!("{}{}", GPT_PROMPT, self.help);
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

    /// Create a user message for the chat completion request.
    fn make_user_message(&self, query: String) -> ChatCompletionRequestMessage {
        ChatCompletionRequestUserMessageArgs::default()
            .content(query)
            .build()
            .unwrap()
            .into()
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
                asdf.push(self.make_user_message(query));
                asdf.clone()
            },
            ttl_cache::Entry::Vacant(messages) => messages
                .insert(self.init_convo(query).await, Duration::from_secs(60 * 10))
                .clone(),
        };
        // let asdf = messages.entry(user_id).or_insert(self.init_convo(query));
        // let messages = match messages {
        //     Some(messages) => {
        //         messages.push(self.make_user_message(query));
        //         messages
        //     },
        //     None => self.init_convo(query),
        // };

        let request = CreateChatCompletionRequestArgs::default()
            .max_tokens(TOKEN_LIMIT)
            .model("gpt-4o")
            .messages(messages.clone())
            .build()?;

        let res1 = client.chat().create(request).await;
        let response = res1.unwrap();
        let asdf = response.choices.first().expect("No choices in response.");

        Ok(asdf
            .message
            .content
            .clone()
            .expect("No content in message."))
    }
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
