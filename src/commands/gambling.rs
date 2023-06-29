use crate::{Context, Error};

/// Flip a coin.
#[poise::command(prefix_command, slash_command)]
pub async fn coinflip(ctx: Context<'_>) -> Result<(), Error> {
    let res = rand::random::<bool>();

    ctx.send(|m| {
        m.content(format!(
            "You flipped a coin and it landed on {}!",
            if res { "heads" } else { "tails" }
        ))
    })
    .await?;

    Ok(())
}
