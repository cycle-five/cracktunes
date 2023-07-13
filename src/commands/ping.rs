use crate::{is_prefix, Context, Error};

/// Ping the bot.
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    crate::utils::count_command("ping", is_prefix(ctx));
    ctx.say("Pong!").await?;
    Ok(())
}
