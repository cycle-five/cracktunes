//! Immunity grants as stored. The rules that read them come with the logic.

use chrono::{DateTime, Utc};
use serenity::all::UserId;

/// Who gave the grant. A member has at most one of each per server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantSource {
    Bought,
    Mod,
}

impl GrantSource {
    pub fn as_str(self) -> &'static str {
        match self {
            GrantSource::Bought => "bought",
            GrantSource::Mod => "mod",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bought" => Some(GrantSource::Bought),
            "mod" => Some(GrantSource::Mod),
            _ => None,
        }
    }
}

/// One stored grant. `until: None` lasts until lifted (mod grants only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub source: GrantSource,
    pub until: Option<DateTime<Utc>>,
    pub granted_by: Option<UserId>,
    pub reason: Option<String>,
}

/// Why a member is out of reach of pranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    OptedOut,
    Until(DateTime<Utc>),
    Indefinitely,
}

/// Whether a member is protected now. Reads the stored opt-out flag and
/// grants only, never the server's three switches: those gate creating
/// protection, so turning one off leaves what exists in force.
pub fn is_protected(opted_out: bool, grants: &[Grant], now: DateTime<Utc>) -> Option<Protection> {
    if opted_out {
        return Some(Protection::OptedOut);
    }
    if grants.iter().any(|g| g.until.is_none()) {
        return Some(Protection::Indefinitely);
    }
    grants
        .iter()
        .filter_map(|g| g.until)
        .filter(|u| *u > now)
        .max()
        .map(Protection::Until)
}

/// Whether the member may switch their opt-out now. `Err` carries when
/// switching is next allowed.
pub fn can_switch_optout(
    changed_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    cooldown_days: u32,
) -> Result<(), DateTime<Utc>> {
    let Some(changed) = changed_at else {
        return Ok(());
    };
    let next = changed + chrono::Duration::days(i64::from(cooldown_days));
    if cooldown_days == 0 || now >= next {
        Ok(())
    } else {
        Err(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn grant(source: GrantSource, until: Option<DateTime<Utc>>) -> Grant {
        Grant {
            source,
            until,
            granted_by: None,
            reason: None,
        }
    }

    #[test]
    fn is_protected_matrix() {
        use Protection as P;
        let now = t0();
        let expired = grant(GrantSource::Bought, Some(now - Duration::hours(1)));
        let at_now = grant(GrantSource::Bought, Some(now));
        let active = grant(GrantSource::Bought, Some(now + Duration::hours(5)));
        let later_mod = grant(GrantSource::Mod, Some(now + Duration::hours(9)));
        let forever = grant(GrantSource::Mod, None);
        let cases: Vec<(&str, Vec<Grant>, Option<Protection>)> = vec![
            ("no grant", vec![], None),
            ("expired", vec![expired.clone()], None),
            ("ends exactly now", vec![at_now], None),
            (
                "active bought",
                vec![active.clone()],
                Some(P::Until(now + Duration::hours(5))),
            ),
            (
                "indefinite mod",
                vec![forever.clone()],
                Some(P::Indefinitely),
            ),
            (
                "bought and indefinite mod",
                vec![active.clone(), forever.clone()],
                Some(P::Indefinitely),
            ),
            (
                "expired and active",
                vec![expired, active.clone()],
                Some(P::Until(now + Duration::hours(5))),
            ),
            (
                "later of two finite wins",
                vec![active.clone(), later_mod.clone()],
                Some(P::Until(now + Duration::hours(9))),
            ),
            (
                "later wins in either order",
                vec![later_mod, active],
                Some(P::Until(now + Duration::hours(9))),
            ),
        ];
        for (name, grants, want) in cases {
            assert_eq!(is_protected(false, &grants, now), want, "in: {name}");
            assert_eq!(
                is_protected(true, &grants, now),
                Some(P::OptedOut),
                "opted out beats all, in: {name}"
            );
        }
    }

    #[test]
    fn optout_cooldown() {
        let now = t0();
        let six = now - Duration::days(6);
        assert_eq!(
            can_switch_optout(Some(six), now, 7),
            Err(six + Duration::days(7))
        );
        assert_eq!(
            can_switch_optout(Some(now - Duration::days(7)), now, 7),
            Ok(())
        );
        assert_eq!(can_switch_optout(Some(now), now, 0), Ok(()));
        assert_eq!(can_switch_optout(None, now, 7), Ok(()));
    }
}
