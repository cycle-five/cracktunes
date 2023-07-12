use crate::{Context, Error};

/// Ping the bot.
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    //COMMAND_EXECUTIONS.with_label_values(&["ping"]).inc();
    ctx.say("Pong!").await?;
    Ok(())
}
