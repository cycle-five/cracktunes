//! Which permissions CrackTunes is missing for rooms to work, and where.
//!
//! [`preflight`] is pure; [`for_guild`] is the thin cache reader that feeds
//! it. An empty result means ready.

use super::overwrites::MANAGED;
use super::state::{OverwriteTarget, StoredOverwrite};
use crack_common::perms::roles_resolvable;
use serenity::all::{Cache, ChannelId, GuildId, Permissions};

/// What the bot needs in the hub's category (or server-wide with no category).
pub const NEED: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::CONNECT)
    .union(Permissions::MANAGE_CHANNELS)
    .union(Permissions::MANAGE_ROLES)
    .union(Permissions::MOVE_MEMBERS)
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::EMBED_LINKS);

/// What the bot needs in the hub channel itself.
pub const NEED_IN_HUB: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::CONNECT)
    .union(Permissions::MOVE_MEMBERS);

/// Nice to have; rooms work without it.
pub const OPTIONAL: Permissions = Permissions::PIN_MESSAGES.union(Permissions::MUTE_MEMBERS);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Category(ChannelId),
    Hub(ChannelId),
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    Missing { at: Where, perms: Permissions },
    Optional { at: Where, perms: Permissions },
    LockMayFail { perms: Permissions },
}

pub struct PreflightInput {
    pub hub: ChannelId,
    pub category: Option<ChannelId>,
    pub bot_in_category: Permissions,
    pub bot_in_hub: Permissions,
    pub category_overwrites: Vec<StoredOverwrite>,
}

/// Findings in a stable order: Missing (category/server, then hub), Optional,
/// LockMayFail. Empty means ready.
pub fn preflight(i: &PreflightInput) -> Vec<Finding> {
    let at = match i.category {
        Some(c) => Where::Category(c),
        None => Where::Server,
    };
    let mut out = Vec::new();

    let missing = NEED - i.bot_in_category;
    if !missing.is_empty() {
        out.push(Finding::Missing { at, perms: missing });
    }
    let missing_hub = NEED_IN_HUB - i.bot_in_hub;
    if !missing_hub.is_empty() {
        out.push(Finding::Missing {
            at: Where::Hub(i.hub),
            perms: missing_hub,
        });
    }
    let optional = OPTIONAL - i.bot_in_category;
    if !optional.is_empty() {
        out.push(Finding::Optional {
            at,
            perms: optional,
        });
    }

    // A lock rewrites role overwrites; bits the category baseline touches that
    // the bot itself lacks are bits it may be unable to set.
    let touched = i
        .category_overwrites
        .iter()
        .filter(|o| matches!(o.target, OverwriteTarget::Role { .. }))
        .fold(Permissions::empty(), |acc, o| acc | o.allow | o.deny);
    let lock = touched - i.bot_in_category - MANAGED;
    if !lock.is_empty() {
        out.push(Finding::LockMayFail { perms: lock });
    }
    out
}

/// Read the bot's permissions from the cache and run [`preflight`]. `None`
/// when the cache cannot answer (guild, bot member, hub, category or a role is
/// not cached), rather than guessing from an under-reported bitset.
pub fn for_guild(cache: &Cache, guild: GuildId, hub: ChannelId) -> Option<Vec<Finding>> {
    let bot_id = cache.current_user().id;
    let g = cache.guild(guild)?;
    let bot = g.members.get(&bot_id)?;
    if !roles_resolvable(guild, &bot.roles, |r| g.roles.get(&r).is_some()) {
        return None;
    }
    let hub_chan = g.channels.get(&hub)?;
    let bot_in_hub = g.user_permissions_in(hub_chan, bot);
    let category = hub_chan.parent_id;
    let (bot_in_category, category_overwrites) = match category {
        Some(cid) => {
            let cat = g.channels.get(&cid)?;
            (
                g.user_permissions_in(cat, bot),
                cat.permission_overwrites
                    .iter()
                    .map(StoredOverwrite::from)
                    .collect(),
            )
        },
        None => (g.member_permissions(bot), Vec::new()),
    };
    Some(preflight(&PreflightInput {
        hub,
        category,
        bot_in_category,
        bot_in_hub,
        category_overwrites,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::RoleId;

    const HUB: ChannelId = ChannelId::new(10);
    const CAT: ChannelId = ChannelId::new(20);

    fn input() -> PreflightInput {
        PreflightInput {
            hub: HUB,
            category: Some(CAT),
            bot_in_category: NEED | OPTIONAL,
            bot_in_hub: NEED_IN_HUB,
            category_overwrites: vec![],
        }
    }

    fn role(allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target: OverwriteTarget::Role { id: RoleId::new(5) },
            allow,
            deny,
        }
    }

    #[test]
    fn all_granted_is_ready() {
        assert_eq!(preflight(&input()), vec![]);
    }

    #[test]
    fn missing_move_members_in_category_is_named_there() {
        let mut i = input();
        i.bot_in_category -= Permissions::MOVE_MEMBERS;
        assert_eq!(
            preflight(&i),
            vec![Finding::Missing {
                at: Where::Category(CAT),
                perms: Permissions::MOVE_MEMBERS
            }]
        );
    }

    #[test]
    fn missing_in_hub_is_reported_separately() {
        let mut i = input();
        i.bot_in_hub = Permissions::VIEW_CHANNEL;
        assert_eq!(
            preflight(&i),
            vec![Finding::Missing {
                at: Where::Hub(HUB),
                perms: Permissions::CONNECT | Permissions::MOVE_MEMBERS
            }]
        );
    }

    #[test]
    fn no_category_reports_server() {
        let mut i = input();
        i.category = None;
        i.bot_in_category = Permissions::empty();
        assert_eq!(
            preflight(&i),
            vec![
                Finding::Missing {
                    at: Where::Server,
                    perms: NEED
                },
                Finding::Optional {
                    at: Where::Server,
                    perms: OPTIONAL
                }
            ]
        );
    }

    #[test]
    fn mute_members_is_optional() {
        let mut i = input();
        i.bot_in_category -= Permissions::MUTE_MEMBERS;
        assert_eq!(
            preflight(&i),
            vec![Finding::Optional {
                at: Where::Category(CAT),
                perms: Permissions::MUTE_MEMBERS
            }]
        );
    }

    #[test]
    fn pin_is_only_optional() {
        let mut i = input();
        i.bot_in_category -= Permissions::PIN_MESSAGES;
        assert_eq!(
            preflight(&i),
            vec![Finding::Optional {
                at: Where::Category(CAT),
                perms: Permissions::PIN_MESSAGES
            }]
        );
    }

    #[test]
    fn baseline_bit_the_bot_lacks_warns_lock_may_fail() {
        let mut i = input();
        i.category_overwrites = vec![role(Permissions::MANAGE_MESSAGES, Permissions::empty())];
        assert_eq!(
            preflight(&i),
            vec![Finding::LockMayFail {
                perms: Permissions::MANAGE_MESSAGES
            }]
        );
    }

    #[test]
    fn managed_bits_never_warn() {
        let mut i = input();
        i.bot_in_category -= Permissions::SPEAK;
        i.category_overwrites = vec![role(MANAGED, MANAGED)];
        assert_eq!(preflight(&i), vec![]);
    }

    #[test]
    fn member_overwrites_do_not_warn() {
        let mut i = input();
        i.category_overwrites = vec![StoredOverwrite {
            target: OverwriteTarget::Member {
                id: serenity::all::UserId::new(7),
            },
            allow: Permissions::MANAGE_MESSAGES,
            deny: Permissions::empty(),
        }];
        assert_eq!(preflight(&i), vec![]);
    }

    #[test]
    fn deny_bits_count_toward_lock_warning() {
        let mut i = input();
        i.category_overwrites = vec![role(Permissions::empty(), Permissions::ADD_REACTIONS)];
        assert_eq!(
            preflight(&i),
            vec![Finding::LockMayFail {
                perms: Permissions::ADD_REACTIONS
            }]
        );
    }

    #[test]
    fn findings_come_in_a_stable_order() {
        let i = PreflightInput {
            hub: HUB,
            category: Some(CAT),
            bot_in_category: Permissions::VIEW_CHANNEL,
            bot_in_hub: Permissions::empty(),
            category_overwrites: vec![role(Permissions::MANAGE_MESSAGES, Permissions::empty())],
        };
        assert_eq!(
            preflight(&i),
            vec![
                Finding::Missing {
                    at: Where::Category(CAT),
                    perms: NEED - Permissions::VIEW_CHANNEL
                },
                Finding::Missing {
                    at: Where::Hub(HUB),
                    perms: NEED_IN_HUB
                },
                Finding::Optional {
                    at: Where::Category(CAT),
                    perms: OPTIONAL
                },
                Finding::LockMayFail {
                    perms: Permissions::MANAGE_MESSAGES
                },
            ]
        );
    }
}
