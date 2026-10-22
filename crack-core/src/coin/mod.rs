//! The CrackCoin devnet token: the seam between the bot and the chain.
pub mod chain;
pub mod config;
#[cfg(test)]
pub(crate) mod fake;
pub mod flow;
pub mod registry;
pub mod resolve;
pub mod sweep;

use crate::economy::effects::spawn_supervised;
use crate::Data;
use chain::{CoinChain, LiveChain, OpenError};
use config::{CoinConfig, CoinOff};
use registry::CoinState;
use std::sync::Arc;
use std::time::Duration;

/// The line TuneTitan's verify greps for.
pub fn ready_line(mint: &str, authority: &str, lamports: u64) -> String {
    // Hundredths of a SOL, rounded down: 1_234_000_000 lamports is 1.23.
    let hundredths = lamports / 10_000_000;
    format!(
        "crack-coin: ready (mint {mint}, authority {authority}, {}.{:02} devnet SOL)",
        hundredths / 100,
        hundredths % 100
    )
}

pub fn off_line(off: &CoinOff) -> String {
    format!("crack-coin: off ({off})")
}

/// 5 s, doubling, never more than 300 s.
pub fn startup_backoff(attempt: u32) -> Duration {
    const BASE: u64 = 5;
    const CAP: u64 = 300;
    let secs = 1u64
        .checked_shl(attempt)
        .and_then(|f| f.checked_mul(BASE))
        .unwrap_or(CAP);
    Duration::from_secs(secs.min(CAP))
}

/// What to do with the result of opening the chain.
enum Next {
    Retry(String),
    Off(CoinOff),
    Ready(Box<LiveChain>),
}

fn after_open(result: Result<LiveChain, OpenError>) -> Next {
    match result {
        Ok(chain) => Next::Ready(Box::new(chain)),
        Err(OpenError::Unreachable(why)) => Next::Retry(why),
        Err(OpenError::Refused(off)) => Next::Off(off),
    }
}

/// Decide whether CrackCoin is ready or off, say so once, and start the sweeper.
pub fn start(data: Data) {
    if !data.economy.coin.start_once() {
        return;
    }
    let Some(pool) = data.database_pool.clone() else {
        return;
    };
    spawn_supervised("coin-start", async move {
        let mut attempt: u32 = 0;
        loop {
            // `CoinConfig` is not `Clone` (it holds the key), so each attempt reads it afresh.
            let cfg = match CoinConfig::from_env() {
                Ok(cfg) => cfg,
                Err(off) => {
                    data.economy.coin.set(CoinState::Off(off.clone()));
                    tracing::info!("{}", off_line(&off));
                    return;
                },
            };
            match after_open(LiveChain::open(cfg).await) {
                Next::Retry(why) => {
                    let wait = startup_backoff(attempt);
                    tracing::warn!(
                        "crack-coin: devnet unreachable (attempt {}, retrying in {}s): {why}",
                        attempt + 1,
                        wait.as_secs()
                    );
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(wait).await;
                },
                Next::Off(off) => {
                    data.economy.coin.set(CoinState::Off(off.clone()));
                    if off == CoinOff::Mainnet {
                        tracing::error!("{}", off_line(&off));
                    } else {
                        tracing::warn!("{}", off_line(&off));
                    }
                    return;
                },
                Next::Ready(chain) => {
                    let chain: Arc<dyn CoinChain> = Arc::new(*chain);
                    let lamports = chain.tap_lamports().await.unwrap_or(0);
                    data.economy.coin.set(CoinState::Ready(chain.clone()));
                    tracing::info!("{}", ready_line(chain.mint(), &chain.authority(), lamports));
                    sweep::start_sweeper(pool, chain, data.economy.process_start());
                    return;
                },
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::registry::{CoinRegistry, CoinState};
    use super::*;

    #[test]
    fn ready_line_is_exact() {
        assert_eq!(
            ready_line("M", "A", 1_234_000_000),
            "crack-coin: ready (mint M, authority A, 1.23 devnet SOL)"
        );
        assert_eq!(
            ready_line("M", "A", 0),
            "crack-coin: ready (mint M, authority A, 0.00 devnet SOL)"
        );
        // Hundredths round down, and a big balance does not overflow.
        assert!(ready_line("M", "A", 1_239_999_999).ends_with(", 1.23 devnet SOL)"));
        assert!(ready_line("M", "A", u64::MAX).ends_with(", 18446744073.70 devnet SOL)"));
        assert!(ready_line("M", "A", 9_999_999).ends_with(", 0.00 devnet SOL)"));
    }

    #[test]
    fn off_line_names_the_reason() {
        assert_eq!(
            off_line(&CoinOff::NotConfigured("CRACK_COIN_MINT")),
            "crack-coin: off (CRACK_COIN_MINT is not set)"
        );
        assert_eq!(
            off_line(&CoinOff::Mainnet),
            "crack-coin: off (the RPC endpoint is mainnet)"
        );
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let secs: Vec<u64> = (0..9).map(|n| startup_backoff(n).as_secs()).collect();
        assert_eq!(secs, [5, 10, 20, 40, 80, 160, 300, 300, 300]);
        assert_eq!(startup_backoff(u32::MAX).as_secs(), 300);
        assert_eq!(startup_backoff(63).as_secs(), 300);
    }

    #[test]
    fn the_state_starts_as_starting_and_start_runs_once() {
        let reg = CoinRegistry::default();
        assert!(matches!(reg.state(), CoinState::Starting));
        assert!(reg.start_once());
        assert!(!reg.start_once());
        reg.set(CoinState::Off(CoinOff::Mainnet));
        assert!(matches!(reg.state(), CoinState::Off(CoinOff::Mainnet)));
        // A clone shares the state.
        let other = reg.clone();
        other.set(CoinState::Starting);
        assert!(matches!(reg.state(), CoinState::Starting));
        assert!(!other.start_once());
    }
}
