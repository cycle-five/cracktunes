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
