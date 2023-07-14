use crate::{messaging::message::CrackedMessage, utils::create_response_poise, Context, Error};

/// Get the current version of the bot.
#[poise::command(slash_command, prefix_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    create_response_poise(
        ctx,
        CrackedMessage::Version {
            current: current.to_owned(),
        },
    )
    .await
}
