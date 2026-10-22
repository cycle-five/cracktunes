//! Where the coin stands: starting, off (with the reason) or ready (with the chain).

use super::chain::CoinChain;
use super::config::CoinOff;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub enum CoinState {
    Starting,
    Off(CoinOff),
    Ready(Arc<dyn CoinChain>),
}

impl fmt::Debug for CoinState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoinState::Starting => f.write_str("Starting"),
            CoinState::Off(off) => write!(f, "Off({off:?})"),
            CoinState::Ready(_) => f.write_str("Ready(..)"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoinRegistry {
    state: Arc<RwLock<CoinState>>,
    started: Arc<AtomicBool>,
}

impl Default for CoinRegistry {
    fn default() -> Self {
        CoinRegistry {
            state: Arc::new(RwLock::new(CoinState::Starting)),
            started: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl CoinRegistry {
    pub fn state(&self) -> CoinState {
        match self.state.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn set(&self, s: CoinState) {
        match self.state.write() {
            Ok(mut guard) => *guard = s,
            Err(poisoned) => *poisoned.into_inner() = s,
        }
    }

    /// True for the first caller only.
    pub fn start_once(&self) -> bool {
        !self.started.swap(true, Ordering::SeqCst)
    }
}
