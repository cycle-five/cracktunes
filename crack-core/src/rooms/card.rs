//! What a room command or button tells the member when it cannot proceed.

/// Why a room action was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomRefusal {
    /// The member is not in a voice room.
    NotInRoom,
    /// The member's standing in the room does not allow this action.
    NotAllowed,
    /// The target is protected (the bot, a moderator, the owner or oneself).
    ProtectedTarget,
    /// The target is not in this room.
    TargetNotInRoom,
    /// The target is not blocked from this room.
    NotBlocked,
    /// The server's own permissions keep the member or role out of this room.
    CannotJoinHere,
    /// Renaming is rate limited; try again after the wait.
    RenameLimited { wait_secs: u32 },
    /// The member limit is out of range.
    BadLimit,
    /// The room name is empty or too long.
    BadName,
    /// A reason is required and was empty.
    ReasonRequired,
    /// The reason is longer than the maximum.
    ReasonTooLong,
    /// A modal was submitted without the member, role or mode its action needs.
    MissingChoice,
    /// The room already has its owner present.
    OwnerPresent,
    /// The member already owns this room.
    AlreadyOwner,
    /// There are too many options to fit in a select menu.
    TooManyForMenu,
    /// The bot lacks a Discord permission it needs.
    MissingPermissions,
    /// Discord rejected the call.
    DiscordFailed,
    /// Rooms need a database and none is configured.
    NoDatabase,
    /// Rooms are not enabled on this server.
    RoomsOff,
}
