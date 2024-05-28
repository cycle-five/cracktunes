use crate::messaging::help::CHATGPT_QUERY_DESCRIPTION;
use crate::utils::check_reply;
use crate::{Context, Error};
use crack_gpt::get_chatgpt_response;

use poise::{CreateReply, ReplyHandle};

#[cfg(feature = "crack-gpt")]
/// Talk with chatgpt.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn chatgpt(
    ctx: Context<'_>,
    #[rest]
    #[description = CHATGPT_QUERY_DESCRIPTION]
    query: String,
) -> Result<(), Error> {
    ctx.defer().await?;

    let response = get_chatgpt_response(query).await?;

    check_reply(
        ctx.send(CreateReply::default().content(response).reply(true))
            .await,
    );

    Ok(())
}
