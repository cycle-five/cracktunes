// https://12ft.io/
use crack_core::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

/// paywall bypass
#[poise::command(prefix_command, hide_in_help)]
pub async fn paywall(ctx: Context<'_>, url: String) -> Result<(), Error> {
    let message = CrackedMessage::Paywall(url);

    send_response_poise(ctx, message).await?;

    Ok(())
}
