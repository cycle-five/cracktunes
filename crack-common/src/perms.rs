//! Pure permission helpers shared by the rooms card, the music gate and the
//! economy's preflight. No serenity cache, no poise.
use serenity::all::{GuildId, Permissions, RoleId};

pub fn permission_names(perms: Permissions) -> Vec<String> {
    perms
        .get_permission_names()
        .iter()
        .map(|n| (*n).to_string())
        .collect()
}

pub fn names_text(names: &[String], bold: bool) -> String {
    names
        .iter()
        .map(|n| if bold { format!("**{n}**") } else { n.clone() })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn perm_names(perms: Permissions, bold: bool) -> String {
    names_text(&permission_names(perms), bold)
}

/// Whether every role the permission math depends on is actually in the cache.
///
/// 🪤 `Guild::user_permissions_in` has no way to say "unknown role". When a
/// role id on the member is absent from `guild.roles`, or the `@everyone` role
/// (whose id is the guild id) is, serenity substitutes `Permissions::empty()`
/// for it and only logs — see `user_permissions_in_` in serenity's
/// `model/guild/mod.rs`, the `@everyone role missing` error and the
/// `has non-existent role` warning. The bitset that comes back is then
/// **under**-reported, which would make the gate refuse a join that would have
/// worked and send an admin to grant a permission they had already granted.
/// That is the fail-open constraint violated in the worst direction, so both
/// cache readers below check this first and give up rather than guess.
///
/// Takes a lookup closure rather than a `Guild` so the rule is pure and
/// testable without a populated `Cache`.
pub fn roles_resolvable(
    guild_id: GuildId,
    member_roles: &[RoleId],
    is_known: impl Fn(RoleId) -> bool,
) -> bool {
    is_known(RoleId::new(guild_id.get())) && member_roles.iter().all(|r| is_known(*r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_the_cache_does_not_have_makes_the_bitset_untrustworthy() {
        let gid = serenity::all::GuildId::new(10);
        let everyone = RoleId::new(10);
        let extra = RoleId::new(11);
        let known = [everyone, extra];

        assert!(
            roles_resolvable(gid, &[extra], |r| known.contains(&r)),
            "every role resolves, so the bitset can be trusted"
        );
        // 🪤 serenity substitutes Permissions::empty() for a role it cannot
        // find and only logs, so the bitset comes back *under*-reported and
        // the gate would refuse a join that works.
        assert!(
            !roles_resolvable(gid, &[RoleId::new(99)], |r| known.contains(&r)),
            "an unknown bot role must not be silently treated as no permissions"
        );
    }

    #[test]
    fn a_missing_everyone_role_makes_the_bitset_untrustworthy() {
        let gid = serenity::all::GuildId::new(10);
        // @everyone carries the guild's baseline grants and its id is the
        // guild id; without it every permission looks denied.
        assert!(!roles_resolvable(gid, &[], |_| false));
        assert!(roles_resolvable(gid, &[], |r| r == RoleId::new(10)));
    }
}
