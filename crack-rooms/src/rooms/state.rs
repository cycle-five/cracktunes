//! A temporary voice room's persisted state.

use serde::{Deserialize, Serialize};
use serenity::all::{
    MessageId, PermissionOverwrite, PermissionOverwriteType, Permissions, RoleId, UserId,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverwriteTarget {
    Member { id: UserId },
    Role { id: RoleId },
}

/// A channel permission overwrite, in a form we own and can store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredOverwrite {
    pub target: OverwriteTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

impl From<&PermissionOverwrite> for StoredOverwrite {
    fn from(o: &PermissionOverwrite) -> Self {
        Self::of(o)
            .unwrap_or_else(|| unreachable!("serenity's overwrite types are member and role"))
    }
}

impl StoredOverwrite {
    /// The overwrite in our form; `None` for a kind serenity may add later.
    pub fn of(o: &PermissionOverwrite) -> Option<Self> {
        let target = match o.kind {
            PermissionOverwriteType::Member(id) => OverwriteTarget::Member { id },
            PermissionOverwriteType::Role(id) => OverwriteTarget::Role { id },
            _ => return None,
        };
        Some(Self {
            target,
            allow: o.allow,
            deny: o.deny,
        })
    }

    pub fn to_serenity(&self) -> PermissionOverwrite {
        PermissionOverwrite {
            allow: self.allow,
            deny: self.deny,
            kind: match self.target {
                OverwriteTarget::Member { id } => PermissionOverwriteType::Member(id),
                OverwriteTarget::Role { id } => PermissionOverwriteType::Role(id),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomState {
    pub owner: UserId,
    #[serde(default)]
    pub helpers: BTreeSet<UserId>,
    /// Kicked: cannot see or join.
    #[serde(default)]
    pub blocked: BTreeSet<UserId>,
    /// Cannot speak in this room.
    #[serde(default)]
    pub muted: BTreeSet<UserId>,
    /// May join while locked.
    #[serde(default)]
    pub permitted_users: BTreeSet<UserId>,
    #[serde(default)]
    pub permitted_roles: BTreeSet<RoleId>,
    #[serde(default)]
    pub blocked_roles: BTreeSet<RoleId>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub chat_locked: bool,
    #[serde(default)]
    pub panel: Option<MessageId>,
}

impl RoomState {
    pub fn new(owner: UserId) -> Self {
        Self {
            owner,
            helpers: BTreeSet::new(),
            blocked: BTreeSet::new(),
            muted: BTreeSet::new(),
            permitted_users: BTreeSet::new(),
            permitted_roles: BTreeSet::new(),
            blocked_roles: BTreeSet::new(),
            locked: false,
            chat_locked: false,
            panel: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> RoomState {
        RoomState {
            owner: UserId::new(1),
            helpers: [UserId::new(2), UserId::new(3)].into(),
            blocked: [UserId::new(4)].into(),
            muted: [UserId::new(5)].into(),
            permitted_users: [UserId::new(6)].into(),
            permitted_roles: [RoleId::new(7)].into(),
            blocked_roles: [RoleId::new(8)].into(),
            locked: true,
            chat_locked: true,
            panel: Some(MessageId::new(9)),
        }
    }

    #[test]
    fn room_state_round_trips_through_json() {
        let s = full();
        let raw = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<RoomState>(&raw).unwrap(), s);
    }

    #[test]
    fn room_state_reads_a_row_missing_new_fields() {
        let got: RoomState = serde_json::from_str(r#"{"owner":"1"}"#).unwrap();
        assert_eq!(got, RoomState::new(UserId::new(1)));
    }

    #[test]
    fn stored_overwrite_round_trips_through_serenity() {
        let member = PermissionOverwrite {
            allow: Permissions::CONNECT | Permissions::SPEAK,
            deny: Permissions::SEND_MESSAGES,
            kind: PermissionOverwriteType::Member(UserId::new(10)),
        };
        let role = PermissionOverwrite {
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::CONNECT,
            kind: PermissionOverwriteType::Role(RoleId::new(11)),
        };
        for o in [member, role] {
            let stored = StoredOverwrite::from(&o);
            assert_eq!(stored.allow, o.allow);
            assert_eq!(stored.deny, o.deny);
            let back = stored.to_serenity();
            assert_eq!(back.kind, o.kind);
            assert_eq!(back.allow, o.allow);
            assert_eq!(back.deny, o.deny);
            let raw = serde_json::to_string(&stored).unwrap();
            assert_eq!(
                serde_json::from_str::<StoredOverwrite>(&raw).unwrap(),
                stored
            );
        }
    }
}
