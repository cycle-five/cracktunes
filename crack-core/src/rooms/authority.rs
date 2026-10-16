//! Who may do what in a room. Pure decisions, no Discord calls.

use super::card::RoomRefusal;
use super::state::RoomState;
use serenity::all::{Permissions, RoleId, UserId};

/// A member's standing relative to one room. Moderator is a superset of Owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Owner,
    Helper,
    Moderator,
    Occupant,
    Outsider,
}

/// A kick or mute reason: trimmed, 1..=[`Reason::MAX`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason(String);

impl Reason {
    pub const MAX: usize = 200;

    pub fn parse(raw: &str) -> Result<Self, RoomRefusal> {
        let t = raw.trim();
        if t.is_empty() {
            return Err(RoomRefusal::ReasonRequired);
        }
        if t.chars().count() > Self::MAX {
            return Err(RoomRefusal::ReasonTooLong);
        }
        Ok(Self(t.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomAction {
    Kick {
        target: UserId,
        reason: Reason,
    },
    LetIn {
        target: UserId,
    },
    Mute {
        target: UserId,
        reason: Option<Reason>,
    },
    Unmute {
        target: UserId,
    },
    Lock,
    Unlock,
    ChatLock,
    ChatUnlock,
    Limit(u8),
    Rename(String),
    Transfer {
        target: UserId,
    },
    HelperAdd {
        target: UserId,
    },
    HelperRemove {
        target: UserId,
    },
    AllowUser {
        target: UserId,
    },
    AllowRole {
        role: RoleId,
    },
    BlockRole {
        role: RoleId,
    },
    UnblockRole {
        role: RoleId,
    },
    Claim,
    Panel,
}

/// What the target of an action is, relative to whoever acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetStanding {
    Itself,
    TheBot,
    Owner,
    Helper,
    Moderator,
    Plain,
}

/// Priority: Moderator > Owner > Helper > Occupant > Outsider.
pub fn standing(user: UserId, state: &RoomState, is_moderator: bool, in_room: bool) -> Standing {
    if is_moderator {
        Standing::Moderator
    } else if user == state.owner {
        Standing::Owner
    } else if state.helpers.contains(&user) {
        Standing::Helper
    } else if in_room {
        Standing::Occupant
    } else {
        Standing::Outsider
    }
}

/// Server-wide moderator powers: any of ADMINISTRATOR, MANAGE_CHANNELS, MOVE_MEMBERS.
pub fn is_moderator(guild_perms: Permissions) -> bool {
    guild_perms.intersects(
        Permissions::ADMINISTRATOR | Permissions::MANAGE_CHANNELS | Permissions::MOVE_MEMBERS,
    )
}

fn helper_may(action: &RoomAction) -> bool {
    matches!(
        action,
        RoomAction::Kick { .. }
            | RoomAction::LetIn { .. }
            | RoomAction::Mute { .. }
            | RoomAction::Unmute { .. }
            | RoomAction::Lock
            | RoomAction::Unlock
            | RoomAction::ChatLock
            | RoomAction::ChatUnlock
            | RoomAction::Limit(_)
            | RoomAction::Panel
    )
}

/// Whether `who` may attempt `action` at all (targets are checked by [`target_ok`]).
pub fn may(who: Standing, action: &RoomAction, owner_present: bool) -> Result<(), RoomRefusal> {
    if who == Standing::Outsider {
        return Err(RoomRefusal::NotAllowed);
    }
    if matches!(action, RoomAction::Claim) {
        return match who {
            Standing::Owner => Err(RoomRefusal::AlreadyOwner),
            _ if owner_present => Err(RoomRefusal::OwnerPresent),
            _ => Ok(()),
        };
    }
    match who {
        Standing::Owner | Standing::Moderator => Ok(()),
        Standing::Helper if helper_may(action) => Ok(()),
        _ => Err(RoomRefusal::NotAllowed),
    }
}

/// Whether `action` may be aimed at a target of the given standing.
pub fn target_ok(
    who: Standing,
    action: &RoomAction,
    target: TargetStanding,
) -> Result<(), RoomRefusal> {
    let refuse = Err(RoomRefusal::ProtectedTarget);
    match action {
        RoomAction::Kick { .. } | RoomAction::Mute { .. } => match target {
            TargetStanding::Itself | TargetStanding::TheBot | TargetStanding::Moderator => refuse,
            TargetStanding::Owner if who != Standing::Moderator => refuse,
            TargetStanding::Helper if who == Standing::Helper => refuse,
            _ => Ok(()),
        },
        RoomAction::Transfer { .. } | RoomAction::HelperAdd { .. } => match target {
            TargetStanding::Itself | TargetStanding::TheBot => refuse,
            _ => Ok(()),
        },
        RoomAction::LetIn { .. }
        | RoomAction::Unmute { .. }
        | RoomAction::HelperRemove { .. }
        | RoomAction::AllowUser { .. } => match target {
            TargetStanding::TheBot => refuse,
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::state::RoomState;

    const OWNER: UserId = UserId::new(1);
    const HELPER: UserId = UserId::new(2);
    const OTHER: UserId = UserId::new(3);
    const T: UserId = UserId::new(9);

    fn state() -> RoomState {
        let mut s = RoomState::new(OWNER);
        s.helpers.insert(HELPER);
        s
    }

    fn reason() -> Reason {
        Reason::parse("because").unwrap()
    }

    fn all_actions() -> Vec<RoomAction> {
        vec![
            RoomAction::Kick {
                target: T,
                reason: reason(),
            },
            RoomAction::LetIn { target: T },
            RoomAction::Mute {
                target: T,
                reason: None,
            },
            RoomAction::Unmute { target: T },
            RoomAction::Lock,
            RoomAction::Unlock,
            RoomAction::ChatLock,
            RoomAction::ChatUnlock,
            RoomAction::Limit(5),
            RoomAction::Rename("x".into()),
            RoomAction::Transfer { target: T },
            RoomAction::HelperAdd { target: T },
            RoomAction::HelperRemove { target: T },
            RoomAction::AllowUser { target: T },
            RoomAction::AllowRole {
                role: RoleId::new(4),
            },
            RoomAction::BlockRole {
                role: RoleId::new(4),
            },
            RoomAction::UnblockRole {
                role: RoleId::new(4),
            },
            RoomAction::Claim,
            RoomAction::Panel,
        ]
    }

    fn is_claim(a: &RoomAction) -> bool {
        matches!(a, RoomAction::Claim)
    }

    fn helper_ok(a: &RoomAction) -> bool {
        matches!(
            a,
            RoomAction::Kick { .. }
                | RoomAction::LetIn { .. }
                | RoomAction::Mute { .. }
                | RoomAction::Unmute { .. }
                | RoomAction::Lock
                | RoomAction::Unlock
                | RoomAction::ChatLock
                | RoomAction::ChatUnlock
                | RoomAction::Limit(_)
                | RoomAction::Panel
        )
    }

    #[test]
    fn standing_priority_and_membership() {
        let s = state();
        assert_eq!(standing(OWNER, &s, true, true), Standing::Moderator);
        assert_eq!(standing(OWNER, &s, false, true), Standing::Owner);
        assert_eq!(standing(HELPER, &s, false, true), Standing::Helper);
        assert_eq!(standing(OTHER, &s, false, true), Standing::Occupant);
        assert_eq!(standing(OTHER, &s, false, false), Standing::Outsider);
        assert_eq!(standing(OTHER, &s, true, false), Standing::Moderator);
        assert!(is_moderator(Permissions::MOVE_MEMBERS));
        assert!(is_moderator(Permissions::MANAGE_CHANNELS));
        assert!(is_moderator(Permissions::ADMINISTRATOR));
        assert!(!is_moderator(
            Permissions::CONNECT | Permissions::MUTE_MEMBERS
        ));
    }

    #[test]
    fn owner_may_do_everything_but_claim() {
        for a in all_actions() {
            let r = may(Standing::Owner, &a, true);
            if is_claim(&a) {
                assert_eq!(r, Err(RoomRefusal::AlreadyOwner));
            } else {
                assert_eq!(r, Ok(()), "{a:?}");
            }
        }
    }

    #[test]
    fn helper_may_only_moderate() {
        for a in all_actions() {
            let r = may(Standing::Helper, &a, true);
            if helper_ok(&a) {
                assert_eq!(r, Ok(()), "{a:?}");
            } else if is_claim(&a) {
                assert_eq!(r, Err(RoomRefusal::OwnerPresent));
            } else {
                assert_eq!(r, Err(RoomRefusal::NotAllowed), "{a:?}");
            }
        }
        assert_eq!(may(Standing::Helper, &RoomAction::Claim, false), Ok(()));
    }

    #[test]
    fn occupant_may_only_claim_when_owner_absent() {
        for a in all_actions() {
            let r = may(Standing::Occupant, &a, false);
            if is_claim(&a) {
                assert_eq!(r, Ok(()));
            } else {
                assert_eq!(r, Err(RoomRefusal::NotAllowed), "{a:?}");
            }
        }
        assert_eq!(
            may(Standing::Occupant, &RoomAction::Claim, true),
            Err(RoomRefusal::OwnerPresent)
        );
    }

    #[test]
    fn outsider_may_nothing() {
        for a in all_actions() {
            let r = may(Standing::Outsider, &a, false);
            assert_eq!(r, Err(RoomRefusal::NotAllowed), "{a:?}");
        }
    }

    #[test]
    fn moderator_may_everything() {
        for a in all_actions() {
            let r = may(Standing::Moderator, &a, true);
            if is_claim(&a) {
                assert_eq!(r, Err(RoomRefusal::OwnerPresent));
            } else {
                assert_eq!(r, Ok(()), "{a:?}");
            }
        }
        assert_eq!(may(Standing::Moderator, &RoomAction::Claim, false), Ok(()));
    }

    #[test]
    fn kick_refuses_self_bot_moderator_and_owner_unless_moderator() {
        let kick = RoomAction::Kick {
            target: T,
            reason: reason(),
        };
        let mute = RoomAction::Mute {
            target: T,
            reason: None,
        };
        for a in [&kick, &mute] {
            for who in [Standing::Owner, Standing::Moderator] {
                assert_eq!(
                    target_ok(who, a, TargetStanding::Itself),
                    Err(RoomRefusal::ProtectedTarget)
                );
                assert_eq!(
                    target_ok(who, a, TargetStanding::TheBot),
                    Err(RoomRefusal::ProtectedTarget)
                );
                assert_eq!(
                    target_ok(who, a, TargetStanding::Moderator),
                    Err(RoomRefusal::ProtectedTarget)
                );
                assert_eq!(target_ok(who, a, TargetStanding::Helper), Ok(()));
                assert_eq!(target_ok(who, a, TargetStanding::Plain), Ok(()));
            }
            assert_eq!(
                target_ok(Standing::Moderator, a, TargetStanding::Owner),
                Ok(())
            );
            assert_eq!(
                target_ok(Standing::Owner, a, TargetStanding::Owner),
                Err(RoomRefusal::ProtectedTarget)
            );
            assert_eq!(
                target_ok(Standing::Helper, a, TargetStanding::Owner),
                Err(RoomRefusal::ProtectedTarget)
            );
        }
    }

    #[test]
    fn helper_cannot_kick_helper() {
        let kick = RoomAction::Kick {
            target: T,
            reason: reason(),
        };
        assert_eq!(
            target_ok(Standing::Helper, &kick, TargetStanding::Helper),
            Err(RoomRefusal::ProtectedTarget)
        );
        assert_eq!(
            target_ok(Standing::Helper, &kick, TargetStanding::Plain),
            Ok(())
        );
    }

    #[test]
    fn unmute_letin_refuse_only_the_bot() {
        for a in [
            RoomAction::Unmute { target: T },
            RoomAction::LetIn { target: T },
        ] {
            assert_eq!(
                target_ok(Standing::Helper, &a, TargetStanding::TheBot),
                Err(RoomRefusal::ProtectedTarget)
            );
            for t in [
                TargetStanding::Itself,
                TargetStanding::Owner,
                TargetStanding::Helper,
                TargetStanding::Moderator,
                TargetStanding::Plain,
            ] {
                assert_eq!(target_ok(Standing::Helper, &a, t), Ok(()), "{a:?} {t:?}");
            }
        }
    }

    #[test]
    fn transfer_and_helper_add_refuse_self_and_bot() {
        for a in [
            RoomAction::Transfer { target: T },
            RoomAction::HelperAdd { target: T },
        ] {
            assert_eq!(
                target_ok(Standing::Owner, &a, TargetStanding::Itself),
                Err(RoomRefusal::ProtectedTarget)
            );
            assert_eq!(
                target_ok(Standing::Owner, &a, TargetStanding::TheBot),
                Err(RoomRefusal::ProtectedTarget)
            );
            assert_eq!(
                target_ok(Standing::Owner, &a, TargetStanding::Plain),
                Ok(())
            );
        }
        let a = RoomAction::HelperRemove { target: T };
        assert_eq!(
            target_ok(Standing::Owner, &a, TargetStanding::Itself),
            Ok(())
        );
        assert_eq!(
            target_ok(Standing::Owner, &a, TargetStanding::TheBot),
            Err(RoomRefusal::ProtectedTarget)
        );
    }

    #[test]
    fn reason_parse_trims_and_bounds() {
        assert_eq!(Reason::parse("  "), Err(RoomRefusal::ReasonRequired));
        assert_eq!(
            Reason::parse(&"a".repeat(201)),
            Err(RoomRefusal::ReasonTooLong)
        );
        assert_eq!(Reason::parse(&"a".repeat(200)).unwrap().as_str().len(), 200);
        assert_eq!(Reason::parse("  hi  ").unwrap().as_str(), "hi");
        // Counted in chars, not bytes.
        assert!(Reason::parse(&"é".repeat(200)).is_ok());
    }
}
