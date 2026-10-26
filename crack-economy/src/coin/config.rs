//! Reading the CrackCoin settings from the environment.
//!
//! Nothing here ever prints a value: every refusal names a variable or a reason, never what was
//! in it.

use std::fmt;

use crack_coin::{Keypair, Pubkey, DEFAULT_RPC_URL};

/// The signing key, in the Solana CLI JSON format.
pub const KEYPAIR_VAR: &str = "CRACK_COIN_KEYPAIR";
/// The mint address, base58.
pub const MINT_VAR: &str = "CRACK_COIN_MINT";
/// The RPC endpoint; optional.
pub const URL_VAR: &str = "CRACK_COIN_RPC_URL";

/// Everything needed to open the coin. `Keypair`'s own `Debug` shows only its public half.
#[derive(Debug)]
pub struct CoinConfig {
    pub keypair: Keypair,
    pub mint: Pubkey,
    pub url: String,
}

/// Why the coin is off. Displays a reason, never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoinOff {
    NotConfigured(&'static str),
    BadKeypair,
    BadMint,
    Mainnet,
    MintMismatch(&'static str),
    NoDatabase,
}

impl fmt::Display for CoinOff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoinOff::NotConfigured(var) => write!(f, "{var} is not set"),
            CoinOff::BadKeypair => f.write_str("the keypair does not parse"),
            CoinOff::BadMint => f.write_str("the mint does not parse"),
            CoinOff::Mainnet => f.write_str("the RPC endpoint is mainnet"),
            CoinOff::MintMismatch(why) => write!(f, "the mint does not match: {why}"),
            CoinOff::NoDatabase => f.write_str("there is no database"),
        }
    }
}

/// `None` for absent, empty or whitespace-only.
fn present(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.trim().is_empty())
}

impl CoinConfig {
    pub fn from_values(
        keypair: Option<&str>,
        mint: Option<&str>,
        url: Option<&str>,
    ) -> Result<CoinConfig, CoinOff> {
        let keypair = present(keypair).ok_or(CoinOff::NotConfigured(KEYPAIR_VAR))?;
        let mint = present(mint).ok_or(CoinOff::NotConfigured(MINT_VAR))?;
        let keypair = Keypair::from_json(keypair).map_err(|_| CoinOff::BadKeypair)?;
        let mint = mint
            .trim()
            .parse::<Pubkey>()
            .map_err(|_| CoinOff::BadMint)?;
        let url = present(url).map_or(DEFAULT_RPC_URL, str::trim).to_owned();
        Ok(CoinConfig { keypair, mint, url })
    }

    pub fn from_env() -> Result<CoinConfig, CoinOff> {
        let get = |var: &str| std::env::var(var).ok();
        CoinConfig::from_values(
            get(KEYPAIR_VAR).as_deref(),
            get(MINT_VAR).as_deref(),
            get(URL_VAR).as_deref(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway test key in the Solana CLI format: seed then public key.
    fn keypair_json() -> String {
        let seed = [7u8; 32];
        let public = crack_coin::Keypair::from_seed(seed).pubkey().0;
        let all: Vec<u8> = seed.iter().chain(public.iter()).copied().collect();
        serde_json::to_string(&all).unwrap()
    }

    fn mint() -> String {
        crack_coin::Keypair::from_seed([9u8; 32])
            .pubkey()
            .to_string()
    }

    #[test]
    fn each_missing_variable_is_named() {
        let kp = keypair_json();
        assert_eq!(
            CoinConfig::from_values(None, Some(&mint()), None).err(),
            Some(CoinOff::NotConfigured("CRACK_COIN_KEYPAIR"))
        );
        assert_eq!(
            CoinConfig::from_values(Some(&kp), None, None).err(),
            Some(CoinOff::NotConfigured("CRACK_COIN_MINT"))
        );
        // Empty and whitespace-only count as unset.
        assert_eq!(
            CoinConfig::from_values(Some("  "), Some(&mint()), None).err(),
            Some(CoinOff::NotConfigured("CRACK_COIN_KEYPAIR"))
        );
        assert_eq!(
            CoinConfig::from_values(Some(&kp), Some(""), None).err(),
            Some(CoinOff::NotConfigured("CRACK_COIN_MINT"))
        );
    }

    #[test]
    fn the_url_is_optional_and_defaults() {
        let kp = keypair_json();
        let cfg = CoinConfig::from_values(Some(&kp), Some(&mint()), None).unwrap();
        assert_eq!(cfg.url, crack_coin::DEFAULT_RPC_URL);
        let cfg = CoinConfig::from_values(Some(&kp), Some(&mint()), Some("  ")).unwrap();
        assert_eq!(cfg.url, crack_coin::DEFAULT_RPC_URL);
        let cfg = CoinConfig::from_values(Some(&kp), Some(&mint()), Some("http://x:1")).unwrap();
        assert_eq!(cfg.url, "http://x:1");
    }

    #[test]
    fn a_keypair_with_a_trailing_newline_is_read() {
        let kp = format!("{}\n", keypair_json());
        let cfg = CoinConfig::from_values(Some(&kp), Some(&mint()), None).unwrap();
        assert_eq!(
            cfg.keypair.pubkey(),
            crack_coin::Keypair::from_seed([7u8; 32]).pubkey()
        );
        assert_eq!(cfg.mint.to_string(), mint());
    }

    #[test]
    fn a_bad_keypair_is_refused_without_echoing_it() {
        let junk = "[7777,7777]";
        let err = CoinConfig::from_values(Some(junk), Some(&mint()), None).unwrap_err();
        assert_eq!(err, CoinOff::BadKeypair);
        assert!(!err.to_string().contains("7777"));
        assert!(!format!("{err:?}").contains("7777"));
    }

    #[test]
    fn a_bad_mint_is_refused() {
        let kp = keypair_json();
        let err = CoinConfig::from_values(Some(&kp), Some("not-base58-0OIl"), None).unwrap_err();
        assert_eq!(err, CoinOff::BadMint);
        assert!(!err.to_string().contains("0OIl"));
    }

    /// Whether `shown` contains a run of the test seed (all sevens) in either the compact
    /// (`7,7,7`) or the derived-Debug (`7, 7, 7`) formatting.
    fn leaks_the_seed(shown: &str) -> bool {
        shown.contains("7,7,7,7") || shown.contains("7, 7, 7, 7")
    }

    #[test]
    fn the_leak_check_trips_on_either_formatting() {
        let seed = [7u8; 32];
        assert!(leaks_the_seed(&format!("{seed:?}")));
        assert!(leaks_the_seed(&serde_json::to_string(&seed).unwrap()));
        assert!(!leaks_the_seed("Keypair(abc)"));
    }

    #[test]
    fn the_debug_of_a_config_shows_no_key() {
        let kp = keypair_json();
        let cfg = CoinConfig::from_values(Some(&kp), Some(&mint()), None).unwrap();
        let shown = format!("{cfg:?}");
        assert!(!leaks_the_seed(&shown), "{shown}");
        let expected = format!(
            "CoinConfig {{ keypair: Keypair({}), mint: {}, url: {:?} }}",
            cfg.keypair.pubkey(),
            mint(),
            crack_coin::DEFAULT_RPC_URL
        );
        assert_eq!(shown, expected);
    }

    #[test]
    fn a_refusal_names_its_reason_and_no_value() {
        assert_eq!(
            CoinOff::NotConfigured("CRACK_COIN_MINT").to_string(),
            "CRACK_COIN_MINT is not set"
        );
        assert_eq!(
            CoinOff::BadKeypair.to_string(),
            "the keypair does not parse"
        );
        assert_eq!(CoinOff::BadMint.to_string(), "the mint does not parse");
        assert_eq!(CoinOff::Mainnet.to_string(), "the RPC endpoint is mainnet");
        assert_eq!(
            CoinOff::MintMismatch("decimals").to_string(),
            "the mint does not match: decimals"
        );
    }
}
