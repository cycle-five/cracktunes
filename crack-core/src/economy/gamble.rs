//! The coinflip's rules, apart from Discord and the database: which bets are
//! refused, how the coin falls, and what is said about it.

use crate::messaging::messages::{
    ECONOMY_BET_NON_POSITIVE, ECONOMY_BET_OVER_MAX_PREFIX, ECONOMY_COIN_BALANCE,
    ECONOMY_COIN_LANDED, ECONOMY_COIN_LOST, ECONOMY_COIN_WON, ECONOMY_COIN_YOU_CALLED,
    ECONOMY_GAMBLER_OPTED_OUT, ECONOMY_GAMBLING_OFF, ECONOMY_HEADS, ECONOMY_TAILS,
    ECONOMY_YOU_HAVE,
};
use rand::{Rng, RngExt};

/// A side of the coin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinSide {
    Heads,
    Tails,
}

impl CoinSide {
    fn label(self) -> &'static str {
        match self {
            CoinSide::Heads => ECONOMY_HEADS,
            CoinSide::Tails => ECONOMY_TAILS,
        }
    }
}

/// Why a bet was refused before any credits moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetRefusal {
    GamblingOff,
    OptedOut,
    NonPositive,
    OverMax { max: u32 },
    CantAfford { balance: i64 },
}

/// Check a bet, refusing for the first reason that applies, in the enum's
/// order: gambling off, opted out, not positive, over the max, can't afford.
pub fn check_bet(
    gambling: bool,
    opted_out: bool,
    bet: i64,
    max_bet: u32,
    balance: i64,
) -> Result<(), BetRefusal> {
    if !gambling {
        return Err(BetRefusal::GamblingOff);
    }
    if opted_out {
        return Err(BetRefusal::OptedOut);
    }
    if bet <= 0 {
        return Err(BetRefusal::NonPositive);
    }
    if bet > i64::from(max_bet) {
        return Err(BetRefusal::OverMax { max: max_bet });
    }
    if balance < bet {
        return Err(BetRefusal::CantAfford { balance });
    }
    Ok(())
}

/// The words for a refusal, naming the server's currency.
pub fn bet_refusal_text(r: &BetRefusal, currency: &str, emoji: &str) -> String {
    match r {
        BetRefusal::GamblingOff => ECONOMY_GAMBLING_OFF.to_string(),
        BetRefusal::OptedOut => ECONOMY_GAMBLER_OPTED_OUT.to_string(),
        BetRefusal::NonPositive => ECONOMY_BET_NON_POSITIVE.to_string(),
        BetRefusal::OverMax { max } => {
            format!("{ECONOMY_BET_OVER_MAX_PREFIX} {max} {emoji} {currency}.")
        },
        BetRefusal::CantAfford { balance } => {
            format!("{ECONOMY_YOU_HAVE} {balance} {emoji} {currency}.")
        },
    }
}

/// Toss a fair coin.
pub fn flip(rng: &mut impl Rng) -> CoinSide {
    if rng.random_bool(0.5) {
        CoinSide::Heads
    } else {
        CoinSide::Tails
    }
}

/// What the flip said: the side, the call, the result, the new balance.
pub fn coinflip_text(
    call: CoinSide,
    landed: CoinSide,
    bet: i64,
    balance: i64,
    currency: &str,
    emoji: &str,
) -> String {
    let outcome = if call == landed {
        format!("{ECONOMY_COIN_WON} {bet} {currency}!")
    } else {
        format!("{ECONOMY_COIN_LOST} {bet} {currency}.")
    };
    format!(
        "{emoji} {ECONOMY_COIN_LANDED} **{}**. {ECONOMY_COIN_YOU_CALLED} **{}**. {outcome} \
         {ECONOMY_COIN_BALANCE} {balance} {currency}.",
        landed.label(),
        call.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn check_bet_order_and_edges() {
        assert_eq!(
            check_bet(true, false, 0, 500, 1000),
            Err(BetRefusal::NonPositive)
        );
        assert_eq!(
            check_bet(true, false, -3, 500, 1000),
            Err(BetRefusal::NonPositive)
        );
        assert_eq!(
            check_bet(true, false, 501, 500, 1000),
            Err(BetRefusal::OverMax { max: 500 })
        );
        assert_eq!(check_bet(true, false, 500, 500, 500), Ok(()));
        assert_eq!(
            check_bet(true, false, 500, 500, 499),
            Err(BetRefusal::CantAfford { balance: 499 })
        );
        // Each refusal beats the ones after it.
        assert_eq!(
            check_bet(false, true, 0, 500, 0),
            Err(BetRefusal::GamblingOff)
        );
        assert_eq!(check_bet(true, true, 0, 500, 0), Err(BetRefusal::OptedOut));
        assert_eq!(
            check_bet(true, false, 501, 500, 0),
            Err(BetRefusal::OverMax { max: 500 })
        );
    }

    #[test]
    fn flip_is_fair_enough() {
        let mut rng = StdRng::seed_from_u64(7);
        let heads = (0..10_000)
            .filter(|_| flip(&mut rng) == CoinSide::Heads)
            .count();
        assert!((4_800..=5_200).contains(&heads), "heads: {heads}");
    }

    #[test]
    fn coinflip_text_win_and_loss() {
        assert_eq!(
            coinflip_text(CoinSide::Heads, CoinSide::Heads, 50, 150, "credits", "🪙"),
            "🪙 The coin landed **Heads**. You called **Heads**. You win 50 credits! \
             Balance: 150 credits."
        );
        assert_eq!(
            coinflip_text(CoinSide::Heads, CoinSide::Tails, 50, 50, "credits", "🪙"),
            "🪙 The coin landed **Tails**. You called **Heads**. You lose 50 credits. \
             Balance: 50 credits."
        );
    }

    #[test]
    fn refusal_texts_name_the_currency_and_numbers() {
        assert_eq!(
            bet_refusal_text(&BetRefusal::OverMax { max: 500 }, "gold", "🥇"),
            "The most you can bet here is 500 🥇 gold."
        );
        assert_eq!(
            bet_refusal_text(&BetRefusal::CantAfford { balance: 499 }, "gold", "🥇"),
            "You have 499 🥇 gold."
        );
        assert_eq!(
            bet_refusal_text(&BetRefusal::GamblingOff, "gold", "🥇"),
            "Gambling is off in this server."
        );
    }
}
