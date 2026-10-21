//! Which permissions CrackTunes lacks for each shop action.
//!
//! [`needs`], [`missing`] and [`economy_preflight`] are pure; [`bot_perms`] is
//! the thin cache reader that feeds them. Empty means the action can be sold.
//! The dashboard, `/economy status` and `/shop menu` all show
//! [`economy_preflight`]'s report.

use super::settings::{ActionKind, EconomyConfig};
use crate::messaging::messages::{
    DASH_ECONOMY_FINDING_GIVE, DASH_ECONOMY_FINDING_IN, DASH_ECONOMY_FINDING_SERVER,
    DASH_ECONOMY_PREFLIGHT_NO_JAIL,
};
use crate::music::perms::roles_resolvable;
use crate::rooms::card::perm_names;
use serde::Serialize;
use serenity::all::{Cache, ChannelId, GuildId, Permissions};
use std::collections::HashMap;

/// What a channel must offer the bot for drag and jail.
pub const NEED_CHANNEL: Permissions = Permissions::VIEW_CHANNEL.union(Permissions::CONNECT);

/// What the announcement channel must offer the bot for cards (spec 7).
pub const NEED_CARD: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::EMBED_LINKS);

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
        ActionKind::Immunity
        | ActionKind::Mirror
        | ActionKind::Cleanse
        | ActionKind::Reveal
        | ActionKind::Theme
        | ActionKind::Dedicate
        | ActionKind::Jump
        | ActionKind::Bounty
        | ActionKind::Crowdfund => Permissions::empty(),
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

/// What one prank still needs, in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionPreflight {
    pub kind: String,
    pub label: String,
    /// `None`: could not be checked. Empty: ready.
    pub needs: Option<Vec<String>>,
}

/// The per-prank permission report in words, and the announcement channel's
/// card gaps. `perms` is `None` when the cache cannot answer, which reads
/// as "can't tell", never as ready. Drag's channels depend on the purchase,
/// so only its server-wide needs are reported.
pub fn economy_preflight(
    c: &EconomyConfig,
    perms: Option<&BotPerms>,
    name_of: impl Fn(ChannelId) -> String,
) -> (Vec<ActionPreflight>, Option<Vec<String>>) {
    let line = |at: Option<ChannelId>, p: Permissions| match at {
        None => format!(
            "{DASH_ECONOMY_FINDING_GIVE} {} {DASH_ECONOMY_FINDING_SERVER}",
            perm_names(p, false)
        ),
        Some(ch) => format!(
            "{DASH_ECONOMY_FINDING_GIVE} {} {DASH_ECONOMY_FINDING_IN} {}",
            perm_names(p, false),
            name_of(ch)
        ),
    };
    let actions = ActionKind::PRANKS
        .iter()
        .map(|&k| {
            let channels: Vec<ChannelId> = if k == ActionKind::Jail {
                c.jail_channel_id.into_iter().collect()
            } else {
                Vec::new()
            };
            let mut needs = perms.map(|p| {
                missing(k, p, &channels)
                    .into_iter()
                    .map(|(at, gap)| line(at, gap))
                    .collect::<Vec<_>>()
            });
            if k == ActionKind::Jail && c.jail_channel_id.is_none() {
                needs
                    .get_or_insert_with(Vec::new)
                    .push(DASH_ECONOMY_PREFLIGHT_NO_JAIL.to_owned());
            }
            ActionPreflight {
                kind: k.key().to_owned(),
                label: k.label().to_owned(),
                needs,
            }
        })
        .collect();
    let announce = c.announce_channel_id.zip(perms).map(|(ch, p)| {
        let have = p
            .in_channel
            .get(&ch)
            .copied()
            .unwrap_or_else(Permissions::empty);
        let gap = NEED_CARD - have;
        if gap.is_empty() {
            Vec::new()
        } else {
            vec![line(Some(ch), gap)]
        }
    });
    (actions, announce)
}

/// [`economy_preflight`] for a server, from the cache, naming channels as
/// Discord mentions (for messages; the dashboard names them itself).
pub fn report_for(
    cache: &Cache,
    guild: GuildId,
    c: &EconomyConfig,
) -> (Vec<ActionPreflight>, Option<Vec<String>>) {
    let channels: Vec<ChannelId> = c
        .jail_channel_id
        .into_iter()
        .chain(c.announce_channel_id)
        .collect();
    let perms = bot_perms(cache, guild, &channels);
    economy_preflight(c, perms.as_ref(), |ch| format!("<#{ch}>"))
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
        for k in [
            Immunity, Mirror, Cleanse, Reveal, Theme, Dedicate, Jump, Bounty, Crowdfund,
        ] {
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

    fn all_perms() -> BotPerms {
        BotPerms {
            guild: Permissions::MANAGE_NICKNAMES
                | Permissions::VIEW_AUDIT_LOG
                | Permissions::MOVE_MEMBERS
                | Permissions::MUTE_MEMBERS
                | Permissions::DEAFEN_MEMBERS,
            in_channel: [(ChannelId::new(111), NEED_CHANNEL)].into_iter().collect(),
        }
    }

    fn needs_of(p: &[ActionPreflight], key: &str) -> Option<Vec<String>> {
        p.iter().find(|a| a.kind == key).unwrap().needs.clone()
    }

    #[test]
    fn preflight_says_ready_or_what_is_missing_per_prank() {
        let mut c = EconomyConfig::default();
        c.jail_channel_id = Some(ChannelId::new(111));
        let (all, _) = economy_preflight(&c, Some(&all_perms()), |ch| format!("#{ch}"));
        let kinds: Vec<_> = all.iter().map(|a| a.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["rename", "lock", "clown", "drag", "mute", "deafen", "jail"]
        );
        assert!(all.iter().all(|a| a.needs == Some(vec![])), "{all:?}");

        // No View Audit Log: only lock is short.
        let mut p = all_perms();
        p.guild -= Permissions::VIEW_AUDIT_LOG;
        let (got, _) = economy_preflight(&c, Some(&p), |ch| format!("#{ch}"));
        assert_eq!(
            needs_of(&got, "lock"),
            Some(vec![
                "Give CrackTunes View Audit Log in Server Settings → Roles".to_owned()
            ])
        );
        assert_eq!(needs_of(&got, "rename"), Some(vec![]));

        // The jail channel misses Connect: named by channel.
        let mut p = all_perms();
        p.in_channel
            .insert(ChannelId::new(111), Permissions::VIEW_CHANNEL);
        let (got, _) = economy_preflight(&c, Some(&p), |ch| format!("#{ch}"));
        assert_eq!(
            needs_of(&got, "jail"),
            Some(vec!["Give CrackTunes Connect in #111".to_owned()])
        );
        // Drag's channels are unknown, so it shows guild needs only.
        assert_eq!(needs_of(&got, "drag"), Some(vec![]));
    }

    #[test]
    fn preflight_without_a_jail_channel_or_a_cache() {
        let c = EconomyConfig::default();
        let (got, ann) = economy_preflight(&c, Some(&all_perms()), |ch| format!("#{ch}"));
        assert_eq!(
            needs_of(&got, "jail"),
            Some(vec!["Choose a jail channel first.".to_owned()])
        );
        assert_eq!(ann, None);
        // No cache: can't tell (None), never ready; jail still says why.
        let (got, _) = economy_preflight(&c, None, |ch| format!("#{ch}"));
        assert_eq!(needs_of(&got, "rename"), None);
        assert_eq!(needs_of(&got, "lock"), None);
        assert_eq!(
            needs_of(&got, "jail"),
            Some(vec!["Choose a jail channel first.".to_owned()])
        );
    }

    #[test]
    fn preflight_checks_the_announce_channel_for_cards() {
        let mut c = EconomyConfig::default();
        c.announce_channel_id = Some(ChannelId::new(222));
        let mut p = all_perms();
        p.in_channel.insert(
            ChannelId::new(222),
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
        );
        let (_, ann) = economy_preflight(&c, Some(&p), |ch| format!("#{ch}"));
        assert_eq!(
            ann,
            Some(vec!["Give CrackTunes Embed Links in #222".to_owned()])
        );
        p.in_channel.insert(ChannelId::new(222), NEED_CARD);
        let (_, ann) = economy_preflight(&c, Some(&p), |ch| format!("#{ch}"));
        assert_eq!(ann, Some(vec![]));
        let (_, ann) = economy_preflight(&c, None, |ch| format!("#{ch}"));
        assert_eq!(ann, None);
    }

    #[test]
    fn lock_needs_view_audit_log() {
        let nick = Permissions::MANAGE_NICKNAMES;
        assert_eq!(
            missing(ActionKind::Lock, &perms(nick, &[]), &[]),
            vec![(None, Permissions::VIEW_AUDIT_LOG)]
        );
        assert_eq!(
            missing(
                ActionKind::Lock,
                &perms(nick | Permissions::VIEW_AUDIT_LOG, &[]),
                &[]
            ),
            vec![]
        );
    }
}
