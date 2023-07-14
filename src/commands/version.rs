use crate::{
    is_prefix,
    messaging::message::CrackedMessage,
    utils::{count_command, create_response_poise},
    Context, Error,
};

/// Get the current version of the bot.
#[poise::command(slash_command, prefix_command)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    count_command("version", is_prefix(ctx));
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    create_response_poise(
        ctx,
        CrackedMessage::Version {
            current: current.to_owned(),
        },
    )
    .await
}
