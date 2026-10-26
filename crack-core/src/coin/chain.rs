//! The seam between the bot and the chain. Everything the withdrawal flow needs from Solana is a
//! method on [`CoinChain`], so tests can substitute a fake.

use async_trait::async_trait;
use crack_coin::{Coin, Pubkey, Signature};

use super::config::{CoinConfig, CoinOff};
use crate::db::economy::coin::Recorded;

/// What the chain says about a signature. Mirrors `crack_coin::TxStatus` one to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    Landed,
    Failed(String),
    Processed,
    NotFound,
}

impl From<crack_coin::TxStatus> for ChainStatus {
    fn from(status: crack_coin::TxStatus) -> Self {
        match status {
            crack_coin::TxStatus::Landed => ChainStatus::Landed,
            crack_coin::TxStatus::Failed(why) => ChainStatus::Failed(why),
            crack_coin::TxStatus::Processed => ChainStatus::Processed,
            crack_coin::TxStatus::NotFound => ChainStatus::NotFound,
        }
    }
}

/// A chain call failed. The text comes from `crack_coin::Error`'s `Display`, which never
/// contains signing-key material. It can carry third-party text (an HTTP client message that
/// names the RPC URL, or a server's error message), so treat it as loggable, not as public.
#[derive(Debug)]
pub struct ChainError(pub String);

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ChainError {}

impl From<crack_coin::Error> for ChainError {
    fn from(e: crack_coin::Error) -> Self {
        ChainError(e.to_string())
    }
}

/// A mint transaction, signed and ready to record then send. All strings are base58.
#[derive(Debug, Clone)]
pub struct Signed {
    pub signature: String,
    pub tx: Vec<u8>,
    pub last_valid_block_height: u64,
    pub genesis: String,
}

#[async_trait]
pub trait CoinChain: Send + Sync {
    fn mint(&self) -> &str;
    fn authority(&self) -> String;
    /// The cluster's genesis hash, fetched fresh.
    async fn genesis(&self) -> Result<String, ChainError>;
    /// The finalized block height. Callers must read this BEFORE [`CoinChain::status`]
    /// (Ruling R9): a height read after the status could pass a transaction that landed in
    /// between, and expiry would then be wrongly proven.
    async fn finalized_height(&self) -> Result<u64, ChainError>;
    /// The authority's lamports, which pay fees and rent.
    async fn tap_lamports(&self) -> Result<u64, ChainError>;
    async fn has_token_account(&self, owner: &str) -> Result<bool, ChainError>;
    /// Signs a mint of `amount` to `owner` against a fresh genesis and blockhash.
    async fn prepare(&self, owner: &str, amount: u64) -> Result<Signed, ChainError>;
    /// Sends the recorded bytes. Only a successful `record_sent` makes a [`Recorded`].
    async fn send(&self, tx: &Recorded) -> Result<(), ChainError>;
    /// Where `signature` stands. Call [`CoinChain::finalized_height`] first (Ruling R9).
    async fn status(&self, signature: &str) -> Result<ChainStatus, ChainError>;
}

/// Why [`LiveChain::open`] failed.
#[derive(Debug)]
pub enum OpenError {
    /// The endpoint could not be reached; worth retrying.
    Unreachable(String),
    /// The configuration is wrong; retrying will not help.
    Refused(CoinOff),
}

impl From<crack_coin::Error> for OpenError {
    fn from(e: crack_coin::Error) -> Self {
        match e {
            crack_coin::Error::Mainnet => OpenError::Refused(CoinOff::Mainnet),
            crack_coin::Error::MintMismatch(why) => OpenError::Refused(CoinOff::MintMismatch(why)),
            other => OpenError::Unreachable(other.to_string()),
        }
    }
}

/// The real chain: devnet through `crack_coin`.
pub struct LiveChain {
    coin: Coin,
    mint: String,
}

impl LiveChain {
    pub async fn open(cfg: CoinConfig) -> Result<LiveChain, OpenError> {
        let coin = Coin::open(&cfg.url, cfg.keypair, cfg.mint).await?;
        let mint = coin.mint().to_string();
        Ok(LiveChain { coin, mint })
    }
}

fn parse<T: std::str::FromStr<Err = crack_coin::Error>>(text: &str) -> Result<T, ChainError> {
    text.parse::<T>().map_err(ChainError::from)
}

#[async_trait]
impl CoinChain for LiveChain {
    fn mint(&self) -> &str {
        &self.mint
    }

    fn authority(&self) -> String {
        self.coin.authority().to_string()
    }

    async fn genesis(&self) -> Result<String, ChainError> {
        Ok(self.coin.devnet().genesis_hash().await?.to_string())
    }

    async fn finalized_height(&self) -> Result<u64, ChainError> {
        Ok(self.coin.devnet().finalized_block_height().await?)
    }

    async fn tap_lamports(&self) -> Result<u64, ChainError> {
        Ok(self.coin.devnet().balance(&self.coin.authority()).await?)
    }

    async fn has_token_account(&self, owner: &str) -> Result<bool, ChainError> {
        let owner: Pubkey = parse(owner)?;
        Ok(self.coin.has_token_account(&owner).await?)
    }

    async fn prepare(&self, owner: &str, amount: u64) -> Result<Signed, ChainError> {
        let owner: Pubkey = parse(owner)?;
        let genesis = self.coin.devnet().genesis_hash().await?;
        let prepared = self.coin.prepare_mint(&owner, amount).await?;
        Ok(Signed {
            signature: prepared.signature.to_string(),
            tx: prepared.wire,
            last_valid_block_height: prepared.last_valid_block_height,
            genesis: genesis.to_string(),
        })
    }

    async fn send(&self, tx: &Recorded) -> Result<(), ChainError> {
        self.coin.devnet().send(tx.tx()).await?;
        Ok(())
    }

    async fn status(&self, signature: &str) -> Result<ChainStatus, ChainError> {
        let signature: Signature = parse(signature)?;
        Ok(self.coin.devnet().status(&signature).await?.into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crack_coin::TxStatus;

    #[test]
    fn statuses_map_one_to_one() {
        assert_eq!(ChainStatus::from(TxStatus::Landed), ChainStatus::Landed);
        assert_eq!(
            ChainStatus::from(TxStatus::Failed("boom".into())),
            ChainStatus::Failed("boom".into())
        );
        assert_eq!(
            ChainStatus::from(TxStatus::Processed),
            ChainStatus::Processed
        );
        assert_eq!(ChainStatus::from(TxStatus::NotFound), ChainStatus::NotFound);
    }

    #[test]
    fn open_errors_map_to_refusals_or_unreachable() {
        use crack_coin::Error;
        assert!(matches!(
            OpenError::from(Error::Mainnet),
            OpenError::Refused(CoinOff::Mainnet)
        ));
        assert!(matches!(
            OpenError::from(Error::MintMismatch("decimals")),
            OpenError::Refused(CoinOff::MintMismatch("decimals"))
        ));
        assert!(matches!(
            OpenError::from(Error::Http("down".into())),
            OpenError::Unreachable(_)
        ));
        assert!(matches!(
            OpenError::from(Error::Rpc {
                code: 429,
                message: "slow".into()
            }),
            OpenError::Unreachable(_)
        ));
    }

    #[test]
    fn a_chain_error_carries_the_crate_error_text() {
        let e = ChainError::from(crack_coin::Error::Http("timed out".into()));
        assert_eq!(e.0, "http error: timed out");
    }
}
