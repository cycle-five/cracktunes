use crate::utils::check_reply;
use crate::Context;
use crate::Error;
use crack_gpt::get_chatgpt_response;

#[cfg(feature = "crack-gpt")]
/// Talk with chatgpt.
#[allow(unused)]
#[poise::command(slash_command, prefix_command)]
pub async fn chatgpt(
    ctx: Context<'_>,
    #[rest]
    #[description = "Query text to send to the model."]
    query: String,
) -> Result<(), Error> {
    use poise::{CreateReply, ReplyHandle};

    ctx.defer().await?;

    let response = get_chatgpt_response(query).await?;

    check_reply(
        ctx.send(CreateReply::default().content(response).reply(true))
            .await,
    );
    return Ok(());
}
