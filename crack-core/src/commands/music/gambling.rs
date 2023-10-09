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

/// Roll N DM dice.
#[poise::command(prefix_command, slash_command)]
pub async fn rolldice(
    ctx: Context<'_>,
    #[description = "Number of dice to roll"] number_of_dice: u32,
    #[description = "Number of sides per die"] sides_per_die: u32,
) -> Result<(), Error> {
    let mut res: Vec<u32> = Vec::with_capacity(number_of_dice as usize);
    for _ in 0..number_of_dice {
        let r = rand::random::<u32>() % sides_per_die;
        res.push(r);
    }

    ctx.send(|m| {
        m.content(format!(
            "You roll {}, {} sided dice. Here are the results.\n{}",
            number_of_dice,
            sides_per_die,
            res.iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>()
                .join(", ")
        ))
    })
    .await?;

    Ok(())
}
