use crate::utils::check_reply;
use crate::{Context, Error};
use crack_gpt::GptContext;
use poise::CreateReply;

/// Chat with cracktunes using GPT-4o
#[poise::command(slash_command, prefix_command)]
pub async fn chat(
    ctx: Context<'_>,
    #[rest]
    #[description = "Query to send to the model."]
    query: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let user_id = ctx.author().id.get();

    let gpt_ctx = GptContext::default();

    let response = gpt_ctx.openai_azure_response(query, user_id).await?;

    check_reply(
        ctx.send(CreateReply::default().content(response).reply(true))
            .await,
    );

    Ok(())
}
