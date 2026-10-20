//! Which permissions CrackTunes lacks for each shop action.
//!
//! [`needs`] and [`missing`] are pure; [`bot_perms`] is the thin cache reader
//! that feeds them. Empty means the action can be sold.

use super::settings::ActionKind;
use crate::music::perms::roles_resolvable;
use serenity::all::{Cache, ChannelId, GuildId, Permissions};
use std::collections::HashMap;

/// What a channel must offer the bot for drag and jail.
pub const NEED_CHANNEL: Permissions = Permissions::VIEW_CHANNEL.union(Permissions::CONNECT);

/// The bot's computed permissions: server-wide, and in the channels asked about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BotPerms {
    pub guild: Permissions,
    pub in_channel: HashMap<ChannelId, Permissions>,
}

/// The server-wide permissions an action needs. Cleanse undoes pranks with
/// the permissions they already needed, so it needs none of its own.
pub fn needs(kind: ActionKind) -> Permissions {
    match kind {
        ActionKind::Rename | ActionKind::Clown => Permissions::MANAGE_NICKNAMES,
        ActionKind::Lock => Permissions::MANAGE_NICKNAMES | Permissions::VIEW_AUDIT_LOG,
        ActionKind::Drag | ActionKind::Jail => Permissions::MOVE_MEMBERS,
        ActionKind::Mute => Permissions::MUTE_MEMBERS,
        ActionKind::Deafen => Permissions::DEAFEN_MEMBERS,
        ActionKind::Immunity | ActionKind::Mirror | ActionKind::Cleanse | ActionKind::Reveal => {
            Permissions::empty()
        },
    }
}

/// What is missing, as `(where, permissions)`: `None` is server-wide. Server
/// first, then `channels` in the order given. `channels` matter for drag
/// (both) and jail (the jail channel); other kinds ignore them. A channel
/// absent from `perms` misses everything there.
pub fn missing(
    kind: ActionKind,
    perms: &BotPerms,
    channels: &[ChannelId],
) -> Vec<(Option<ChannelId>, Permissions)> {
    let mut out = Vec::new();
    let gap = needs(kind) - perms.guild;
    if !gap.is_empty() {
        out.push((None, gap));
    }
    if matches!(kind, ActionKind::Drag | ActionKind::Jail) {
        for &ch in channels {
            let have = perms
                .in_channel
                .get(&ch)
                .copied()
                .unwrap_or_else(Permissions::empty);
            let gap = NEED_CHANNEL - have;
            if !gap.is_empty() {
                out.push((Some(ch), gap));
            }
        }
    }
    out
}

/// Whether the bot lacks View Audit Log, so a lock cannot tell who changed a
/// nickname.
pub fn lock_needs_audit_log(perms: &BotPerms) -> bool {
    !perms.guild.contains(Permissions::VIEW_AUDIT_LOG)
}

/// Read the bot's permissions from the cache. `None` when the cache cannot
/// answer (guild, bot member or one of its roles not cached), rather than
/// guessing from an under-reported bitset. A channel the cached server does
/// not have is left out, which [`missing`] reads as missing everything.
pub fn bot_perms(cache: &Cache, guild: GuildId, channels: &[ChannelId]) -> Option<BotPerms> {
    let bot_id = cache.current_user().id;
    let g = cache.guild(guild)?;
    let bot = g.members.get(&bot_id)?;
    if !roles_resolvable(guild, &bot.roles, |r| g.roles.get(&r).is_some()) {
        return None;
    }
    let in_channel = channels
        .iter()
        .filter_map(|ch| {
            g.channels
                .get(ch)
                .map(|c| (*ch, g.user_permissions_in(c, bot)))
        })
        .collect();
    Some(BotPerms {
        guild: g.member_permissions(bot),
        in_channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: ChannelId = ChannelId::new(10);
    const B: ChannelId = ChannelId::new(11);

    fn perms(guild: Permissions, chans: &[(ChannelId, Permissions)]) -> BotPerms {
        BotPerms {
            guild,
            in_channel: chans.iter().copied().collect(),
        }
    }

    #[test]
    fn needs_per_kind() {
        use ActionKind::*;
        let nick = Permissions::MANAGE_NICKNAMES;
        assert_eq!(needs(Rename), nick);
        assert_eq!(needs(Clown), nick);
        assert_eq!(needs(Lock), nick | Permissions::VIEW_AUDIT_LOG);
        assert_eq!(needs(Drag), Permissions::MOVE_MEMBERS);
        assert_eq!(needs(Jail), Permissions::MOVE_MEMBERS);
        assert_eq!(needs(Mute), Permissions::MUTE_MEMBERS);
        assert_eq!(needs(Deafen), Permissions::DEAFEN_MEMBERS);
        for k in [Immunity, Mirror, Cleanse, Reveal] {
            assert_eq!(needs(k), Permissions::empty(), "{k:?}");
        }
    }

    #[test]
    fn missing_reports_guild_and_channel_gaps() {
        let full = NEED_CHANNEL;
        let ok = perms(Permissions::MOVE_MEMBERS, &[(A, full), (B, full)]);
        assert_eq!(missing(ActionKind::Drag, &ok, &[A, B]), vec![]);

        // Server gap and a channel gap, server first.
        let p = perms(
            Permissions::empty(),
            &[(A, Permissions::VIEW_CHANNEL), (B, full)],
        );
        assert_eq!(
            missing(ActionKind::Drag, &p, &[A, B]),
            vec![
                (None, Permissions::MOVE_MEMBERS),
                (Some(A), Permissions::CONNECT),
            ]
        );
        // A channel the bot has no entry for misses both bits.
        assert_eq!(
            missing(ActionKind::Jail, &ok, &[ChannelId::new(99)]),
            vec![(Some(ChannelId::new(99)), NEED_CHANNEL)]
        );
        // Kinds with no channel needs ignore the channels.
        let m = perms(Permissions::MUTE_MEMBERS, &[]);
        assert_eq!(missing(ActionKind::Mute, &m, &[A]), vec![]);
        // Nothing needed, nothing missing.
        assert_eq!(
            missing(ActionKind::Cleanse, &BotPerms::default(), &[A]),
            vec![]
        );
    }

    #[test]
    fn lock_needs_view_audit_log() {
        let nick = Permissions::MANAGE_NICKNAMES;
        assert!(lock_needs_audit_log(&perms(nick, &[])));
        assert!(!lock_needs_audit_log(&perms(
            nick | Permissions::VIEW_AUDIT_LOG,
            &[]
        )));
        // And the audit log is part of what a lock needs.
        assert_eq!(
            missing(ActionKind::Lock, &perms(nick, &[]), &[]),
            vec![(None, Permissions::VIEW_AUDIT_LOG)]
        );
    }
}
