// https://12ft.io/
use crate::{messaging::message::CrackedMessage, utils::create_response_poise, Context, Error};

/// paywall bypass
#[poise::command(prefix_command, hide_in_help)]
pub async fn paywall(ctx: Context<'_>, url: String) -> Result<(), Error> {
    let message = CrackedMessage::Paywall(url);

    create_response_poise(ctx, message).await?;

    Ok(())
}
