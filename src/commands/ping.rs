use crate::{Context, Error};

/// Ping the bot, returning the time it took to respond.
/// This is useful for checking the bot's latency.
#[poise::command(slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let msg = ctx
        .say("Pong!")
        .await
        .expect("failed to send message to channel");
    let end = std::time::Instant::now();
    let _ = msg
        .edit(ctx, |m| {
            m.content(format!("Pong! ({}ms)", (end - start).as_millis()))
        })
        .await;
    Ok(())
}
