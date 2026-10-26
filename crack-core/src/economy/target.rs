//! Who can be pranked: the pure target checks, plus the thin cache glue for
//! the never-widen and role-position facts.

use super::immunity::Protection;
use super::settings::ActionKind;
use super::shop::Refusal;
use chrono::{DateTime, Utc};
use crack_common::perms::roles_resolvable;
use serenity::all::{Cache, ChannelId, Guild, GuildId, Member, RoleId, UserId};

/// What [`check_target`] needs to know about the member being pranked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetFacts {
    pub is_bot: bool,
    pub is_self: bool,
    pub is_owner: bool,
    /// `None` when one of their roles can't be placed: a prank is then
    /// refused as "can't tell" (fails closed).
    pub top_role_position: Option<u16>,
    /// The bot's own; `None` when it can't be placed, which fails closed the
    /// same way.
    pub bot_top_role_position: Option<u16>,
    pub in_voice: Option<ChannelId>,
    /// Server-muted now, by anyone, from the cached voice state; false when
    /// the member has no voice state.
    pub server_mute: bool,
    /// Server-deafened now, likewise.
    pub server_deaf: bool,
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
    /// Nickname, else global name, else username: what a clown is built from.
    pub display_name: String,
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
/// bot, self (pranks and the aimed kinds: theme, dedication, bounty), out of
/// reach (pranks only: the owner; then can't tell when a role position
/// is unknown; then the hierarchy), voice needs (a drag also refuses a
/// target already in the buyer's channel), already server-muted or -deafened
/// (a moderator's mute is never taken over, so never undone), opted out, then
/// a mirror (pranks only; it returns [`TargetOutcome::Mirrored`] before
/// immunity is read), immunity, the same prank already running, and the never-widen
/// destination check (drag only: jail is exempt, spec ruling Q4).
pub fn check_target(
    kind: ActionKind,
    t: &TargetFacts,
    buyer_in_voice: Option<ChannelId>,
) -> Result<TargetOutcome, Refusal> {
    check_target_with(kind, t, buyer_in_voice, VoiceStep::Apply)
}

/// Whether [`check_target_with`] makes the voice checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStep {
    Apply,
    /// A crowdfund's start: voice is looked at again when it fills.
    Skip,
}

/// [`check_target`] with the voice step optional, so a caller never has to
/// fake a voice state to get past it.
pub fn check_target_with(
    kind: ActionKind,
    t: &TargetFacts,
    buyer_in_voice: Option<ChannelId>,
    voice: VoiceStep,
) -> Result<TargetOutcome, Refusal> {
    if t.is_bot {
        return Err(Refusal::TargetIsBot);
    }
    if t.is_self && (kind.is_prank() || kind.is_aimed()) {
        return Err(Refusal::TargetIsSelf);
    }
    if kind.is_prank() {
        if t.is_owner {
            return Err(Refusal::OutOfReach);
        }
        match (t.top_role_position, t.bot_top_role_position) {
            (Some(top), Some(bot)) if top < bot => {},
            (Some(_), Some(_)) => return Err(Refusal::OutOfReach),
            // Not the hierarchy: a position nobody could read.
            _ => return Err(Refusal::CantTell),
        }
    }
    match kind {
        _ if voice == VoiceStep::Skip => {},
        ActionKind::Drag => {
            if buyer_in_voice.is_none() {
                return Err(Refusal::BuyerNotInVoice);
            }
            if t.in_voice.is_none() {
                return Err(Refusal::TargetNotInVoice);
            }
            // Nothing to move: a charge for nothing.
            if t.in_voice == buyer_in_voice {
                return Err(Refusal::AlreadyThere);
            }
        },
        ActionKind::Mute | ActionKind::Deafen | ActionKind::Jail if t.in_voice.is_none() => {
            return Err(Refusal::TargetNotInVoice);
        },
        _ => {},
    }
    // Whoever muted them owns it: the shop's undo would lift their mute. A
    // running shop mute of the same kind is the shop's own, and is refused
    // below as already running, with its end.
    let ours = t.running_same_kind.is_some();
    if kind == ActionKind::Mute && t.server_mute && !ours {
        return Err(Refusal::AlreadyMuted);
    }
    if kind == ActionKind::Deafen && t.server_deaf && !ours {
        return Err(Refusal::AlreadyDeafened);
    }
    if t.opted_out {
        return Err(Refusal::TargetOptedOut);
    }
    // A mirror answers pranks only: a theme, dedication or bounty neither
    // bounces nor uses it up.
    if kind.is_prank() && t.has_mirror {
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
fn top_of(roles: &[RoleId], position: impl Fn(RoleId) -> Option<u16>) -> u16 {
    roles.iter().filter_map(|r| position(*r)).max().unwrap_or(0)
}

/// The highest position among `roles`, placed by `position` (the server's
/// role positions; `None` for a role it doesn't know). `Some(0)` for no
/// roles; `None` when @everyone or any of the roles can't be placed, which
/// callers treat as "can't tell".
pub fn top_position_from(
    guild: GuildId,
    roles: &[RoleId],
    position: impl Fn(RoleId) -> Option<u16>,
) -> Option<u16> {
    if !roles_resolvable(guild, roles, |r| position(r).is_some()) {
        return None;
    }
    Some(top_of(roles, position))
}

/// A role's position in a cached server.
pub fn cached_position(g: &Guild, r: RoleId) -> Option<u16> {
    g.roles
        .get(&r)
        .map(|role| role.position.max(0).cast_unsigned())
}

/// A cached member's highest role position (the bot's own, which the cache
/// always holds); `None` when the cache cannot answer.
pub fn top_position(cache: &Cache, guild: GuildId, member: UserId) -> Option<u16> {
    let g = cache.guild(guild)?;
    let m = g.members.get(&member)?;
    top_position_from(guild, &m.roles, |r| cached_position(&g, r))
}

/// What a member's own record says about them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFacts {
    /// `None` when one of their roles can't be placed.
    pub top_role_position: Option<u16>,
    pub current_nick: Option<String>,
    /// Nickname, else global name, else username.
    pub display_name: String,
}

/// [`MemberFacts`] from the member as Discord resolved them (an interaction
/// carries their roles and nickname), placed by the server's role positions.
/// The member need not be in the cache: most members who only type never are.
pub fn member_facts(
    guild: GuildId,
    member: &Member,
    position: impl Fn(RoleId) -> Option<u16>,
) -> MemberFacts {
    let current_nick = member.nick.as_ref().map(ToString::to_string);
    let display_name = current_nick
        .clone()
        .or_else(|| member.user.global_name.as_ref().map(ToString::to_string))
        .unwrap_or_else(|| member.user.name.to_string());
    MemberFacts {
        top_role_position: top_position_from(guild, &member.roles, position),
        current_nick,
        display_name,
    }
}

/// Whether `member` owns the server (always out of reach). `false` when the
/// guild is not cached.
pub fn is_owner(cache: &Cache, guild: GuildId, member: UserId) -> bool {
    cache.guild(guild).is_some_and(|g| g.owner_id == member)
}

/// The voice channel a member is in, from the cache.
pub fn voice_of(cache: &Cache, guild: GuildId, member: UserId) -> Option<ChannelId> {
    cache
        .guild(guild)?
        .voice_states
        .get(&member)
        .and_then(|v| v.channel_id)
}

/// Whether a member is server-muted and server-deafened, from their cached
/// voice state; `(false, false)` when they have none.
pub fn voice_flags(cache: &Cache, guild: GuildId, member: UserId) -> (bool, bool) {
    cache
        .guild(guild)
        .and_then(|g| g.voice_states.get(&member).map(|v| (v.mute(), v.deaf())))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            bot_top_role_position: Some(5),
            in_voice: Some(VC),
            server_mute: false,
            server_deaf: false,
            opted_out: false,
            protection: None,
            has_mirror: false,
            running_same_kind: None,
            can_join_destination: Some(true),
            current_nick: None,
            display_name: "bob".to_string(),
        }
    }

    /// Everything wrong at once.
    fn all_wrong() -> TargetFacts {
        TargetFacts {
            is_bot: true,
            is_self: true,
            is_owner: true,
            top_role_position: Some(9),
            bot_top_role_position: Some(5),
            in_voice: None,
            server_mute: true,
            server_deaf: true,
            opted_out: true,
            protection: Some(Protection::Indefinitely),
            has_mirror: true,
            running_same_kind: Some(t0()),
            can_join_destination: Some(false),
            current_nick: None,
            display_name: "bob".to_string(),
        }
    }

    #[test]
    fn check_target_order() {
        let kind = ActionKind::Drag;
        let buyer = Some(OTHER_VC);
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
        // Already where the buyer is: nothing to drag.
        f.in_voice = buyer;
        assert_eq!(check_target(kind, &f, buyer), Err(Refusal::AlreadyThere));
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

        // Mute and deafen: an existing server mute or deafen comes right
        // after the voice needs, before opt-out and the mirror.
        for (kind, want) in [
            (ActionKind::Mute, Refusal::AlreadyMuted),
            (ActionKind::Deafen, Refusal::AlreadyDeafened),
        ] {
            let mut f = all_wrong();
            f.is_bot = false;
            f.is_self = false;
            f.is_owner = false;
            f.top_role_position = Some(1);
            assert_eq!(
                check_target(kind, &f, None),
                Err(Refusal::TargetNotInVoice),
                "{kind:?}"
            );
            f.in_voice = Some(VC);
            // A running shop prank of the same kind is ours, not a
            // moderator's: on to the later checks (it ends at AlreadyRunning).
            assert_eq!(
                check_target(kind, &f, None),
                Err(Refusal::TargetOptedOut),
                "{kind:?}"
            );
            f.running_same_kind = None;
            assert_eq!(check_target(kind, &f, None), Err(want), "{kind:?}");
            f.server_mute = false;
            f.server_deaf = false;
            assert_eq!(
                check_target(kind, &f, None),
                Err(Refusal::TargetOptedOut),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn dedicate_refusals() {
        let k = ActionKind::Dedicate;
        let refused = |f: TargetFacts| check_target(k, &f, None);
        assert_eq!(
            refused(TargetFacts {
                is_self: true,
                ..clean()
            }),
            Err(Refusal::TargetIsSelf)
        );
        assert_eq!(
            refused(TargetFacts {
                is_bot: true,
                ..clean()
            }),
            Err(Refusal::TargetIsBot)
        );
        assert_eq!(
            refused(TargetFacts {
                protection: Some(Protection::Indefinitely),
                ..clean()
            }),
            Err(Refusal::TargetImmune(Protection::Indefinitely))
        );
        assert_eq!(
            refused(TargetFacts {
                opted_out: true,
                ..clean()
            }),
            Err(Refusal::TargetOptedOut)
        );
        // A mirror holder is dedicated to like anyone: it lands, no bounce.
        assert_eq!(
            refused(TargetFacts {
                has_mirror: true,
                ..clean()
            }),
            Ok(TargetOutcome::Lands)
        );
    }

    #[test]
    fn check_target_aimed_kinds() {
        for kind in [ActionKind::Theme, ActionKind::Dedicate, ActionKind::Bounty] {
            // Yourself is refused, as for a prank.
            let me = TargetFacts {
                is_self: true,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &me, None),
                Err(Refusal::TargetIsSelf),
                "{kind:?}"
            );
            // No mirror: it lands, and the fact is not read.
            let mirrored = TargetFacts {
                has_mirror: true,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &mirrored, None),
                Ok(TargetOutcome::Lands),
                "{kind:?}"
            );
            // No reach check: above the bot, the owner, unknown positions.
            let above = TargetFacts {
                top_role_position: Some(9),
                bot_top_role_position: Some(5),
                is_owner: true,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &above, None),
                Ok(TargetOutcome::Lands),
                "{kind:?}"
            );
            let unknown = TargetFacts {
                top_role_position: None,
                bot_top_role_position: None,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &unknown, None),
                Ok(TargetOutcome::Lands),
                "{kind:?}"
            );
            // No voice needs.
            let away = TargetFacts {
                in_voice: None,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &away, None),
                Ok(TargetOutcome::Lands),
                "{kind:?}"
            );
            // Immune and opted out are refused, even behind a mirror.
            let immune = TargetFacts {
                protection: Some(Protection::Indefinitely),
                has_mirror: true,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &immune, None),
                Err(Refusal::TargetImmune(Protection::Indefinitely)),
                "{kind:?}"
            );
            let out = TargetFacts {
                opted_out: true,
                ..clean()
            };
            assert_eq!(
                check_target(kind, &out, None),
                Err(Refusal::TargetOptedOut),
                "{kind:?}"
            );
            // A second one of the same kind waits its turn.
            let busy = TargetFacts {
                running_same_kind: Some(t0()),
                ..clean()
            };
            assert_eq!(
                check_target(kind, &busy, None),
                Err(Refusal::AlreadyRunning { until: t0() }),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_running_shop_mute_keeps_its_own_refusal() {
        for kind in [ActionKind::Mute, ActionKind::Deafen] {
            // Muted (or deafened) by the shop's own running prank.
            let shop = TargetFacts {
                server_mute: true,
                server_deaf: true,
                running_same_kind: Some(t0()),
                ..clean()
            };
            assert_eq!(
                check_target(kind, &shop, None),
                Err(Refusal::AlreadyRunning { until: t0() }),
                "{kind:?}"
            );
            // By a moderator, with no shop row: theirs.
            let modded = TargetFacts {
                running_same_kind: None,
                ..shop
            };
            let want = if kind == ActionKind::Mute {
                Refusal::AlreadyMuted
            } else {
                Refusal::AlreadyDeafened
            };
            assert_eq!(check_target(kind, &modded, None), Err(want), "{kind:?}");
        }
    }

    #[test]
    fn a_mute_and_a_deafen_are_separate() {
        // Muted by a moderator: a deafen can still be bought, and the other
        // way round; and neither touches the name pranks.
        let mut f = clean();
        f.server_mute = true;
        assert_eq!(
            check_target(ActionKind::Deafen, &f, None),
            Ok(TargetOutcome::Lands)
        );
        assert_eq!(
            check_target(ActionKind::Rename, &f, None),
            Ok(TargetOutcome::Lands)
        );
        f.server_mute = false;
        f.server_deaf = true;
        assert_eq!(
            check_target(ActionKind::Mute, &f, None),
            Ok(TargetOutcome::Lands)
        );
        assert_eq!(
            check_target(ActionKind::Deafen, &f, None),
            Err(Refusal::AlreadyDeafened)
        );
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
            check_target(ActionKind::Drag, &f, Some(OTHER_VC)),
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
    fn an_unresolvable_role_position_cant_tell() {
        let pranks = [
            ActionKind::Rename,
            ActionKind::Lock,
            ActionKind::Clown,
            ActionKind::Drag,
            ActionKind::Mute,
            ActionKind::Deafen,
            ActionKind::Jail,
        ];
        // The target's position, or the bot's own, unknown: not the
        // hierarchy, so not "out of reach".
        for (top, bot) in [(None, Some(5)), (Some(1), None), (None, None)] {
            let f = TargetFacts {
                top_role_position: top,
                bot_top_role_position: bot,
                ..clean()
            };
            for k in pranks {
                assert_eq!(
                    check_target(k, &f, Some(OTHER_VC)),
                    Err(Refusal::CantTell),
                    "{k:?} {top:?} {bot:?}"
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
        // The owner is out of reach whatever is known.
        let owner = TargetFacts {
            is_owner: true,
            top_role_position: None,
            ..clean()
        };
        assert_eq!(
            check_target(ActionKind::Rename, &owner, None),
            Err(Refusal::OutOfReach)
        );
        // And the words say so.
        assert_eq!(
            crate::economy::shop::refusal_text(&Refusal::CantTell, "c"),
            "CrackTunes can't see that member right now. Try again in a moment."
        );
    }

    const SERVER: GuildId = GuildId::new(100);

    /// A member as an interaction resolves them, in no cache.
    fn resolved(id: u64, roles: &[u64], nick: Option<&str>) -> Member {
        let mut m = Member::default();
        m.user.id = UserId::new(id);
        m.user.name = serenity::small_fixed_array::FixedString::from_str_trunc("bobby");
        m.roles = roles
            .iter()
            .map(|r| RoleId::new(*r))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        m.nick = nick.map(serenity::small_fixed_array::FixedString::from_str_trunc);
        m
    }

    /// Only the server's roles are known: @everyone, a "member" role at 2, a
    /// "mod" role at 8.
    fn guild_roles(r: RoleId) -> Option<u16> {
        match r.get() {
            100 => Some(0),
            7 => Some(2),
            9 => Some(8),
            _ => None,
        }
    }

    #[test]
    fn facts_from_a_passed_in_member_are_not_out_of_reach() {
        let m = resolved(20, &[7], Some("Al"));
        let own = member_facts(SERVER, &m, guild_roles);
        assert_eq!(
            own,
            MemberFacts {
                top_role_position: Some(2),
                current_nick: Some("Al".into()),
                display_name: "Al".into(),
            }
        );
        let f = TargetFacts {
            top_role_position: own.top_role_position,
            current_nick: own.current_nick,
            display_name: own.display_name,
            ..clean()
        };
        assert_eq!(
            check_target(ActionKind::Rename, &f, None),
            Ok(TargetOutcome::Lands)
        );
        // Their highest role decides, and no nickname shows the username.
        let modded = member_facts(SERVER, &resolved(21, &[7, 9], None), guild_roles);
        assert_eq!(modded.top_role_position, Some(8));
        assert_eq!(modded.current_nick, None);
        assert_eq!(modded.display_name, "bobby");
        // No roles at all is the bottom, not unknown.
        assert_eq!(
            member_facts(SERVER, &resolved(22, &[], None), guild_roles).top_role_position,
            Some(0)
        );
    }

    #[test]
    fn a_role_the_server_cannot_place_cant_tell() {
        let own = member_facts(SERVER, &resolved(20, &[7, 55], None), guild_roles);
        assert_eq!(own.top_role_position, None);
        let f = TargetFacts {
            top_role_position: own.top_role_position,
            ..clean()
        };
        assert_eq!(
            check_target(ActionKind::Rename, &f, None),
            Err(Refusal::CantTell)
        );
        // Without @everyone the server's roles aren't loaded at all.
        assert_eq!(
            top_position_from(SERVER, &[RoleId::new(7)], |r| (r.get() == 7).then_some(2)),
            None
        );
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
    fn the_voice_step_can_be_skipped_and_nothing_else() {
        let away = TargetFacts {
            in_voice: None,
            ..clean()
        };
        for kind in [ActionKind::Mute, ActionKind::Deafen, ActionKind::Jail] {
            assert_eq!(
                check_target_with(kind, &away, None, VoiceStep::Apply),
                Err(Refusal::TargetNotInVoice),
                "{kind:?}"
            );
            assert_eq!(
                check_target(kind, &away, None),
                Err(Refusal::TargetNotInVoice)
            );
            assert_eq!(
                check_target_with(kind, &away, None, VoiceStep::Skip),
                Ok(TargetOutcome::Lands),
                "{kind:?}"
            );
            // Every other check still runs.
            let immune = TargetFacts {
                protection: Some(Protection::Indefinitely),
                ..away.clone()
            };
            assert_eq!(
                check_target_with(kind, &immune, None, VoiceStep::Skip),
                Err(Refusal::TargetImmune(Protection::Indefinitely))
            );
        }
    }
}
