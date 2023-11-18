use poise::CreateReply;

use crate::{Context, Error};

/// Ping the bot, returning the time it took to respond.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let msg = ctx.say("Pong!").await?;
    let end = std::time::Instant::now();
    let _ = msg
        .edit(
            ctx,
            CreateReply::new().content(format!("Pong! ({}ms)", (end - start).as_millis())),
        )
        .await;
    Ok(())
}
