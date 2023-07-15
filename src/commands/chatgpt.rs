use crate::{utils::check_reply, Context, Error};
use chatgpt::prelude::{ChatGPT, ChatGPTEngine, ModelConfigurationBuilder};
use url::Url;

/// Talk with chatgpt.
#[poise::command(slash_command, prefix_command)]
pub async fn chatgpt(
    ctx: Context<'_>,
    #[rest]
    #[description = "Query text to send to the model."]
    query: String,
) -> Result<(), Error> {
    let key = std::env::var("OPENAI_KEY").expect("Expected an OpenAI key in the environment");

    ctx.defer().await?;

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
        .api_url(Url::parse("https://api.pawan.krd/v1/chat/completions").unwrap())
        .temperature(1.0)
        .engine(ChatGPTEngine::Gpt35Turbo)
        .build()
        .unwrap();
    tracing::info!("{:?}", config);
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

    //check_msg(msg.reply(&ctx, response).await);
    // check_reply(ctx.send(|m| m.content(response).reply(true)).await);
    check_reply(ctx.send(|m| m.content(response).reply(true)).await);

    Ok(())
}
