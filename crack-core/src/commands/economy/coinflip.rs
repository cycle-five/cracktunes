//! `/coinflip <bet> <call>`: double or nothing, no house edge.
use super::credits::{gate, say};
use crate::db::economy as db;
use crate::economy::gamble::{
    bet_refusal_text, check_bet, coinflip_text, flip, BetRefusal, CoinSide,
};
use crate::economy::wallet::MoveError;
use crate::messaging::messages::ECONOMY_BET_FAILED;
use crate::{Context, Error};

/// Your call on the coin: the slash command's choice parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum CoinCall {
    Heads,
    Tails,
}

impl From<CoinCall> for CoinSide {
    fn from(call: CoinCall) -> Self {
        match call {
            CoinCall::Heads => CoinSide::Heads,
            CoinCall::Tails => CoinSide::Tails,
        }
    }
}

/// Bet credits on a coin toss: win and you get the stake back plus the same again.
#[poise::command(category = "Economy", slash_command, guild_only)]
pub async fn coinflip(
    ctx: Context<'_>,
    #[description = "How many to bet"]
    #[min = 1]
    #[max = 1000000]
    bet: i64,
    #[description = "Your call"] call: CoinCall,
) -> Result<(), Error> {
    let call = CoinSide::from(call);
    let Some((guild, pool)) = gate(ctx).await? else {
        return Ok(());
    };
    let user = ctx.author().id;
    let settings = ctx.data().economy.settings(guild);
    let cfg = &settings.config;
    let currency = cfg.currency_name.as_str();
    let balance = match db::balance(&pool, guild, user).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!("economy: balance of {user} in {guild} failed: {err}");
            return say(ctx, ECONOMY_BET_FAILED, true).await;
        },
    };
    if let Err(refusal) = check_bet(
        cfg.gambling_enabled,
        ctx.data().economy.is_opted_out(guild, user),
        bet,
        cfg.max_bet,
        balance,
    ) {
        return say(
            ctx,
            bet_refusal_text(&refusal, currency, &cfg.currency_emoji),
            true,
        )
        .await;
    }
    let landed = flip(&mut rand::rng());
    match db::settle_coinflip(&pool, guild, user, bet, landed == call).await {
        Ok(now) => {
            say(
                ctx,
                coinflip_text(call, landed, bet, now, currency, &cfg.currency_emoji),
                false,
            )
            .await
        },
        // Spent elsewhere between the read and the settle.
        Err(MoveError::Insufficient { balance }) => {
            say(
                ctx,
                bet_refusal_text(
                    &BetRefusal::CantAfford { balance },
                    currency,
                    &cfg.currency_emoji,
                ),
                true,
            )
            .await
        },
        Err(err) => {
            tracing::warn!("economy: coinflip of {user} in {guild} failed: {err:?}");
            say(ctx, ECONOMY_BET_FAILED, true).await
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coin_call_choices_are_unchanged() {
        let names: Vec<String> = <CoinCall as poise::ChoiceParameter>::list()
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        assert_eq!(names, ["Heads", "Tails"]);
    }
}
