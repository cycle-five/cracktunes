use crate::utils::check_reply;
use crate::{Context, Error};
use crack_gpt::GptContext;
use poise::CreateReply;

/// Chat with cracktunes using GPT-4o
#[poise::command(
    category = "AI",
    slash_command,
    prefix_command,
    user_cooldown = "30",
    guild_cooldown = "30"
)]
pub async fn chat(
    ctx: Context<'_>,
    #[rest]
    #[description = "Query to send to the model."]
    query: String,
) -> Result<(), Error> {
    // Do we need this?
    ctx.defer().await?;

    let user_id = ctx.author().id.get();

    let data = ctx.data();

    tracing::info!("chat: {}", query);
    let lock = data.gpt_ctx.read().await;
    let gpt_ctx = if lock.is_some() {
        let res = lock.clone();
        drop(lock);
        res.unwrap()
    } else {
        drop(lock);
        let new_ctx = GptContext::default();
        *ctx.data().gpt_ctx.write().await = Some(new_ctx);

        ctx.data().gpt_ctx.read().await.clone().unwrap()
    };

    tracing::info!("chat: {:?}", gpt_ctx.cache_status(Some(user_id)).await);

    let response = gpt_ctx.openai_azure_response(query, user_id).await?;

    tracing::info!("chat: response: {}", response);

    check_reply(
        ctx.send(CreateReply::default().content(response).reply(true))
            .await,
    );

    Ok(())
}
