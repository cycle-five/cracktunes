//! Which permissions CrackTunes is missing for the mod log, and whether
//! fetching is paused.
//!
//! [`preflight`] is pure; [`for_guild`] is the thin cache reader that feeds
//! it. An empty result means ready.

use crate::music::perms::roles_resolvable;
use crate::Data;
use serenity::all::{Cache, ChannelId, GuildId, Permissions};

/// What the bot needs server-wide: the audit log is not a channel thing.
pub const NEED_GUILD: Permissions = Permissions::VIEW_AUDIT_LOG;

/// What the bot needs in the log channel.
pub const NEED_CHANNEL: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::EMBED_LINKS);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModLogFinding {
    /// Missing server-wide (View Audit Log).
    MissingServer(Permissions),
    /// Missing in the log channel.
    MissingChannel {
        channel: ChannelId,
        perms: Permissions,
    },
    /// A 403 paused audit fetches.
    Suspended,
}

/// Findings in a stable order: server, channel, suspension. `channel` is the
/// configured log channel (none: nothing to check there); `bot_in_channel`
/// is the bot's permissions in it, `None` when the server has no such
/// channel (deleted), which misses everything there.
pub fn preflight(
    bot_guild: Permissions,
    bot_in_channel: Option<Permissions>,
    channel: Option<ChannelId>,
    suspended: bool,
) -> Vec<ModLogFinding> {
    let mut out = Vec::new();
    let missing = NEED_GUILD - bot_guild;
    if !missing.is_empty() {
        out.push(ModLogFinding::MissingServer(missing));
    }
    if let Some(channel) = channel {
        let perms = NEED_CHANNEL - bot_in_channel.unwrap_or_else(Permissions::empty);
        if !perms.is_empty() {
            out.push(ModLogFinding::MissingChannel { channel, perms });
        }
    }
    if suspended {
        out.push(ModLogFinding::Suspended);
    }
    out
}

/// Read the bot's permissions from the cache and run [`preflight`] for the
/// configured channel. `None` when the cache cannot answer (guild, bot
/// member or one of its roles not cached), rather than guessing from an
/// under-reported bitset. A configured channel the cached server does not
/// have is reported as missing everything there.
pub fn for_guild(cache: &Cache, data: &Data, guild: GuildId) -> Option<Vec<ModLogFinding>> {
    let bot_id = cache.current_user().id;
    let g = cache.guild(guild)?;
    let bot = g.members.get(&bot_id)?;
    if !roles_resolvable(guild, &bot.roles, |r| g.roles.get(&r).is_some()) {
        return None;
    }
    let bot_guild = g.member_permissions(bot);
    let channel = data.modlog.settings(guild).channel;
    let bot_in_channel = channel
        .and_then(|ch| g.channels.get(&ch))
        .map(|ch| g.user_permissions_in(ch, bot));
    let suspended = data.modlog.is_suspended(guild, tokio::time::Instant::now());
    Some(preflight(bot_guild, bot_in_channel, channel, suspended))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: ChannelId = ChannelId::new(55);

    fn all_channel() -> Option<Permissions> {
        Some(NEED_CHANNEL)
    }

    #[test]
    fn all_granted_is_ready() {
        assert_eq!(
            preflight(NEED_GUILD, all_channel(), Some(LOG), false),
            vec![]
        );
    }

    #[test]
    fn missing_view_audit_log_is_reported() {
        assert_eq!(
            preflight(
                Permissions::ADMINISTRATOR.complement() - NEED_GUILD,
                all_channel(),
                Some(LOG),
                false
            ),
            vec![ModLogFinding::MissingServer(Permissions::VIEW_AUDIT_LOG)]
        );
    }

    #[test]
    fn missing_channel_perms_are_reported_there() {
        assert_eq!(
            preflight(
                NEED_GUILD,
                Some(Permissions::VIEW_CHANNEL),
                Some(LOG),
                false
            ),
            vec![ModLogFinding::MissingChannel {
                channel: LOG,
                perms: Permissions::SEND_MESSAGES | Permissions::EMBED_LINKS,
            }]
        );
    }

    #[test]
    fn a_channel_the_server_lacks_misses_everything_there() {
        assert_eq!(
            preflight(NEED_GUILD, None, Some(LOG), false),
            vec![ModLogFinding::MissingChannel {
                channel: LOG,
                perms: NEED_CHANNEL,
            }]
        );
    }

    #[test]
    fn no_channel_is_nothing_to_check_there() {
        assert_eq!(preflight(NEED_GUILD, None, None, false), vec![]);
    }

    #[test]
    fn suspended_is_reported() {
        assert_eq!(
            preflight(NEED_GUILD, all_channel(), Some(LOG), true),
            vec![ModLogFinding::Suspended]
        );
    }

    #[test]
    fn findings_come_in_a_stable_order() {
        assert_eq!(
            preflight(Permissions::empty(), None, Some(LOG), true),
            vec![
                ModLogFinding::MissingServer(NEED_GUILD),
                ModLogFinding::MissingChannel {
                    channel: LOG,
                    perms: NEED_CHANNEL,
                },
                ModLogFinding::Suspended,
            ]
        );
    }
}
