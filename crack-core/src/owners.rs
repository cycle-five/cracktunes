//! Who counts as a bot owner, for the bot's commands and for the dashboard alike.
//!
//! Poise's `owners_only` checks the framework's owner set: the configured
//! owners (or [`DEFAULT_OWNER`] when none are configured), plus, with
//! `initialize_owners`, the application's owner and team, fetched from Discord
//! at startup. The dashboard has no framework, so [`bot_owners`] builds the
//! same set the same way. One function for the configured part means the two
//! can't drift.

use crate::BotConfig;
use poise::serenity_prelude::{Http, UserId};
use std::collections::HashSet;

/// The owner when the config names none.
pub const DEFAULT_OWNER: u64 = 285219649921220608;

/// The owners the config names, or [`DEFAULT_OWNER`] when it names none.
#[must_use]
pub fn configured_owners(config: &BotConfig) -> HashSet<UserId> {
    config
        .owners
        .as_deref()
        .unwrap_or(&[DEFAULT_OWNER])
        .iter()
        .map(|id| UserId::new(*id))
        .collect()
}

/// Every bot owner, as poise's `owners_only` sees them: the configured owners
/// plus the application's owner and team. If Discord doesn't answer, the
/// configured owners alone.
pub async fn bot_owners(config: &BotConfig, http: &Http) -> HashSet<UserId> {
    let mut owners = configured_owners(config);
    if let Err(e) = poise::framework::insert_owners_from_http(http, &mut owners, &None).await {
        tracing::warn!("bot owners: the application's owners could not be read: {e}");
    }
    owners
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn no_configured_owners_means_the_default_owner() {
        let config = BotConfig {
            owners: None,
            ..Default::default()
        };
        assert_eq!(
            configured_owners(&config),
            HashSet::from([UserId::new(DEFAULT_OWNER)])
        );
    }

    #[test]
    fn configured_owners_replace_the_default() {
        let config = BotConfig {
            owners: Some(vec![7, 8]),
            ..Default::default()
        };
        assert_eq!(
            configured_owners(&config),
            HashSet::from([UserId::new(7), UserId::new(8)])
        );
    }
}
