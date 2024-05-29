use async_openai::{
    config::AzureConfig,
    error::OpenAIError,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
    Client,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use ttl_cache::TtlCache;

const GPT_PROMPT: &str = "You are a discord music and utility bot called Crack Tunes, you are friendly and helpful. You have a 64 token output limit and no memory between questions.";

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone)]
/// A context struct for our GPT command.
pub struct GptContext {
    pub msg_cache: Arc<RwLock<TtlCache<u64, Vec<ChatCompletionRequestMessage>>>>,
    pub key: Option<String>,
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
            key: None,
        }
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
        self.key = Some(key);
    }
}

pub async fn openai_azure_response(query: String) -> Result<String, OpenAIError> {
    let key = std::env::var("OPENAI_API_KEY").expect("No OPENAI_API_KEY environment variable set.");

    let config = AzureConfig::new()
        .with_api_base("https://openai-resource-prod.openai.azure.com")
        .with_api_key(key)
        .with_deployment_id("gpt-4o-prod")
        .with_api_version("2024-02-01");

    let client = Client::with_config(config);

    let request = CreateChatCompletionRequestArgs::default()
        .max_tokens(64u16)
        //.model("gpt-3.5-turbo")
        .model("gpt-4o")
        .messages([
            ChatCompletionRequestSystemMessageArgs::default()
                .content(GPT_PROMPT)
                .build()?
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content(query)
                .build()?
                .into(),
        ])
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

#[cfg(test)]
mod test {
    use ctor;

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
        let response = crate::openai_azure_response(query).await;
        println!("{:?}", response);
        assert!(response.is_err() || response.unwrap().to_ascii_lowercase().contains("fish"));
    }
}
