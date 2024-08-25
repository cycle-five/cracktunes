use crate::messaging::message::CrackedMessage;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::{Context, Error};

/// Flip a coin.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Games", prefix_command, slash_command)]
pub async fn coinflip(ctx: Context<'_>) -> Result<(), Error> {
    let res = rand::random::<bool>();

    ctx.send_reply(CrackedMessage::Coinflip(res), true).await?;

    Ok(())
}

/// Roll N dice, each with D sides.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Games", prefix_command, slash_command)]
pub async fn rolldice(
    ctx: Context<'_>,
    #[description = "Number of dice to roll."] number_of_dice: u32,
    #[description = "Number of sides per die."] sides_per_die: u32,
) -> Result<(), Error> {
    rolldice_internal(ctx, number_of_dice, sides_per_die).await
}

// Role N D sided dice.
#[cfg(not(tarpaulin_include))]
pub async fn rolldice_internal(
    ctx: Context<'_>,
    number_of_dice: u32,
    sides_per_die: u32,
) -> Result<(), Error> {
    let res = roll_n_d(number_of_dice, sides_per_die);
    let msg = CrackedMessage::DiceRoll {
        dice: number_of_dice,
        sides: sides_per_die,
        results: res,
    };
    ctx.send_reply(msg, true).await?;
    Ok(())
}

/// Roll N D sided dice.
pub fn roll_n_d(number_of_dice: u32, sides_per_die: u32) -> Vec<u32> {
    let mut res: Vec<u32> = Vec::with_capacity(number_of_dice as usize);
    for _ in 0..number_of_dice {
        let r = rand::random::<u32>() % sides_per_die;
        res.push(r);
    }

    res
}

mod tests {
    #[test]
    fn test_roll_n_d() {
        let res = crate::commands::roll_n_d(3, 6);
        assert_eq!(res.len(), 3);
        for r in res {
            assert!(r < 6);
        }
    }
}
