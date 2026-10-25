//! Custom ids for the room panel's buttons and modals.
//!
//! 🔑 Like the now-playing buttons, an id carries everything a press needs:
//! `room:<kind>:<channel>` for a button, `roomm:<kind>:<channel>` for the modal
//! it opens. A panel on an old message still works after a restart, and a press
//! names the room it was drawn for.
use crack_model::RoomRef;
use serenity::all::ChannelId;

/// Every room button's custom id starts with this.
pub const ROOM_PREFIX: &str = "room:";
/// Every room modal's custom id starts with this.
pub const ROOM_MODAL_PREFIX: &str = "roomm:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomButton {
    Kick,
    Mute,
    Unmute,
    LetIn,
    Lock,
    Unlock,
    ChatLock,
    ChatUnlock,
    Limit,
    Rename,
    Transfer,
    Helpers,
    WhoCanJoin,
    Claim,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomModal {
    Kick,
    Mute,
    Unmute,
    LetIn,
    Limit,
    Rename,
    Transfer,
    Helpers,
    WhoCanJoin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonId {
    pub kind: RoomButton,
    pub room: ChannelId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalId {
    pub kind: RoomModal,
    pub room: ChannelId,
}

/// Names of the inputs in a room modal.
pub mod field {
    pub const MEMBER: &str = "member";
    pub const REASON: &str = "reason";
    pub const VALUE: &str = "value";
    pub const MODE: &str = "mode";
    pub const ROLE: &str = "role";
}

impl RoomButton {
    /// Every button, for tests and for anything that must cover them all.
    pub const ALL: [RoomButton; 15] = [
        Self::Kick,
        Self::Mute,
        Self::Unmute,
        Self::LetIn,
        Self::Lock,
        Self::Unlock,
        Self::ChatLock,
        Self::ChatUnlock,
        Self::Limit,
        Self::Rename,
        Self::Transfer,
        Self::Helpers,
        Self::WhoCanJoin,
        Self::Claim,
        Self::Help,
    ];

    fn slug(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::LetIn => "let-in",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::ChatLock => "chat-lock",
            Self::ChatUnlock => "chat-unlock",
            Self::Limit => "limit",
            Self::Rename => "rename",
            Self::Transfer => "transfer",
            Self::Helpers => "helpers",
            Self::WhoCanJoin => "who",
            Self::Claim => "claim",
            Self::Help => "help",
        }
    }

    fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.slug() == s)
    }

    /// The custom id of this button on `room`'s panel; the same text as
    /// [`ButtonId::custom_id`], for a room that is still a ref.
    #[must_use]
    pub fn custom_id_for(self, room: &RoomRef) -> String {
        format!("{ROOM_PREFIX}{}:{}", self.slug(), room.raw().as_str())
    }
}

impl RoomModal {
    pub const ALL: [RoomModal; 9] = [
        Self::Kick,
        Self::Mute,
        Self::Unmute,
        Self::LetIn,
        Self::Limit,
        Self::Rename,
        Self::Transfer,
        Self::Helpers,
        Self::WhoCanJoin,
    ];

    fn slug(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::LetIn => "let-in",
            Self::Limit => "limit",
            Self::Rename => "rename",
            Self::Transfer => "transfer",
            Self::Helpers => "helpers",
            Self::WhoCanJoin => "who",
        }
    }

    fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.slug() == s)
    }
}

/// `<kind>:<channel>` after `prefix`, with nothing after it. Never panics:
/// serenity ids are `NonMaxU64`, so `ChannelId::new(u64::MAX)` panics; that
/// and the never-issued 0 are rejected before it.
fn parse_parts<'a>(id: &'a str, prefix: &str) -> Option<(&'a str, ChannelId)> {
    let mut parts = id.strip_prefix(prefix)?.split(':');
    let kind = parts.next()?;
    let room = parts
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|&n| n != 0 && n != u64::MAX)
        .map(ChannelId::new)?;
    parts.next().is_none().then_some((kind, room))
}

impl ButtonId {
    #[must_use]
    pub fn custom_id(&self) -> String {
        format!("{ROOM_PREFIX}{}:{}", self.kind.slug(), self.room)
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let (kind, room) = parse_parts(id, ROOM_PREFIX)?;
        Some(Self {
            kind: RoomButton::from_slug(kind)?,
            room,
        })
    }
}

impl ModalId {
    #[must_use]
    pub fn custom_id(&self) -> String {
        format!("{ROOM_MODAL_PREFIX}{}:{}", self.kind.slug(), self.room)
    }

    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let (kind, room) = parse_parts(id, ROOM_MODAL_PREFIX)?;
        Some(Self {
            kind: RoomModal::from_slug(kind)?,
            room,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: ChannelId = ChannelId::new(123456789012345678);

    #[test]
    fn button_ids_round_trip_every_kind() {
        let mut seen = std::collections::HashSet::new();
        for kind in RoomButton::ALL {
            let b = ButtonId { kind, room: ROOM };
            let id = b.custom_id();
            assert_eq!(ButtonId::parse(&id), Some(b), "{id}");
            assert!(id.starts_with(ROOM_PREFIX) && id.ends_with(":123456789012345678"));
            assert!(seen.insert(id), "two kinds share an id");
        }
        // The ref spelling is the same text.
        let room = crack_types::discord::ToRef::to_ref(&ROOM);
        for kind in RoomButton::ALL {
            assert_eq!(
                kind.custom_id_for(&room),
                ButtonId { kind, room: ROOM }.custom_id()
            );
        }
        // The wire spelling is a contract with panels already posted.
        let slugs: Vec<_> = RoomButton::ALL
            .into_iter()
            .map(|kind| ButtonId { kind, room: ROOM }.custom_id())
            .collect();
        let expected = [
            "kick",
            "mute",
            "unmute",
            "let-in",
            "lock",
            "unlock",
            "chat-lock",
            "chat-unlock",
            "limit",
            "rename",
            "transfer",
            "helpers",
            "who",
            "claim",
            "help",
        ]
        .map(|k| format!("room:{k}:123456789012345678"));
        assert_eq!(slugs, expected);
    }

    #[test]
    fn modal_ids_round_trip_every_kind() {
        let mut seen = std::collections::HashSet::new();
        for kind in RoomModal::ALL {
            let m = ModalId { kind, room: ROOM };
            let id = m.custom_id();
            assert_eq!(ModalId::parse(&id), Some(m), "{id}");
            assert!(id.starts_with(ROOM_MODAL_PREFIX));
            assert!(seen.insert(id), "two kinds share an id");
        }
        let slugs: Vec<_> = RoomModal::ALL
            .into_iter()
            .map(|kind| ModalId { kind, room: ROOM }.custom_id())
            .collect();
        let expected = [
            "kick", "mute", "unmute", "let-in", "limit", "rename", "transfer", "helpers", "who",
        ]
        .map(|k| format!("roomm:{k}:123456789012345678"));
        assert_eq!(slugs, expected);
    }

    #[test]
    fn ids_reject_zero_max_garbage_and_trailing_parts() {
        for bad in [
            "room:kick:0",
            "room:kick:18446744073709551615",
            "room:kick:1:2",
            "room:nope:1",
            "np:pause:1",
            "room:kick",
            "room:kick:",
            "room:kick:abc",
            "room:kick:-1",
            "room::1",
            "roomm:kick:1",
            "",
        ] {
            assert_eq!(ButtonId::parse(bad), None, "{bad}");
        }
        for bad in [
            "roomm:kick:0",
            "roomm:kick:18446744073709551615",
            "roomm:kick:1:2",
            "roomm:nope:1",
            "roomm:lock:1",
            "room:kick:1",
            "np:pause:1",
        ] {
            assert_eq!(ModalId::parse(bad), None, "{bad}");
        }
        assert!(ButtonId::parse("room:kick:1").is_some());
    }
}
