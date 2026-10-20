//! Who can be pranked: the pure target checks, plus the thin cache glue for
//! the never-widen and role-position facts.

use super::immunity::Protection;
use super::settings::ActionKind;
use super::shop::Refusal;
use crate::music::perms::roles_resolvable;
use crate::rooms::guard::{member_can_join, BaselineView};
use crate::rooms::state::StoredOverwrite;
use chrono::{DateTime, Utc};
use serenity::all::{Cache, ChannelId, GuildId, Permissions, RoleId, UserId};
use std::collections::HashMap;

/// What [`check_target`] needs to know about the member being pranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFacts {
    pub is_bot: bool,
    pub is_self: bool,
    pub is_owner: bool,
    /// `None` when the cache cannot say: a prank is then out of reach.
    pub top_role_position: Option<u16>,
    /// 0 when unknown, which fails closed: `>= 0` is always true.
    pub bot_top_role_position: u16,
    pub in_voice: Option<ChannelId>,
    pub opted_out: bool,
    pub protection: Option<Protection>,
    pub has_mirror: bool,
    /// When a prank of the same slot (rename and clown share one) ends.
    pub running_same_kind: Option<DateTime<Utc>>,
    /// Could the target join the drag destination on their own? `None` means
    /// unknown or not applicable. Drag treats `None` as a refusal; jail and
    /// the other kinds ignore it.
    pub can_join_destination: Option<bool>,
    /// What a name prank restores.
    pub current_nick: Option<String>,
}

/// Where a purchase that passed the checks goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOutcome {
    /// The prank lands on the target.
    Lands,
    /// The target holds a mirror: it bounces to the buyer.
    Mirrored,
}

/// Check a target, refusing for the first reason that applies, in order:
/// bot, self, out of reach, voice needs, opted out, then a mirror (which
/// returns [`TargetOutcome::Mirrored`] before immunity is read), immunity, the
/// same prank already running, and the never-widen destination check (drag
/// only: jail is exempt, spec ruling Q4).
pub fn check_target(
    kind: ActionKind,
    t: &TargetFacts,
    buyer_in_voice: Option<ChannelId>,
) -> Result<TargetOutcome, Refusal> {
    if t.is_bot {
        return Err(Refusal::TargetIsBot);
    }
    if kind.is_prank() {
        if t.is_self {
            return Err(Refusal::TargetIsSelf);
        }
        if t.is_owner
            || t.top_role_position
                .is_none_or(|p| p >= t.bot_top_role_position)
        {
            return Err(Refusal::OutOfReach);
        }
    }
    match kind {
        ActionKind::Drag => {
            if buyer_in_voice.is_none() {
                return Err(Refusal::BuyerNotInVoice);
            }
            if t.in_voice.is_none() {
                return Err(Refusal::TargetNotInVoice);
            }
        },
        ActionKind::Mute | ActionKind::Deafen | ActionKind::Jail if t.in_voice.is_none() => {
            return Err(Refusal::TargetNotInVoice);
        },
        _ => {},
    }
    if t.opted_out {
        return Err(Refusal::TargetOptedOut);
    }
    if t.has_mirror {
        return Ok(TargetOutcome::Mirrored);
    }
    if let Some(p) = t.protection {
        return Err(Refusal::TargetImmune(p));
    }
    if let Some(until) = t.running_same_kind {
        return Err(Refusal::AlreadyRunning { until });
    }
    if kind == ActionKind::Drag && t.can_join_destination != Some(true) {
        return Err(Refusal::CannotJoinDestination);
    }
    Ok(TargetOutcome::Lands)
}

/// The highest of `roles`' positions; 0 when none is known.
pub fn top_of(roles: &[RoleId], position: impl Fn(RoleId) -> Option<u16>) -> u16 {
    roles.iter().filter_map(|r| position(*r)).max().unwrap_or(0)
}

/// Whether `member` could join `channel` on their own, by Discord's
/// algorithm over the channel's overwrites. `None` when the cache cannot
/// answer (guild, member, channel or one of their roles not cached).
pub fn can_join(cache: &Cache, guild: GuildId, member: UserId, channel: ChannelId) -> Option<bool> {
    let g = cache.guild(guild)?;
    let m = g.members.get(&member)?;
    if !roles_resolvable(guild, &m.roles, |r| g.roles.contains_key(&r)) {
        return None;
    }
    let ch = g.channels.get(&channel)?;
    let everyone = g.roles.get(&RoleId::new(guild.get()))?.permissions;
    let role_perms: HashMap<RoleId, Permissions> =
        g.roles.iter().map(|r| (r.id, r.permissions)).collect();
    let overwrites: Vec<StoredOverwrite> = ch
        .permission_overwrites
        .iter()
        .map(StoredOverwrite::from)
        .collect();
    let view = BaselineView {
        guild,
        everyone,
        role_perms: &role_perms,
        overwrites: &overwrites,
    };
    Some(member_can_join(&view, member, &m.roles))
}

/// The member's highest role position from the cache; `Some(0)` for no
/// roles, `None` when the cache cannot answer (guild or member missing, or a
/// role not cached), which callers treat as out of reach.
pub fn top_position(cache: &Cache, guild: GuildId, member: UserId) -> Option<u16> {
    let g = cache.guild(guild)?;
    let m = g.members.get(&member)?;
    if !roles_resolvable(guild, &m.roles, |r| g.roles.contains_key(&r)) {
        return None;
    }
    Some(top_of(&m.roles, |r| {
        g.roles
            .get(&r)
            .map(|role| role.position.max(0).cast_unsigned())
    }))
}

/// Whether `member` owns the server (always out of reach). `false` when the
/// guild is not cached.
pub fn is_owner(cache: &Cache, guild: GuildId, member: UserId) -> bool {
    cache.guild(guild).is_some_and(|g| g.owner_id == member)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::state::OverwriteTarget;

    const VC: ChannelId = ChannelId::new(5);
    const OTHER_VC: ChannelId = ChannelId::new(6);

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    /// A clean target: every check passes for a prank that needs voice.
    fn clean() -> TargetFacts {
        TargetFacts {
            is_bot: false,
            is_self: false,
            is_owner: false,
            top_role_position: Some(1),
            bot_top_role_position: 5,
            in_voice: Some(VC),
            opted_out: false,
            protection: None,
            has_mirror: false,
            running_same_kind: None,
            can_join_destination: Some(true),
            current_nick: None,
        }
    }

    /// Everything wrong at once.
    fn all_wrong() -> TargetFacts {
        TargetFacts {
            is_bot: true,
            is_self: true,
            is_owner: true,
            top_role_position: Some(9),
            bot_top_role_position: 5,
            in_voice: None,
            opted_out: true,
            protection: Some(Protection::Indefinitely),
            has_mirror: true,
            running_same_kind: Some(t0()),
            can_join_destination: Some(false),
            current_nick: None,
        }
    }

    #[test]
    fn check_target_order() {
        let kind = ActionKind::Drag;
        let buyer = Some(VC);
        let mut f = all_wrong();
        assert_eq!(check_target(kind, &f, buyer), Err(Refusal::TargetIsBot));
        f.is_bot = false;
        assert_eq!(check_target(kind, &f, buyer), Err(Refusal::TargetIsSelf));
        f.is_self = false;
        assert_eq!(check_target(kind, &f, buyer), Err(Refusal::OutOfReach));
        f.is_owner = false;
        f.top_role_position = Some(1);
        // Drag: the buyer must be in voice, then the target.
        assert_eq!(check_target(kind, &f, None), Err(Refusal::BuyerNotInVoice));
        assert_eq!(
            check_target(kind, &f, buyer),
            Err(Refusal::TargetNotInVoice)
        );
        f.in_voice = Some(VC);
        assert_eq!(check_target(kind, &f, buyer), Err(Refusal::TargetOptedOut));
        f.opted_out = false;
        // A mirror wins over immunity, running and the destination.
        assert_eq!(check_target(kind, &f, buyer), Ok(TargetOutcome::Mirrored));
        f.has_mirror = false;
        assert_eq!(
            check_target(kind, &f, buyer),
            Err(Refusal::TargetImmune(Protection::Indefinitely))
        );
        f.protection = None;
        assert_eq!(
            check_target(kind, &f, buyer),
            Err(Refusal::AlreadyRunning { until: t0() })
        );
        f.running_same_kind = None;
        assert_eq!(
            check_target(kind, &f, buyer),
            Err(Refusal::CannotJoinDestination)
        );
        f.can_join_destination = Some(true);
        assert_eq!(check_target(kind, &f, buyer), Ok(TargetOutcome::Lands));
    }

    #[test]
    fn out_of_reach_covers_owner_and_equal_or_higher_roles() {
        let kind = ActionKind::Rename;
        let mut f = clean();
        assert_eq!(check_target(kind, &f, None), Ok(TargetOutcome::Lands));
        f.top_role_position = Some(4);
        assert_eq!(check_target(kind, &f, None), Ok(TargetOutcome::Lands));
        f.top_role_position = Some(5);
        assert_eq!(check_target(kind, &f, None), Err(Refusal::OutOfReach));
        f.top_role_position = Some(6);
        assert_eq!(check_target(kind, &f, None), Err(Refusal::OutOfReach));
        f.top_role_position = Some(0);
        f.is_owner = true;
        assert_eq!(check_target(kind, &f, None), Err(Refusal::OutOfReach));
        for k in [
            ActionKind::Rename,
            ActionKind::Lock,
            ActionKind::Clown,
            ActionKind::Drag,
            ActionKind::Mute,
            ActionKind::Deafen,
            ActionKind::Jail,
        ] {
            assert_eq!(
                check_target(k, &f, Some(VC)),
                Err(Refusal::OutOfReach),
                "{k:?}"
            );
        }
    }

    #[test]
    fn mirror_is_checked_before_immunity() {
        let mut f = clean();
        f.has_mirror = true;
        f.protection = Some(Protection::Until(t0()));
        assert_eq!(
            check_target(ActionKind::Mute, &f, None),
            Ok(TargetOutcome::Mirrored)
        );
        // Opt-out still comes first: an opted-out member is not in the game.
        f.opted_out = true;
        assert_eq!(
            check_target(ActionKind::Mute, &f, None),
            Err(Refusal::TargetOptedOut)
        );
    }

    #[test]
    fn rename_and_clown_share_a_slot() {
        // The caller fills `running_same_kind` from the shared slot, so the
        // check refuses whichever of the two kinds is being bought.
        let mut f = clean();
        f.running_same_kind = Some(t0());
        for k in [ActionKind::Rename, ActionKind::Clown, ActionKind::Lock] {
            assert_eq!(
                check_target(k, &f, None),
                Err(Refusal::AlreadyRunning { until: t0() }),
                "{k:?}"
            );
        }
        f.running_same_kind = None;
        assert_eq!(
            check_target(ActionKind::Clown, &f, None),
            Ok(TargetOutcome::Lands)
        );
    }

    #[test]
    fn drag_refuses_a_channel_the_target_cannot_join() {
        let mut f = clean();
        f.can_join_destination = Some(false);
        assert_eq!(
            check_target(ActionKind::Drag, &f, Some(OTHER_VC)),
            Err(Refusal::CannotJoinDestination)
        );
        f.can_join_destination = Some(true);
        assert_eq!(
            check_target(ActionKind::Drag, &f, Some(OTHER_VC)),
            Ok(TargetOutcome::Lands)
        );
        f.can_join_destination = None;
        assert_eq!(
            check_target(ActionKind::Drag, &f, Some(OTHER_VC)),
            Err(Refusal::CannotJoinDestination)
        );
        // Other kinds never read it.
        f.can_join_destination = Some(false);
        assert_eq!(
            check_target(ActionKind::Mute, &f, None),
            Ok(TargetOutcome::Lands)
        );
    }

    #[test]
    fn drag_refuses_an_unknown_destination() {
        let mut f = clean();
        f.can_join_destination = None;
        assert_eq!(
            check_target(ActionKind::Drag, &f, Some(VC)),
            Err(Refusal::CannotJoinDestination)
        );
        for k in [ActionKind::Jail, ActionKind::Mute, ActionKind::Rename] {
            assert_eq!(
                check_target(k, &f, Some(VC)),
                Ok(TargetOutcome::Lands),
                "{k:?}"
            );
        }
    }

    #[test]
    fn unknown_target_position_is_out_of_reach() {
        let mut f = clean();
        f.top_role_position = None;
        for k in [
            ActionKind::Rename,
            ActionKind::Lock,
            ActionKind::Clown,
            ActionKind::Drag,
            ActionKind::Mute,
            ActionKind::Deafen,
            ActionKind::Jail,
        ] {
            assert_eq!(
                check_target(k, &f, Some(VC)),
                Err(Refusal::OutOfReach),
                "{k:?}"
            );
        }
        for k in [ActionKind::Mirror, ActionKind::Cleanse, ActionKind::Reveal] {
            assert_eq!(
                check_target(k, &f, Some(VC)),
                Ok(TargetOutcome::Lands),
                "{k:?}"
            );
        }
    }

    #[test]
    fn jail_ignores_never_widen() {
        let mut f = clean();
        f.can_join_destination = Some(false);
        assert_eq!(
            check_target(ActionKind::Jail, &f, None),
            Ok(TargetOutcome::Lands)
        );
    }

    #[test]
    fn voice_requirements_per_kind() {
        let mut f = clean();
        f.in_voice = None;
        for k in [ActionKind::Mute, ActionKind::Deafen, ActionKind::Jail] {
            assert_eq!(
                check_target(k, &f, Some(VC)),
                Err(Refusal::TargetNotInVoice),
                "{k:?}"
            );
        }
        // Names don't need voice, and neither does the buyer for them.
        for k in [ActionKind::Rename, ActionKind::Lock, ActionKind::Clown] {
            assert_eq!(check_target(k, &f, None), Ok(TargetOutcome::Lands), "{k:?}");
        }
        // Drag needs both.
        assert_eq!(
            check_target(ActionKind::Drag, &f, None),
            Err(Refusal::BuyerNotInVoice)
        );
        assert_eq!(
            check_target(ActionKind::Drag, &f, Some(VC)),
            Err(Refusal::TargetNotInVoice)
        );
        // Mute, deafen and jail don't need the buyer in voice.
        f.in_voice = Some(VC);
        for k in [ActionKind::Mute, ActionKind::Deafen, ActionKind::Jail] {
            assert_eq!(check_target(k, &f, None), Ok(TargetOutcome::Lands), "{k:?}");
        }
    }

    #[test]
    fn top_of_takes_the_highest_known_position() {
        let pos = |r: RoleId| match r.get() {
            1 => Some(3),
            2 => Some(7),
            _ => None,
        };
        assert_eq!(top_of(&[RoleId::new(1), RoleId::new(2)], pos), 7);
        assert_eq!(top_of(&[RoleId::new(1), RoleId::new(99)], pos), 3);
        assert_eq!(top_of(&[RoleId::new(99)], pos), 0);
        assert_eq!(top_of(&[], pos), 0);
    }

    #[test]
    fn overwrites_convert_the_way_rooms_reads_a_category() {
        // The glue feeds StoredOverwrite::from into member_can_join: a deny of
        // Connect on a role the member holds keeps them out.
        let guild = GuildId::new(100);
        let role = RoleId::new(7);
        let ovs = [StoredOverwrite {
            target: OverwriteTarget::Role { id: role },
            allow: Permissions::empty(),
            deny: Permissions::CONNECT,
        }];
        let role_perms: HashMap<RoleId, Permissions> = [(role, Permissions::empty())].into();
        let view = BaselineView {
            guild,
            everyone: Permissions::VIEW_CHANNEL | Permissions::CONNECT,
            role_perms: &role_perms,
            overwrites: &ovs,
        };
        assert!(!member_can_join(&view, UserId::new(1), &[role]));
        assert!(member_can_join(&view, UserId::new(1), &[]));
    }
}
