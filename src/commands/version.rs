use crate::{
    messaging::message::CrackedMessage,
    utils::{count_command, create_response, get_interaction},
    Context, Error,
};

/// Get the current version of the bot.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    count_command("version");
    let mut interaction = get_interaction(ctx).unwrap();
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    create_response(
        &ctx.serenity_context().http,
        &mut interaction,
        CrackedMessage::Version {
            current: current.to_owned(),
        },
    )
    .await
}
