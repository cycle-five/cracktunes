//! One variant per distinct failure. They do not collapse: "this seed is not a
//! real track" is permanent, "we are rate limited" is temporary, and "the key
//! is bad" needs an operator. A caller that cannot tell them apart retries the
//! unretryable and gives up on the recoverable.

use thiserror::Error as ThisError;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// The provider says this seed is not a released track. Permanent for this
    /// seed; negative-cache it rather than retrying.
    #[error("`{artist} - {title}` is not a track {provider} knows: {message}")]
    NotATrack {
        provider: &'static str,
        artist: String,
        title: String,
        message: String,
    },

    /// Credential rejected. An operator must act; do not retry.
    #[error("{provider} rejected the credential: {message}")]
    InvalidKey {
        provider: &'static str,
        message: String,
    },

    /// Rate limited. `retry_after` is the provider's own figure when it gave one.
    #[error("{provider} rate limited us{}", .retry_after.map(|d| format!(", retry after {}s", d.as_secs())).unwrap_or_default())]
    RateLimited {
        provider: &'static str,
        retry_after: Option<std::time::Duration>,
    },

    /// The locally-counted daily budget for a metered provider is spent.
    #[error("{provider} daily budget of {budget} calls is spent")]
    BudgetExhausted { provider: &'static str, budget: u32 },

    #[error("{provider} transport error: {source}")]
    Transport {
        provider: &'static str,
        #[source]
        source: reqwest::Error,
    },

    #[error("{provider} sent a body this client could not read: {message}")]
    UnexpectedBody {
        provider: &'static str,
        message: String,
    },

    #[error("configuration: {0}")]
    Config(String),
}

impl Error {
    /// Whether trying the SAME provider again could plausibly succeed.
    ///
    /// 🪤 Deliberately NOT a general `is_retryable()` covering every variant —
    /// it answers one narrow question the orchestrator asks. `NotATrack` is
    /// false: the seed is wrong, not the moment.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Error::Transport { .. } | Error::RateLimited { .. })
    }
}
