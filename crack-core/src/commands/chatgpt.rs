use crate::utils::check_reply;
use crate::{Context, Error};
use crack_gpt::openai_azure_response;
use poise::CreateReply;

/// Talk with chatgpt.
#[poise::command(slash_command, prefix_command)]
pub async fn chatgpt(
    ctx: Context<'_>,
    #[rest]
    #[description = "Query to send to the model."]
    query: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let response = openai_azure_response(query).await?;

    check_reply(
        ctx.send(CreateReply::default().content(response).reply(true))
            .await,
    );

    Ok(())
}
