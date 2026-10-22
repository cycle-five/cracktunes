//! The CrackCoin devnet token: the seam between the bot and the chain.
pub mod chain;
pub mod config;
#[cfg(test)]
pub(crate) mod fake;
pub mod flow;
pub mod resolve;
pub mod sweep;
