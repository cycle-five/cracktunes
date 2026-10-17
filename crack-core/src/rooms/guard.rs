//! Would the server's own category permissions let a member or role into a
//! room? This is the guard that keeps an 18+ category gated: a room owner's
//! allow-list can never admit someone the category keeps out.

use super::state::{OverwriteTarget, StoredOverwrite};
use serenity::all::{GuildId, Permissions, RoleId, UserId};
use std::collections::HashMap;

/// The category baseline a room inherits.
pub struct BaselineView<'a> {
    pub guild: GuildId,
    /// `@everyone`'s guild-level permissions.
    pub everyone: Permissions,
    pub role_perms: &'a HashMap<RoleId, Permissions>,
    pub overwrites: &'a [StoredOverwrite],
}

/// Discord's documented algorithm. `roles` are the member's roles (the
/// implicit `@everyone` role is accounted for separately); a role missing from
/// `role_perms` counts as empty.
pub fn member_permissions(v: &BaselineView<'_>, user: UserId, roles: &[RoleId]) -> Permissions {
    permissions_for(v, Some(user), roles)
}

fn permissions_for(v: &BaselineView<'_>, user: Option<UserId>, roles: &[RoleId]) -> Permissions {
    let everyone_id = RoleId::new(v.guild.get());
    let roles: Vec<RoleId> = roles
        .iter()
        .copied()
        .filter(|r| *r != everyone_id)
        .collect();

    let mut perms = v.everyone;
    for r in &roles {
        perms |= v
            .role_perms
            .get(r)
            .copied()
            .unwrap_or_else(Permissions::empty);
    }
    if perms.contains(Permissions::ADMINISTRATOR) {
        return Permissions::all();
    }

    for o in v.overwrites {
        if o.target == (OverwriteTarget::Role { id: everyone_id }) {
            perms &= !o.deny;
            perms |= o.allow;
        }
    }

    let mut deny = Permissions::empty();
    let mut allow = Permissions::empty();
    for o in v.overwrites {
        if let OverwriteTarget::Role { id } = o.target {
            if roles.contains(&id) {
                deny |= o.deny;
                allow |= o.allow;
            }
        }
    }
    perms &= !deny;
    perms |= allow;

    for o in v.overwrites {
        if user.is_some_and(|id| o.target == (OverwriteTarget::Member { id })) {
            perms &= !o.deny;
            perms |= o.allow;
        }
    }
    perms
}

/// Connecting needs to see the channel as well as the connect bit.
fn can_join(p: Permissions) -> bool {
    p.contains(Permissions::VIEW_CHANNEL | Permissions::CONNECT)
}

pub fn member_can_join(v: &BaselineView<'_>, user: UserId, roles: &[RoleId]) -> bool {
    can_join(member_permissions(v, user, roles))
}

/// Whether a member holding only `role` (and `@everyone`) could join.
pub fn role_can_connect(v: &BaselineView<'_>, role: RoleId) -> bool {
    can_join(permissions_for(v, None, &[role]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUILD: GuildId = GuildId::new(100);
    const ADULT: RoleId = RoleId::new(7);
    const OTHER: RoleId = RoleId::new(8);
    const ME: UserId = UserId::new(1);

    fn everyone_role() -> RoleId {
        RoleId::new(GUILD.get())
    }

    fn ow(target: OverwriteTarget, allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target,
            allow,
            deny,
        }
    }

    fn base() -> Permissions {
        Permissions::VIEW_CHANNEL | Permissions::CONNECT
    }

    #[test]
    fn admin_role_gets_everything() {
        let admin = RoleId::new(50);
        let roles: HashMap<_, _> = [(admin, Permissions::ADMINISTRATOR)].into();
        let ovs = [ow(
            OverwriteTarget::Role {
                id: everyone_role(),
            },
            Permissions::empty(),
            base(),
        )];
        let v = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs,
        };
        assert_eq!(member_permissions(&v, ME, &[admin]), Permissions::all());
        assert!(member_can_join(&v, ME, &[admin]));
    }

    #[test]
    fn member_overwrite_beats_role_overwrites() {
        let roles: HashMap<_, _> = [(OTHER, Permissions::empty())].into();
        let ovs = [
            ow(
                OverwriteTarget::Role { id: OTHER },
                Permissions::empty(),
                Permissions::CONNECT,
            ),
            ow(
                OverwriteTarget::Member { id: ME },
                Permissions::CONNECT,
                Permissions::empty(),
            ),
        ];
        let v = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs,
        };
        assert!(member_permissions(&v, ME, &[OTHER]).contains(Permissions::CONNECT));
        let ovs2 = [
            ow(
                OverwriteTarget::Role { id: OTHER },
                Permissions::CONNECT,
                Permissions::empty(),
            ),
            ow(
                OverwriteTarget::Member { id: ME },
                Permissions::empty(),
                Permissions::CONNECT,
            ),
        ];
        let v2 = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs2,
        };
        assert!(!member_permissions(&v2, ME, &[OTHER]).contains(Permissions::CONNECT));
    }

    #[test]
    fn another_users_member_overwrite_is_ignored() {
        let roles: HashMap<_, _> = HashMap::new();
        let ovs = [ow(
            OverwriteTarget::Member {
                id: UserId::new(999),
            },
            Permissions::empty(),
            Permissions::CONNECT,
        )];
        let v = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs,
        };
        assert!(member_permissions(&v, ME, &[]).contains(Permissions::CONNECT));
        assert!(!member_permissions(&v, UserId::new(999), &[]).contains(Permissions::CONNECT));
    }

    #[test]
    fn role_allow_beats_everyone_deny() {
        let roles: HashMap<_, _> = [(ADULT, Permissions::empty())].into();
        let ovs = [
            ow(
                OverwriteTarget::Role {
                    id: everyone_role(),
                },
                Permissions::empty(),
                Permissions::VIEW_CHANNEL,
            ),
            ow(
                OverwriteTarget::Role { id: ADULT },
                Permissions::VIEW_CHANNEL,
                Permissions::empty(),
            ),
        ];
        let v = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs,
        };
        assert!(member_permissions(&v, ME, &[ADULT]).contains(Permissions::VIEW_CHANNEL));
        assert!(!member_permissions(&v, ME, &[]).contains(Permissions::VIEW_CHANNEL));
    }

    #[test]
    fn role_allow_beats_role_deny_when_combined() {
        // One role denies, another allows: the allow union wins (applied last).
        let roles: HashMap<_, _> =
            [(ADULT, Permissions::empty()), (OTHER, Permissions::empty())].into();
        let ovs = [
            ow(
                OverwriteTarget::Role { id: ADULT },
                Permissions::empty(),
                Permissions::CONNECT,
            ),
            ow(
                OverwriteTarget::Role { id: OTHER },
                Permissions::CONNECT,
                Permissions::empty(),
            ),
        ];
        let v = BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: &roles,
            overwrites: &ovs,
        };
        assert!(member_permissions(&v, ME, &[ADULT, OTHER]).contains(Permissions::CONNECT));
    }

    fn gated<'a>(
        roles: &'a HashMap<RoleId, Permissions>,
        ovs: &'a [StoredOverwrite],
    ) -> BaselineView<'a> {
        BaselineView {
            guild: GUILD,
            everyone: base(),
            role_perms: roles,
            overwrites: ovs,
        }
    }

    fn gate_overwrites() -> [StoredOverwrite; 2] {
        [
            ow(
                OverwriteTarget::Role {
                    id: everyone_role(),
                },
                Permissions::empty(),
                Permissions::VIEW_CHANNEL,
            ),
            ow(
                OverwriteTarget::Role { id: ADULT },
                Permissions::VIEW_CHANNEL,
                Permissions::empty(),
            ),
        ]
    }

    #[test]
    fn member_can_join_needs_view_and_connect() {
        let roles: HashMap<_, _> = [(ADULT, Permissions::empty())].into();
        let ovs = gate_overwrites();
        let v = gated(&roles, &ovs);
        assert!(!member_can_join(&v, ME, &[]));
        assert!(member_can_join(&v, ME, &[ADULT]));
        // Viewing alone is not enough.
        let ovs = [ow(
            OverwriteTarget::Role {
                id: everyone_role(),
            },
            Permissions::empty(),
            Permissions::CONNECT,
        )];
        assert!(!member_can_join(&gated(&roles, &ovs), ME, &[ADULT]));
    }

    #[test]
    fn role_can_connect_respects_category_deny() {
        let roles: HashMap<_, _> =
            [(ADULT, Permissions::empty()), (OTHER, Permissions::empty())].into();
        let ovs = gate_overwrites();
        let v = gated(&roles, &ovs);
        assert!(role_can_connect(&v, ADULT));
        assert!(!role_can_connect(&v, OTHER));
        // A role missing from role_perms counts as empty.
        assert!(!role_can_connect(&v, RoleId::new(999)));
    }
}
