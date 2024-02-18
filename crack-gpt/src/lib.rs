///
use chatgpt::{
    err::Error,
    prelude::{ChatGPT, ChatGPTEngine, ModelConfigurationBuilder},
};
use url::Url;

pub async fn get_chatgpt_response(query: String) -> Result<String, Error> {
    let key = std::env::var("OPENAI_API_KEY")?;

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
            Url::parse("https://api.pawan.krd/v1/chat/completions")
                .map_err(|e| chatgpt::err::Error::ParsingError(e.to_string()))?,
        )
        .temperature(1.0)
        .engine(ChatGPTEngine::Custom("pai-001-light-beta"))
        .build()
        .unwrap();
    // .top_p(1.0)
    // .frequency_penalty(0.5)
    // .presence_penalty(0.0)
    // .max_tokens(150)
    let client = ChatGPT::new_with_config(key, config)?;

    tracing::info!("Client created.");

    // Sending a message and getting the completion
    let response = client.send_message(content).await.map_or_else(
        |e| {
            tracing::error!("Failed to send message: {}", e);
            e.to_string()
        },
        |k| k.message().content.clone(),
    );

    tracing::info!("Response received: {:?}", response);

    Ok::<String, Error>(response)
}

#[cfg(test)]
mod test {
    use crate::get_chatgpt_response;

    #[tokio::test]
    async fn test_get_chatgpt_response() {
        let query = "Hello. Please say hello".to_string();
        let response = get_chatgpt_response(query).await;
        println!("{:?}", response);
        assert!(
            response.is_err()
                || response.as_ref().unwrap().contains("Invalid API key")
                || response.unwrap().contains("Hello")
        )
    }
}
