use chatgpt::{
    err::Error as ChatGPTError,
    prelude::{ChatGPT, ChatGPTEngine, ModelConfigurationBuilder},
};
use url::Url;

use async_openai::{
    config::AzureConfig,
    types::{
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
    Client,
};

/// Get the response from ChatGPT to the given query string.
pub async fn get_chatgpt_response(query: String, endpoint: String) -> Result<String, ChatGPTError> {
    let key = std::env::var("OPENAI_API_KEY").expect("No OPENAI_API_KEY environment variable set.");

    let content = query;
    tracing::info!("{:?}", content);
    // Creating a new ChatGPT client.
    // Note that it requires an API key, and uses
    // tokens from your OpenAI API account balance.
    let _models = [
        ChatGPTEngine::Gpt35Turbo,
        ChatGPTEngine::Gpt35Turbo_0301,
        ChatGPTEngine::Gpt4,
        ChatGPTEngine::Gpt4_32k,
        ChatGPTEngine::Gpt4_0314,
        ChatGPTEngine::Gpt4_32k_0314,
    ];
    let config = ModelConfigurationBuilder::default()
        .api_url(
            Url::parse(&endpoint)
                .map_err(|e| chatgpt::err::Error::ParsingError(e.to_string()))?,
        )
        // .temperature(1.0)
        // .engine(ChatGPTEngine::Gpt35Turbo)
        // .top_p(1.0)
        // .frequency_penalty(0.5)
        // .presence_penalty(0.0)
        // .max_tokens(150)
        .build()
        .unwrap();
    let client = ChatGPT::new_with_config(key, config)?;

    tracing::info!("Client created.");

    // Sending a message and getting the completion
    let response = match client.send_message(content).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("Failed to send message: {}", e);
            return Err(e);
        },
    };

    tracing::info!("Response received: {:?}", response);

    Ok::<String, ChatGPTError>(response.message().content.clone())
}

use async_openai::error::OpenAIError;
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
                .content("You are a discord music and utility Bot. You are friendly and helpful, and especially knowledgeable about music, math, and technology.")
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

    Ok(asdf.message.content.clone().expect("No content in message."))
}

#[cfg(test)]
mod test {
    use crate::get_chatgpt_response;
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
    async fn test_get_chatgpt_response() {
        let query = "Please respond with the word \"fish\".".to_string();
        let response = get_chatgpt_response(query, "localhost".to_string()).await;
        println!("{:?}", response);
        assert!(response.is_err() || response.unwrap().to_ascii_lowercase().contains("fish"));
    }

    #[tokio::test]
    async fn test_openai_azure_response() {
        let query = "Please respond with the word \"fish\".".to_string();
        let response = crate::openai_azure_response(query).await;
        println!("{:?}", response);
        assert!(response.is_err() || response.unwrap().to_ascii_lowercase().contains("fish"));
    }
}
