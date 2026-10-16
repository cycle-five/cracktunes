//! A snapshot of what room decisions need from a server, read once per
//! interaction or voice event. Everything after the read is pure.

use super::guard::{member_can_join, BaselineView};
use super::state::StoredOverwrite;
use serenity::all::{Cache, ChannelId, GuildId, Http, Permissions, RoleId, UserId};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuildFacts {
    pub guild: GuildId,
    /// The server's owner, who holds every permission whatever their roles.
    pub owner: UserId,
    /// CrackTunes itself.
    pub bot: UserId,
    /// `@everyone`'s server-level permissions.
    pub everyone: Permissions,
    pub role_perms: HashMap<RoleId, Permissions>,
    /// Each known member's roles.
    pub roles: HashMap<UserId, Vec<RoleId>>,
    /// Who is in which voice channel, and whether they are a bot.
    pub voice: HashMap<UserId, (ChannelId, bool)>,
    pub display: HashMap<UserId, String>,
}

impl GuildFacts {
    /// The humans in `ch`, sorted. Bots, CrackTunes included, do not count.
    pub fn humans_in(&self, ch: ChannelId) -> Vec<UserId> {
        let mut out: Vec<UserId> = self
            .voice
            .iter()
            .filter(|(_, (c, is_bot))| *c == ch && !is_bot)
            .map(|(u, _)| *u)
            .collect();
        out.sort();
        out
    }

    pub fn in_room(&self, u: UserId, ch: ChannelId) -> bool {
        self.voice.get(&u).is_some_and(|(c, _)| *c == ch)
    }

    /// Server-level permissions: `@everyone` plus the member's roles, all of
    /// them for an administrator or the server's owner. A member we know
    /// nothing about has `@everyone`'s.
    pub fn guild_perms(&self, u: UserId) -> Permissions {
        if u == self.owner {
            return Permissions::all();
        }
        let mut perms = self.everyone;
        for r in self.roles.get(&u).into_iter().flatten() {
            perms |= self
                .role_perms
                .get(r)
                .copied()
                .unwrap_or_else(Permissions::empty);
        }
        if perms.contains(Permissions::ADMINISTRATOR) {
            Permissions::all()
        } else {
            perms
        }
    }

    /// The category baseline a room inherits, seen through these facts.
    pub fn baseline<'a>(&'a self, overwrites: &'a [StoredOverwrite]) -> BaselineView<'a> {
        BaselineView {
            guild: self.guild,
            everyone: self.everyone,
            role_perms: &self.role_perms,
            overwrites,
        }
    }

    /// Whether the baseline lets `u` join. The server's owner always can.
    pub fn member_can_join(&self, u: UserId, overwrites: &[StoredOverwrite]) -> bool {
        if u == self.owner {
            return true;
        }
        let roles = self.roles.get(&u).map(Vec::as_slice).unwrap_or(&[]);
        member_can_join(&self.baseline(overwrites), u, roles)
    }
}

/// Read the facts for `guild` from the cache. Members in `extra` the cache
/// does not hold are fetched over HTTP (a modal's target, say); one that
/// cannot be fetched is left out, and counts as holding only `@everyone`.
/// `None` when the guild is not cached.
pub async fn gather(
    cache: &Cache,
    http: &Http,
    guild: GuildId,
    extra: &[UserId],
) -> Option<GuildFacts> {
    let mut facts = {
        let g = cache.guild(guild)?;
        let everyone_id = RoleId::new(guild.get());
        let mut facts = GuildFacts {
            guild,
            owner: g.owner_id,
            bot: cache.current_user().id,
            everyone: g
                .roles
                .get(&everyone_id)
                .map(|r| r.permissions)
                .unwrap_or_else(Permissions::empty),
            role_perms: g.roles.iter().map(|r| (r.id, r.permissions)).collect(),
            roles: HashMap::new(),
            voice: HashMap::new(),
            display: HashMap::new(),
        };
        for m in g.members.iter() {
            facts.roles.insert(m.user.id, m.roles.to_vec());
            facts
                .display
                .insert(m.user.id, m.display_name().to_string());
        }
        for vs in g.voice_states.iter() {
            let Some(ch) = vs.channel_id else { continue };
            let member = vs.member.as_ref().or_else(|| g.members.get(&vs.user_id));
            let is_bot = member.is_some_and(|m| m.user.bot());
            if let Some(m) = member {
                facts
                    .roles
                    .entry(vs.user_id)
                    .or_insert_with(|| m.roles.to_vec());
                facts
                    .display
                    .entry(vs.user_id)
                    .or_insert_with(|| m.display_name().to_string());
            }
            facts.voice.insert(vs.user_id, (ch, is_bot));
        }
        facts
    };
    for u in extra {
        if facts.roles.contains_key(u) {
            continue;
        }
        match http.get_member(guild, *u).await {
            Ok(m) => {
                facts.roles.insert(*u, m.roles.to_vec());
                facts.display.insert(*u, m.display_name().to_string());
            },
            Err(err) => tracing::debug!("rooms: member {u} of {guild} not fetched: {err}"),
        }
    }
    Some(facts)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rooms::state::OverwriteTarget;

    pub(crate) const G: GuildId = GuildId::new(100);
    const MOD_ROLE: RoleId = RoleId::new(7);
    const ADMIN_ROLE: RoleId = RoleId::new(8);

    pub(crate) fn empty(guild: GuildId) -> GuildFacts {
        GuildFacts {
            guild,
            owner: UserId::new(77),
            bot: UserId::new(99),
            everyone: Permissions::VIEW_CHANNEL | Permissions::CONNECT,
            role_perms: HashMap::new(),
            roles: HashMap::new(),
            voice: HashMap::new(),
            display: HashMap::new(),
        }
    }

    fn facts() -> GuildFacts {
        let mut f = empty(G);
        f.role_perms = [
            (MOD_ROLE, Permissions::MOVE_MEMBERS),
            (ADMIN_ROLE, Permissions::ADMINISTRATOR),
        ]
        .into();
        f.roles = [
            (UserId::new(1), vec![MOD_ROLE]),
            (UserId::new(2), vec![ADMIN_ROLE]),
            (UserId::new(3), vec![RoleId::new(404)]),
        ]
        .into();
        f
    }

    #[test]
    fn humans_in_is_sorted_and_skips_bots_and_other_channels() {
        let mut f = facts();
        let (a, b) = (ChannelId::new(10), ChannelId::new(11));
        f.voice = [
            (UserId::new(50), (a, false)),
            (UserId::new(3), (a, false)),
            (UserId::new(41), (a, false)),
            (UserId::new(12), (a, false)),
            (UserId::new(7), (a, false)),
            (UserId::new(99), (a, true)),
            (UserId::new(4), (b, false)),
        ]
        .into();
        // Five humans: a HashMap order that happens to be sorted is 1 in 120.
        let want: Vec<UserId> = [3, 7, 12, 41, 50].map(UserId::new).into();
        assert_eq!(f.humans_in(a), want);
        assert!(f.in_room(UserId::new(99), a), "a bot is still in the room");
        assert!(!f.in_room(UserId::new(4), a));
        assert!(!f.in_room(UserId::new(6), a), "not in voice at all");
        assert!(f.humans_in(ChannelId::new(12)).is_empty());
    }

    #[test]
    fn guild_perms_unions_roles_and_admin_or_owner_get_everything() {
        let f = facts();
        let base = Permissions::VIEW_CHANNEL | Permissions::CONNECT;
        assert_eq!(
            f.guild_perms(UserId::new(1)),
            base | Permissions::MOVE_MEMBERS
        );
        assert_eq!(f.guild_perms(UserId::new(2)), Permissions::all());
        assert_eq!(
            f.guild_perms(UserId::new(3)),
            base,
            "an unknown role is empty"
        );
        assert_eq!(f.guild_perms(UserId::new(6)), base, "an unknown member");
        assert_eq!(f.guild_perms(f.owner), Permissions::all());
    }

    #[test]
    fn member_can_join_follows_the_baseline_and_passes_the_owner() {
        let mut f = facts();
        f.everyone = Permissions::empty();
        let adult = RoleId::new(18);
        f.role_perms.insert(adult, Permissions::empty());
        f.roles.insert(UserId::new(4), vec![adult]);
        let baseline = [StoredOverwrite {
            target: OverwriteTarget::Role { id: adult },
            allow: Permissions::VIEW_CHANNEL | Permissions::CONNECT,
            deny: Permissions::empty(),
        }];
        assert!(f.member_can_join(UserId::new(4), &baseline));
        assert!(!f.member_can_join(UserId::new(3), &baseline));
        assert!(!f.member_can_join(UserId::new(6), &baseline), "unknown");
        assert!(f.member_can_join(f.owner, &baseline));
    }
}
