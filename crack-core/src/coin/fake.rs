//! A scripted [`CoinChain`] for tests: no network, and every call recorded in order.
//!
//! Scripts are driven by call count, never by wall time, so a test's outcome does not depend on
//! how fast the machine is. A script repeats its last entry once the rest is used up.

use super::chain::{ChainError, ChainStatus, CoinChain, Signed};
use crate::db::economy::coin::Recorded;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const MINT: &str = "FakeMint111111111111111111111111111111111111";
pub const AUTHORITY: &str = "FakeAuthority1111111111111111111111111111111";
/// What the default `prepare` signs.
pub const SIG: &str = "fake-signature-1";
pub const TX: &[u8] = &[7, 7, 7, 1];
pub const LAST_VALID: u64 = 1000;
pub const GENESIS: &str = "fake-genesis-a";

/// One call the flow made, in the order it made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Height,
    Status(String),
    Genesis,
    Tap,
    HasAccount(String),
    Prepare { owner: String, amount: u64 },
    Send(Vec<u8>),
}

/// Holds one `status` call until the test releases it: `reached` fires when the call arrives.
pub struct Gate {
    pub reached: Arc<Notify>,
    pub release: Arc<Notify>,
}

type Scripted<T> = Result<T, String>;

struct Inner {
    prepare: Scripted<Signed>,
    statuses: VecDeque<Scripted<ChainStatus>>,
    by_signature: HashMap<String, VecDeque<Scripted<ChainStatus>>>,
    heights: VecDeque<Scripted<u64>>,
    genesis: Scripted<String>,
    tap: Scripted<u64>,
    has_account: bool,
    send: Scripted<()>,
    calls: Vec<Call>,
    status_calls: usize,
    /// Hold the n-th `status` call (1-based).
    gate: Option<(usize, Arc<Notify>, Arc<Notify>)>,
}

pub struct FakeChain {
    inner: Mutex<Inner>,
}

/// The next entry, repeating the last one forever.
fn next<T: Clone>(q: &mut VecDeque<T>) -> Option<T> {
    if q.len() > 1 {
        q.pop_front()
    } else {
        q.front().cloned()
    }
}

fn err(e: String) -> ChainError {
    ChainError(e)
}

pub fn signed(signature: &str, tx: &[u8], last_valid: u64, genesis: &str) -> Signed {
    Signed {
        signature: signature.to_owned(),
        tx: tx.to_vec(),
        last_valid_block_height: last_valid,
        genesis: genesis.to_owned(),
    }
}

impl FakeChain {
    /// `prepare` signs [`SIG`]/[`TX`], valid to [`LAST_VALID`] on [`GENESIS`]; the chain is at
    /// height 900 on [`GENESIS`] and has never seen anything.
    pub fn new() -> FakeChain {
        FakeChain {
            inner: Mutex::new(Inner {
                prepare: Ok(signed(SIG, TX, LAST_VALID, GENESIS)),
                statuses: VecDeque::from([Ok(ChainStatus::NotFound)]),
                by_signature: HashMap::new(),
                heights: VecDeque::from([Ok(900)]),
                genesis: Ok(GENESIS.to_owned()),
                tap: Ok(1_000_000_000),
                has_account: true,
                send: Ok(()),
                calls: Vec::new(),
                status_calls: 0,
                gate: None,
            }),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().expect("fake chain lock"))
    }

    pub fn set_prepare(&self, result: Result<Signed, &str>) {
        self.with(|i| i.prepare = result.map_err(str::to_owned));
    }

    /// The status script for every signature without its own.
    pub fn script_statuses(&self, script: Vec<Result<ChainStatus, &str>>) {
        assert!(!script.is_empty(), "a script needs an entry");
        self.with(|i| {
            i.statuses = script
                .into_iter()
                .map(|r| r.map_err(str::to_owned))
                .collect()
        });
    }

    pub fn script_status_for(&self, signature: &str, script: Vec<Result<ChainStatus, &str>>) {
        assert!(!script.is_empty(), "a script needs an entry");
        self.with(|i| {
            i.by_signature.insert(
                signature.to_owned(),
                script
                    .into_iter()
                    .map(|r| r.map_err(str::to_owned))
                    .collect(),
            )
        });
    }

    pub fn script_heights(&self, script: Vec<Result<u64, &str>>) {
        assert!(!script.is_empty(), "a script needs an entry");
        self.with(|i| {
            i.heights = script
                .into_iter()
                .map(|r| r.map_err(str::to_owned))
                .collect()
        });
    }

    pub fn set_genesis(&self, genesis: Result<&str, &str>) {
        self.with(|i| i.genesis = genesis.map(str::to_owned).map_err(str::to_owned));
    }

    pub fn set_tap(&self, tap: Result<u64, &str>) {
        self.with(|i| i.tap = tap.map_err(str::to_owned));
    }

    pub fn set_has_account(&self, has: bool) {
        self.with(|i| i.has_account = has);
    }

    /// What every `send` returns. A failed send is still recorded as a send.
    pub fn set_send(&self, result: Result<(), &str>) {
        self.with(|i| i.send = result.map_err(str::to_owned));
    }

    /// Hold the `n`-th `status` call (1-based) until `release` is notified.
    pub fn gate_status_call(&self, n: usize) -> Gate {
        let reached = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.with(|i| i.gate = Some((n, reached.clone(), release.clone())));
        Gate { reached, release }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.with(|i| i.calls.clone())
    }

    /// The bytes of every send, in order.
    pub fn sends(&self) -> Vec<Vec<u8>> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Send(tx) => Some(tx),
                _ => None,
            })
            .collect()
    }

    /// Every `prepare` as (owner, amount), in order.
    pub fn prepares(&self) -> Vec<(String, u64)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Prepare { owner, amount } => Some((owner, amount)),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl CoinChain for FakeChain {
    fn mint(&self) -> &str {
        MINT
    }

    fn authority(&self) -> String {
        AUTHORITY.to_owned()
    }

    async fn genesis(&self) -> Result<String, ChainError> {
        self.with(|i| {
            i.calls.push(Call::Genesis);
            i.genesis.clone().map_err(err)
        })
    }

    async fn finalized_height(&self) -> Result<u64, ChainError> {
        self.with(|i| {
            i.calls.push(Call::Height);
            next(&mut i.heights).expect("a height script").map_err(err)
        })
    }

    async fn tap_lamports(&self) -> Result<u64, ChainError> {
        self.with(|i| {
            i.calls.push(Call::Tap);
            i.tap.clone().map_err(err)
        })
    }

    async fn has_token_account(&self, owner: &str) -> Result<bool, ChainError> {
        self.with(|i| {
            i.calls.push(Call::HasAccount(owner.to_owned()));
            Ok(i.has_account)
        })
    }

    async fn prepare(&self, owner: &str, amount: u64) -> Result<Signed, ChainError> {
        self.with(|i| {
            i.calls.push(Call::Prepare {
                owner: owner.to_owned(),
                amount,
            });
            i.prepare.clone().map_err(err)
        })
    }

    async fn send(&self, tx: &Recorded) -> Result<(), ChainError> {
        self.with(|i| {
            i.calls.push(Call::Send(tx.tx().to_vec()));
            i.send.clone().map_err(err)
        })
    }

    async fn status(&self, signature: &str) -> Result<ChainStatus, ChainError> {
        let held = self.with(|i| {
            i.calls.push(Call::Status(signature.to_owned()));
            i.status_calls += 1;
            match &i.gate {
                Some((n, reached, release)) if *n == i.status_calls => {
                    Some((reached.clone(), release.clone()))
                },
                _ => None,
            }
        });
        if let Some((reached, release)) = held {
            reached.notify_one();
            release.notified().await;
        }
        self.with(|i| {
            let script = i.by_signature.get_mut(signature).unwrap_or(&mut i.statuses);
            next(script).expect("a status script").map_err(err)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_plays_its_scripts_and_repeats_the_last_entry() {
        let fake = FakeChain::new();
        fake.script_heights(vec![Ok(1), Err("down"), Ok(3)]);
        fake.script_statuses(vec![Ok(ChainStatus::Processed), Ok(ChainStatus::Landed)]);
        fake.script_status_for("other", vec![Err("429")]);
        fake.set_tap(Ok(42));
        fake.set_has_account(false);
        fake.set_genesis(Err("gone"));
        let heights = [
            fake.finalized_height().await.ok(),
            fake.finalized_height().await.ok(),
            fake.finalized_height().await.ok(),
            fake.finalized_height().await.ok(),
        ];
        assert_eq!(heights, [Some(1), None, Some(3), Some(3)]);
        assert_eq!(fake.status("s").await.ok(), Some(ChainStatus::Processed));
        assert_eq!(fake.status("s").await.ok(), Some(ChainStatus::Landed));
        assert_eq!(fake.status("s").await.ok(), Some(ChainStatus::Landed));
        assert_eq!(
            fake.status("other").await.map_err(|e| e.0).err().as_deref(),
            Some("429")
        );
        assert_eq!(fake.tap_lamports().await.ok(), Some(42));
        assert_eq!(fake.has_token_account("o").await.ok(), Some(false));
        assert!(fake.genesis().await.is_err());
        assert_eq!(fake.calls()[0], Call::Height);
        assert_eq!(fake.calls().last(), Some(&Call::Genesis));
    }
}
