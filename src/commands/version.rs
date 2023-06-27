use crate::{
    messaging::message::ParrotMessage,
    utils::{create_response, get_interaction},
    Context, Error,
};

/// Get the current version of the bot.
#[poise::command(slash_command, prefix_command, guild_only)]
pub async fn version(ctx: Context<'_>) -> Result<(), Error> {
    let mut interaction = get_interaction(ctx).unwrap();
    let current = option_env!("CARGO_PKG_VERSION").unwrap_or_else(|| "Unknown");
    create_response(
        &ctx.serenity_context().http,
        &mut interaction,
        ParrotMessage::Version {
            current: current.to_owned(),
        },
    )
    .await
}
