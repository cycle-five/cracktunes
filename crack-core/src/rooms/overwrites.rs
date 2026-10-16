//! The managed overwrite layer: a room's state, laid over the category's
//! baseline overwrites, as the overwrites the channel should carry.
//!
//! "Never widen": the bot never writes an allow of `VIEW_CHANNEL`, and a lock
//! only ever moves bits towards deny, so a room owner cannot let in someone the
//! server's own category permissions keep out.

use super::state::{OverwriteTarget, RoomState, StoredOverwrite};
use serenity::all::{GuildId, Permissions, RoleId};
use std::collections::BTreeMap;

/// The only permission bits the rooms feature ever touches.
pub const MANAGED: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::CONNECT)
    .union(Permissions::SPEAK)
    .union(Permissions::SEND_MESSAGES);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverwriteChange {
    Put(StoredOverwrite),
    Delete(OverwriteTarget),
}

type Bits = (Permissions, Permissions);

fn allow(map: &mut BTreeMap<OverwriteTarget, Bits>, t: OverwriteTarget, p: Permissions) {
    let e = map.entry(t).or_default();
    e.0 |= p;
    e.1 &= !p;
}

fn deny(map: &mut BTreeMap<OverwriteTarget, Bits>, t: OverwriteTarget, p: Permissions) {
    let e = map.entry(t).or_default();
    e.0 &= !p;
    e.1 |= p;
}

/// Move `p` from allow to deny on `@everyone` (created if absent) and on every
/// other role overwrite that allows it; other role entries are left alone.
fn lock_roles(map: &mut BTreeMap<OverwriteTarget, Bits>, everyone: RoleId, p: Permissions) {
    map.entry(OverwriteTarget::Role { id: everyone })
        .or_default();
    for (t, bits) in map.iter_mut() {
        let is_everyone = *t == OverwriteTarget::Role { id: everyone };
        if is_everyone || (matches!(t, OverwriteTarget::Role { .. }) && bits.0.intersects(p)) {
            bits.0 &= !p;
            bits.1 |= p;
        }
    }
}

/// The overwrites a room should carry, sorted by target, without all-zero entries.
pub fn room_overwrites(
    guild: GuildId,
    baseline: &[StoredOverwrite],
    state: &RoomState,
) -> Vec<StoredOverwrite> {
    let mut map: BTreeMap<OverwriteTarget, Bits> = baseline
        .iter()
        .map(|o| (o.target, (o.allow, o.deny)))
        .collect();
    let member = |id| OverwriteTarget::Member { id };
    let role = |id| OverwriteTarget::Role { id };
    let everyone = RoleId::new(guild.get());

    if state.locked {
        lock_roles(&mut map, everyone, Permissions::CONNECT);
        for r in &state.permitted_roles {
            allow(&mut map, role(*r), Permissions::CONNECT);
        }
        let users = std::iter::once(&state.owner)
            .chain(&state.helpers)
            .chain(&state.permitted_users);
        for u in users {
            allow(&mut map, member(*u), Permissions::CONNECT);
        }
    }
    if state.chat_locked {
        lock_roles(&mut map, everyone, Permissions::SEND_MESSAGES);
        for u in std::iter::once(&state.owner).chain(&state.helpers) {
            allow(&mut map, member(*u), Permissions::SEND_MESSAGES);
        }
    }
    let shut = Permissions::VIEW_CHANNEL | Permissions::CONNECT;
    for r in &state.blocked_roles {
        deny(&mut map, role(*r), shut);
    }
    for u in &state.muted {
        deny(&mut map, member(*u), Permissions::SPEAK);
    }
    for u in &state.blocked {
        deny(&mut map, member(*u), shut);
    }

    map.into_iter()
        .filter(|(_, (a, d))| !(a.is_empty() && d.is_empty()))
        .map(|(target, (allow, deny))| StoredOverwrite {
            target,
            allow,
            deny,
        })
        .collect()
}

/// What to write to turn `before` into `after`; sorted by target. An entry
/// of `before` with no bits set counts as absent: there is nothing on it to
/// delete.
pub fn diff(before: &[StoredOverwrite], after: &[StoredOverwrite]) -> Vec<OverwriteChange> {
    let b: BTreeMap<_, _> = before
        .iter()
        .filter(|o| !(o.allow.is_empty() && o.deny.is_empty()))
        .map(|o| (o.target, *o))
        .collect();
    let a: BTreeMap<_, _> = after.iter().map(|o| (o.target, *o)).collect();
    let mut out: Vec<(OverwriteTarget, OverwriteChange)> = Vec::new();
    for (t, o) in &a {
        if b.get(t) != Some(o) {
            out.push((*t, OverwriteChange::Put(*o)));
        }
    }
    for t in b.keys() {
        if !a.contains_key(t) {
            out.push((*t, OverwriteChange::Delete(*t)));
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::UserId;

    const G: GuildId = GuildId::new(100);
    const V: Permissions = Permissions::VIEW_CHANNEL;
    const C: Permissions = Permissions::CONNECT;
    const S: Permissions = Permissions::SEND_MESSAGES;

    fn r(id: u64) -> OverwriteTarget {
        OverwriteTarget::Role {
            id: RoleId::new(id),
        }
    }
    fn m(id: u64) -> OverwriteTarget {
        OverwriteTarget::Member {
            id: UserId::new(id),
        }
    }
    fn ow(target: OverwriteTarget, allow: Permissions, deny: Permissions) -> StoredOverwrite {
        StoredOverwrite {
            target,
            allow,
            deny,
        }
    }
    fn get(v: &[StoredOverwrite], t: OverwriteTarget) -> Option<(Permissions, Permissions)> {
        v.iter().find(|o| o.target == t).map(|o| (o.allow, o.deny))
    }
    fn state() -> RoomState {
        RoomState::new(UserId::new(1))
    }
    fn baseline() -> Vec<StoredOverwrite> {
        vec![
            ow(r(100), Permissions::empty(), V),
            ow(r(18), V | C, Permissions::empty()),
        ]
    }

    #[test]
    fn unlocked_default_state_is_the_baseline() {
        let mut base = baseline();
        base.push(ow(r(5), Permissions::empty(), Permissions::empty()));
        base.reverse();
        let out = room_overwrites(G, &base, &state());
        assert_eq!(
            out,
            vec![
                ow(r(18), V | C, Permissions::empty()),
                ow(r(100), Permissions::empty(), V)
            ]
        );
    }

    #[test]
    fn lock_moves_connect_to_deny_on_everyone_and_every_role_allow() {
        let mut s = state();
        s.locked = true;
        let out = room_overwrites(G, &baseline(), &s);
        assert_eq!(get(&out, r(100)), Some((Permissions::empty(), V | C)));
        assert_eq!(get(&out, r(18)), Some((V, C)));
    }

    #[test]
    fn lock_creates_everyone_when_absent() {
        let mut s = state();
        s.locked = true;
        let out = room_overwrites(G, &[], &s);
        assert_eq!(get(&out, r(100)), Some((Permissions::empty(), C)));
    }

    #[test]
    fn lock_lets_owner_helpers_and_permitted_users_connect() {
        let mut s = state();
        s.locked = true;
        s.helpers.insert(UserId::new(2));
        s.permitted_users.insert(UserId::new(3));
        let out = room_overwrites(G, &baseline(), &s);
        for id in [1, 2, 3] {
            assert_eq!(get(&out, m(id)), Some((C, Permissions::empty())), "{id}");
        }
        s.locked = false;
        assert_eq!(get(&room_overwrites(G, &baseline(), &s), m(1)), None);
    }

    #[test]
    fn permitted_role_keeps_connect_while_locked() {
        let mut s = state();
        s.locked = true;
        s.permitted_roles.insert(RoleId::new(18));
        s.permitted_roles.insert(RoleId::new(30));
        let out = room_overwrites(G, &baseline(), &s);
        assert_eq!(get(&out, r(18)), Some((V | C, Permissions::empty())));
        assert_eq!(get(&out, r(30)), Some((C, Permissions::empty())));
    }

    #[test]
    fn chat_lock_moves_send_to_deny_and_exempts_owner_and_helpers() {
        let mut s = state();
        s.chat_locked = true;
        s.helpers.insert(UserId::new(2));
        s.permitted_users.insert(UserId::new(3));
        let base = vec![ow(r(18), S, Permissions::empty())];
        let out = room_overwrites(G, &base, &s);
        assert_eq!(get(&out, r(100)), Some((Permissions::empty(), S)));
        assert_eq!(get(&out, r(18)), Some((Permissions::empty(), S)));
        assert_eq!(get(&out, m(1)), Some((S, Permissions::empty())));
        assert_eq!(get(&out, m(2)), Some((S, Permissions::empty())));
        assert_eq!(get(&out, m(3)), None);
    }

    #[test]
    fn blocked_user_is_denied_view_and_connect_even_if_permitted() {
        let mut s = state();
        s.locked = true;
        s.permitted_users.insert(UserId::new(4));
        s.blocked.insert(UserId::new(4));
        let base = vec![ow(m(4), V | C, Permissions::empty())];
        let out = room_overwrites(G, &base, &s);
        assert_eq!(get(&out, m(4)), Some((Permissions::empty(), V | C)));
    }

    #[test]
    fn muted_user_is_denied_speak_only() {
        let mut s = state();
        s.muted.insert(UserId::new(5));
        let out = room_overwrites(G, &[], &s);
        assert_eq!(
            out,
            vec![ow(m(5), Permissions::empty(), Permissions::SPEAK)]
        );
    }

    #[test]
    fn blocked_role_denied_view_and_connect() {
        let mut s = state();
        s.locked = true;
        s.permitted_roles.insert(RoleId::new(18));
        s.blocked_roles.insert(RoleId::new(18));
        let out = room_overwrites(G, &baseline(), &s);
        assert_eq!(get(&out, r(18)), Some((Permissions::empty(), V | C)));
    }

    #[test]
    fn never_widens_view() {
        let mut full = state();
        full.locked = true;
        full.chat_locked = true;
        full.helpers.insert(UserId::new(2));
        full.permitted_users.insert(UserId::new(3));
        full.permitted_roles.insert(RoleId::new(18));
        full.muted.insert(UserId::new(5));
        let mut blocked = full.clone();
        blocked.blocked.insert(UserId::new(3));
        blocked.blocked_roles.insert(RoleId::new(18));
        let bases = vec![
            vec![],
            baseline(),
            vec![
                ow(m(3), Permissions::empty(), V),
                ow(m(2), Permissions::empty(), V),
            ],
            vec![ow(r(100), V, Permissions::empty())],
        ];
        for st in [state(), full, blocked] {
            for base in &bases {
                for o in room_overwrites(G, base, &st) {
                    let allowed = get(base, o.target).map_or(Permissions::empty(), |b| b.0);
                    assert!(
                        (o.allow & V) & !allowed == Permissions::empty(),
                        "{:?} widened VIEW",
                        o.target
                    );
                }
            }
        }
    }

    #[test]
    fn non_managed_baseline_bits_survive() {
        let mm = Permissions::MANAGE_MESSAGES;
        let base = vec![
            ow(r(18), mm | C | S, Permissions::empty()),
            ow(m(2), mm, Permissions::empty()),
        ];
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        s.helpers.insert(UserId::new(2));
        s.muted.insert(UserId::new(2));
        let out = room_overwrites(G, &base, &s);
        assert_eq!(get(&out, r(18)), Some((mm, C | S)));
        let (a, d) = get(&out, m(2)).unwrap();
        assert!(a.contains(mm));
        assert!(d.contains(Permissions::SPEAK));
    }

    #[test]
    fn diff_puts_changed_and_new_targets_and_deletes_vanished_ones() {
        let before = vec![
            ow(r(1), V, Permissions::empty()),
            ow(r(2), V, Permissions::empty()),
            ow(m(9), C, Permissions::empty()),
        ];
        let after = vec![
            ow(r(1), V, C),
            ow(r(2), V, Permissions::empty()),
            ow(r(3), Permissions::empty(), C),
        ];
        assert_eq!(
            diff(&before, &after),
            vec![
                OverwriteChange::Delete(m(9)),
                OverwriteChange::Put(ow(r(1), V, C)),
                OverwriteChange::Put(ow(r(3), Permissions::empty(), C)),
            ]
        );
    }

    #[test]
    fn lock_leaves_roles_that_do_not_allow_connect_untouched() {
        let mm = Permissions::MANAGE_MESSAGES;
        let base = vec![
            ow(r(40), mm, Permissions::empty()),
            ow(r(41), Permissions::empty(), V),
        ];
        let unlocked = room_overwrites(G, &base, &state());
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        let out = room_overwrites(G, &base, &s);
        assert_eq!(get(&out, r(40)), Some((mm, Permissions::empty())));
        assert_eq!(get(&out, r(41)), Some((Permissions::empty(), V)));
        let touched: Vec<_> = diff(&unlocked, &out)
            .into_iter()
            .filter(
                |c| matches!(c, OverwriteChange::Put(o) if o.target == r(40) || o.target == r(41)),
            )
            .collect();
        assert!(touched.is_empty(), "{touched:?}");
    }

    #[test]
    fn locked_and_chat_locked_permitted_role_gets_connect_allow_and_send_deny() {
        let base = vec![ow(r(18), C | S, Permissions::empty())];
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        s.permitted_roles.insert(RoleId::new(18));
        let out = room_overwrites(G, &base, &s);
        assert_eq!(get(&out, r(18)), Some((C, S)));
    }

    #[test]
    fn diff_of_equal_sets_is_empty() {
        let a = baseline();
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn diff_treats_a_neutral_entry_in_before_as_absent() {
        // A role added to the category with nothing set: no bits to clear,
        // so nothing to send.
        let neutral = ow(r(50), Permissions::empty(), Permissions::empty());
        let mut before = baseline();
        before.push(neutral);
        assert_eq!(diff(&before, &baseline()), vec![]);
        // Given bits again, it is put as any change would be.
        let set = ow(r(50), C, Permissions::empty());
        let mut after = baseline();
        after.push(set);
        assert_eq!(diff(&before, &after), vec![OverwriteChange::Put(set)]);
    }

    #[test]
    fn managed_is_exactly_the_four_bits() {
        assert_eq!(MANAGED, V | C | Permissions::SPEAK | S);
    }
}
