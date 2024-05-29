use poise::CreateReply;

use crate::{Context, Error};

/// Ping the bot
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ping_internal(ctx).await
}

/// Ping the bot internal function
#[cfg(not(tarpaulin_include))]
pub async fn ping_internal(ctx: Context<'_>) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let msg = ctx.say("Pong!").await?;
    let end = std::time::Instant::now();
    let _ = msg
        .edit(
            ctx,
            CreateReply::default().content(format!("Pong! ({}ms)", (end - start).as_millis())),
        )
        .await;
    Ok(())
}
