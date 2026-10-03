//! What a server's plan allows. Premium is `guild_settings.premium`.
//! Spec: docs/superpowers/specs/2026-10-03-premium-history-window-design.md

use crate::messaging::message::CrackedMessage;
use chrono::{DateTime, Duration, Utc};

/// How far back a free server's queue history goes.
pub const FREE_HISTORY_WINDOW: Duration = Duration::hours(24);

/// A server's plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Premium,
}

impl Plan {
    /// The plan for a server's premium setting. Unknown settings (`None`, such
    /// as before they have loaded) count as Free: the limit fails closed.
    #[must_use]
    pub fn of(premium: Option<bool>) -> Plan {
        if premium == Some(true) {
            Plan::Premium
        } else {
            Plan::Free
        }
    }

    /// The oldest moment of queue history this plan shows, or `None` for all of it.
    #[must_use]
    pub fn history_floor(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Plan::Free => Some(now - FREE_HISTORY_WINDOW),
            Plan::Premium => None,
        }
    }

    /// The Patreon message for this plan: the plug for Free, thanks for Premium.
    #[must_use]
    pub fn plug(self) -> CrackedMessage {
        match self {
            Plan::Free => CrackedMessage::PremiumPlug,
            Plan::Premium => CrackedMessage::PremiumThanks,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn only_a_known_true_setting_is_premium() {
        assert_eq!(Plan::of(Some(true)), Plan::Premium);
        assert_eq!(Plan::of(Some(false)), Plan::Free);
        assert_eq!(Plan::of(None), Plan::Free);
    }

    #[test]
    fn free_sees_24_hours_and_premium_sees_everything() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        assert_eq!(
            Plan::Free.history_floor(now),
            Some(now - Duration::hours(24))
        );
        assert_eq!(Plan::Premium.history_floor(now), None);
    }

    #[test]
    fn free_gets_the_plug_and_premium_gets_thanks() {
        assert!(matches!(Plan::Free.plug(), CrackedMessage::PremiumPlug));
        assert!(matches!(
            Plan::Premium.plug(),
            CrackedMessage::PremiumThanks
        ));
    }
}
