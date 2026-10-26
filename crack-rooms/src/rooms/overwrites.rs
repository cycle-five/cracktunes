//! The managed overwrite layer: a room's state, laid over the category's
//! baseline overwrites, as the overwrites the channel should carry.
//!
//! "Never widen": the bot never writes an allow of `VIEW_CHANNEL`, a lock
//! only ever moves bits towards deny, and a managed allow never lifts a deny
//! the baseline holds for the same target, so a room owner cannot let in
//! someone the server's own category permissions keep out.

use super::state::{OverwriteTarget, RoomState, StoredOverwrite};
use serenity::all::{GuildId, Permissions, RoleId, UserId};
use std::collections::{BTreeMap, BTreeSet};

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

/// The allow bits the bot itself writes on a member's entry: a lock's
/// Connect and a chat lock's Send Messages.
const MEMBER_ALLOWS: Permissions = Permissions::CONNECT.union(Permissions::SEND_MESSAGES);

type Bits = (Permissions, Permissions);

/// Allow `p` on `t`, all but the bits the baseline denies `t` (`held`): a
/// managed allow never overrides the server's own deny on the same target.
fn allow(
    map: &mut BTreeMap<OverwriteTarget, Bits>,
    held: &BTreeMap<OverwriteTarget, Permissions>,
    t: OverwriteTarget,
    p: Permissions,
) {
    let p = p & !held.get(&t).copied().unwrap_or_default();
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
/// `bot` is CrackTunes itself, which a lock must not shut out.
///
/// Every managed entry starts from the baseline's entry for its target, so
/// the server's own bits on it survive the write. A managed allow never
/// lifts a deny the baseline holds for the same target: a member or role
/// the category (or an admin, folded in by [`fold_cached`]) denies Connect
/// stays denied when permitted, and so does CrackTunes.
pub fn room_overwrites(
    guild: GuildId,
    bot: UserId,
    baseline: &[StoredOverwrite],
    state: &RoomState,
) -> Vec<StoredOverwrite> {
    let mut map: BTreeMap<OverwriteTarget, Bits> = baseline
        .iter()
        .map(|o| (o.target, (o.allow, o.deny)))
        .collect();
    let held: BTreeMap<OverwriteTarget, Permissions> =
        baseline.iter().map(|o| (o.target, o.deny)).collect();
    let member = |id| OverwriteTarget::Member { id };
    let role = |id| OverwriteTarget::Role { id };
    let everyone = RoleId::new(guild.get());

    // CrackTunes gets the bit a lock takes away, never View Channel: it sees
    // the room through the category, as the preflight requires. Without it,
    // a lock would keep it from playing in the room, a queue resume from
    // rejoining it and the hub from moving an owner in; a chat lock would
    // keep it from posting the panel again.
    if state.locked {
        lock_roles(&mut map, everyone, Permissions::CONNECT);
        for r in &state.permitted_roles {
            allow(&mut map, &held, role(*r), Permissions::CONNECT);
        }
        let users = [&state.owner, &bot]
            .into_iter()
            .chain(&state.helpers)
            .chain(&state.permitted_users);
        for u in users {
            allow(&mut map, &held, member(*u), Permissions::CONNECT);
        }
    }
    if state.chat_locked {
        lock_roles(&mut map, everyone, Permissions::SEND_MESSAGES);
        for u in [&state.owner, &bot].into_iter().chain(&state.helpers) {
            allow(&mut map, &held, member(*u), Permissions::SEND_MESSAGES);
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

/// R33: fold what a room's channel carries now (`cached`, as the cache holds
/// it) into its stored `baseline`, never opener than either. `expected` is
/// what the bot last wrote there: [`room_overwrites`] of `baseline` and the
/// room's state. What differs between the two was changed by someone else,
/// typically the category reaching an untouched room through Discord's sync,
/// or an admin editing the room:
/// - a bit the channel denies that `expected` does not joins the baseline's
///   deny (and leaves its allow);
/// - a bit `expected` allows that the channel does not leaves the baseline's
///   allow;
/// - a target only the channel carries is the server's, and is added whole;
/// - a member entry of one of `managed` (see [`managed_members`]) carries
///   nothing of the bot's but the member allows it writes, Connect and Send
///   Messages (maybe a lock's leftover). Any other bit the channel allows
///   there beyond `expected` is the server's too, an admin's grant to
///   CrackTunes say, and joins the baseline's allow unless the baseline
///   denies it. Such an entry only the channel carries (an unlocked room's
///   owner's, say) is added on the same terms: whole, but for those two;
/// - otherwise a bit the channel allows or stopped denying is not taken:
///   the stored baseline stays the tighter side.
///
/// Every target keeps its place; new ones go at the end. The bot's own
/// latest writes may not be in the cache yet, and would read as someone
/// else's: the caller folds only once the cache has caught up.
pub fn fold_cached(
    baseline: &[StoredOverwrite],
    expected: &[StoredOverwrite],
    cached: &[StoredOverwrite],
    managed: &BTreeSet<UserId>,
) -> Vec<StoredOverwrite> {
    let index = |v: &[StoredOverwrite]| -> BTreeMap<OverwriteTarget, Bits> {
        v.iter().map(|o| (o.target, (o.allow, o.deny))).collect()
    };
    let (expected, cache) = (index(expected), index(cached));
    let known: BTreeSet<OverwriteTarget> = baseline
        .iter()
        .map(|o| o.target)
        .chain(expected.keys().copied())
        .collect();
    let managed_member =
        |t: &OverwriteTarget| matches!(t, OverwriteTarget::Member { id } if managed.contains(id));
    let mut out = baseline.to_vec();
    for t in &known {
        let (want_allow, want_deny) = expected.get(t).copied().unwrap_or_default();
        let (has_allow, has_deny) = cache.get(t).copied().unwrap_or_default();
        let added_deny = has_deny & !want_deny;
        let lost_allow = want_allow & !has_allow;
        let gained = if managed_member(t) {
            has_allow & !want_allow & !MEMBER_ALLOWS & !added_deny
        } else {
            Permissions::empty()
        };
        match out.iter_mut().find(|o| o.target == *t) {
            Some(o) => {
                o.deny |= added_deny;
                o.allow &= !(lost_allow | added_deny);
                o.allow |= gained & !o.deny;
            },
            None if !(added_deny | gained).is_empty() => out.push(StoredOverwrite {
                target: *t,
                allow: gained,
                deny: added_deny,
            }),
            None => {},
        }
    }
    for o in cached.iter().filter(|o| !known.contains(&o.target)) {
        let allow = if managed_member(&o.target) {
            o.allow & !MEMBER_ALLOWS
        } else {
            o.allow
        };
        if !(allow.is_empty() && o.deny.is_empty()) {
            out.push(StoredOverwrite {
                target: o.target,
                allow,
                deny: o.deny,
            });
        }
    }
    out
}

/// The people whose member overwrites in a room the bot writes (some of the
/// time: an unlocked room's owner has none): the owner, helpers, permitted,
/// muted and blocked people, and CrackTunes.
pub fn managed_members(state: &RoomState, bot: UserId) -> BTreeSet<UserId> {
    [state.owner, bot]
        .into_iter()
        .chain(state.helpers.iter().copied())
        .chain(state.permitted_users.iter().copied())
        .chain(state.muted.iter().copied())
        .chain(state.blocked.iter().copied())
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

    const G: GuildId = GuildId::new(100);
    const BOT: UserId = UserId::new(99);
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
        let out = room_overwrites(G, BOT, &base, &state());
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
        let out = room_overwrites(G, BOT, &baseline(), &s);
        assert_eq!(get(&out, r(100)), Some((Permissions::empty(), V | C)));
        assert_eq!(get(&out, r(18)), Some((V, C)));
    }

    #[test]
    fn lock_creates_everyone_when_absent() {
        let mut s = state();
        s.locked = true;
        let out = room_overwrites(G, BOT, &[], &s);
        assert_eq!(get(&out, r(100)), Some((Permissions::empty(), C)));
    }

    #[test]
    fn lock_lets_owner_helpers_and_permitted_users_connect() {
        let mut s = state();
        s.locked = true;
        s.helpers.insert(UserId::new(2));
        s.permitted_users.insert(UserId::new(3));
        let out = room_overwrites(G, BOT, &baseline(), &s);
        for id in [1, 2, 3] {
            assert_eq!(get(&out, m(id)), Some((C, Permissions::empty())), "{id}");
        }
        s.locked = false;
        assert_eq!(get(&room_overwrites(G, BOT, &baseline(), &s), m(1)), None);
    }

    /// A lock must not shut CrackTunes out: it plays music in rooms, a queue
    /// resume rejoins them, and moving an owner into their locked room needs
    /// the mover to be able to connect there.
    #[test]
    fn lock_lets_crack_tunes_connect_and_chat_lock_lets_it_post() {
        let mut s = state();
        s.locked = true;
        let out = room_overwrites(G, BOT, &baseline(), &s);
        assert_eq!(get(&out, m(99)), Some((C, Permissions::empty())));

        let mut s = state();
        s.chat_locked = true;
        let out = room_overwrites(G, BOT, &baseline(), &s);
        assert_eq!(get(&out, m(99)), Some((S, Permissions::empty())));

        s.locked = true;
        let out = room_overwrites(G, BOT, &baseline(), &s);
        assert_eq!(get(&out, m(99)), Some((C | S, Permissions::empty())));
    }

    #[test]
    fn unlocked_room_has_no_crack_tunes_entry() {
        let mut s = state();
        s.helpers.insert(UserId::new(2));
        s.muted.insert(UserId::new(5));
        s.blocked_roles.insert(RoleId::new(30));
        let out = room_overwrites(G, BOT, &baseline(), &s);
        assert_eq!(get(&out, m(99)), None);
        // A category entry for CrackTunes keeps its bits; a lock only adds.
        let mm = Permissions::MANAGE_MESSAGES;
        let base = vec![ow(m(99), mm, Permissions::empty())];
        assert_eq!(
            get(&room_overwrites(G, BOT, &base, &s), m(99)),
            Some((mm, Permissions::empty()))
        );
        s.locked = true;
        assert_eq!(
            get(&room_overwrites(G, BOT, &base, &s), m(99)),
            Some((mm | C, Permissions::empty()))
        );
    }

    #[test]
    fn permitted_role_keeps_connect_while_locked() {
        let mut s = state();
        s.locked = true;
        s.permitted_roles.insert(RoleId::new(18));
        s.permitted_roles.insert(RoleId::new(30));
        let out = room_overwrites(G, BOT, &baseline(), &s);
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
        let out = room_overwrites(G, BOT, &base, &s);
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
        let out = room_overwrites(G, BOT, &base, &s);
        assert_eq!(get(&out, m(4)), Some((Permissions::empty(), V | C)));
    }

    #[test]
    fn muted_user_is_denied_speak_only() {
        let mut s = state();
        s.muted.insert(UserId::new(5));
        let out = room_overwrites(G, BOT, &[], &s);
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
        let out = room_overwrites(G, BOT, &baseline(), &s);
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
                for o in room_overwrites(G, BOT, base, &st) {
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

    /// Never widen, for every bit: a managed allow never overrides a deny
    /// the baseline holds for the same target. A member the category (or an
    /// admin) denies Connect is not let in by a permit; a role it denies
    /// Connect stays denied when permitted; CrackTunes itself goes without
    /// Connect when an admin denied it that.
    #[test]
    fn managed_allows_never_override_a_baseline_deny() {
        let none = Permissions::empty();
        let mut full = state();
        full.locked = true;
        full.chat_locked = true;
        full.helpers.insert(UserId::new(2));
        full.permitted_users.insert(UserId::new(3));
        full.permitted_roles.insert(RoleId::new(18));
        full.permitted_roles.insert(RoleId::new(30));
        full.muted.insert(UserId::new(5));
        let mut chat_only = state();
        chat_only.chat_locked = true;
        chat_only.helpers.insert(UserId::new(2));
        let mut blocked = full.clone();
        blocked.blocked.insert(UserId::new(3));
        blocked.blocked_roles.insert(RoleId::new(30));
        let bases = vec![
            vec![],
            baseline(),
            // Member denies on each of the room's people, CrackTunes too.
            vec![
                ow(m(1), none, C | S),
                ow(m(2), none, C | S),
                ow(m(3), none, C),
                ow(m(99), none, C | S),
            ],
            // Role denies on the permitted roles and on @everyone.
            vec![
                ow(r(18), V, C),
                ow(r(30), none, C | S),
                ow(r(100), none, V | C | S),
            ],
            // One bit denied, another allowed.
            vec![ow(m(1), C, S), ow(m(2), S, C), ow(r(18), C, S)],
        ];
        for st in [state(), chat_only, full, blocked] {
            for base in &bases {
                for o in room_overwrites(G, BOT, base, &st) {
                    let held = get(base, o.target).map_or(none, |b| b.1);
                    assert!(
                        (o.allow & held).is_empty(),
                        "{:?} allows {:?}, which its baseline denies ({st:?}, {base:?})",
                        o.target,
                        o.allow & held
                    );
                }
            }
        }

        // Spelled out for a lock.
        let mut s = state();
        s.locked = true;
        s.permitted_users.insert(UserId::new(3));
        s.permitted_roles.insert(RoleId::new(30));
        let base = vec![ow(m(3), none, C), ow(r(30), none, C), ow(m(99), none, C)];
        let out = room_overwrites(G, BOT, &base, &s);
        assert_eq!(get(&out, m(3)), Some((none, C)));
        assert_eq!(get(&out, r(30)), Some((none, C)));
        assert_eq!(get(&out, m(99)), Some((none, C)));
        assert_eq!(get(&out, m(1)), Some((C, none)), "no deny: let in");
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
        let out = room_overwrites(G, BOT, &base, &s);
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
        let unlocked = room_overwrites(G, BOT, &base, &state());
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        let out = room_overwrites(G, BOT, &base, &s);
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
        let out = room_overwrites(G, BOT, &base, &s);
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

    // --- R33: folding the channel's cached overwrites into the baseline

    fn everyone() -> OverwriteTarget {
        r(100)
    }
    fn managed(s: &RoomState) -> std::collections::BTreeSet<UserId> {
        managed_members(s, BOT)
    }
    /// Fold `cached` into `base` for a room in state `s`.
    fn fold(
        base: &[StoredOverwrite],
        s: &RoomState,
        cached: &[StoredOverwrite],
    ) -> Vec<StoredOverwrite> {
        let expected = room_overwrites(G, BOT, base, s);
        fold_cached(base, &expected, cached, &managed(s))
    }

    #[test]
    fn fold_takes_a_deny_the_category_added_since() {
        // Made open to the adult role; the category has since hidden it
        // from @everyone, and Discord synced that into the untouched room.
        let base = vec![ow(r(18), V | C, Permissions::empty())];
        let cached = vec![
            ow(r(18), V | C, Permissions::empty()),
            ow(everyone(), Permissions::empty(), V),
        ];
        assert_eq!(
            fold(&base, &state(), &cached),
            vec![
                ow(r(18), V | C, Permissions::empty()),
                ow(everyone(), Permissions::empty(), V)
            ]
        );
        // A deny added on a target the baseline has already.
        let base = vec![ow(everyone(), Permissions::empty(), C)];
        let cached = vec![ow(everyone(), Permissions::empty(), V | C)];
        assert_eq!(
            fold(&base, &state(), &cached),
            vec![ow(everyone(), Permissions::empty(), V | C)]
        );
    }

    #[test]
    fn fold_drops_an_allow_the_channel_lost_and_keeps_a_deny_it_dropped() {
        let base = vec![
            ow(r(18), V | C, Permissions::empty()),
            ow(everyone(), Permissions::empty(), V),
        ];
        // The adult role lost View; @everyone's deny is gone (opener).
        let cached = vec![ow(r(18), C, Permissions::empty())];
        assert_eq!(
            fold(&base, &state(), &cached),
            vec![
                ow(r(18), C, Permissions::empty()),
                ow(everyone(), Permissions::empty(), V)
            ],
            "never opener than the stored baseline"
        );
    }

    #[test]
    fn fold_ignores_the_rooms_own_managed_layer() {
        // A locked, chat-locked room with helpers, permits, mutes, a block
        // and a blocked role: the cache shows exactly what the bot wrote.
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        s.helpers.insert(UserId::new(2));
        s.permitted_users.insert(UserId::new(3));
        s.permitted_roles.insert(RoleId::new(18));
        s.muted.insert(UserId::new(5));
        s.blocked.insert(UserId::new(6));
        s.blocked_roles.insert(RoleId::new(40));
        let base = baseline();
        let cached = room_overwrites(G, BOT, &base, &s);
        assert_eq!(fold(&base, &s, &cached), base);
        // The same cache in another order.
        let mut shuffled = cached.clone();
        shuffled.reverse();
        assert_eq!(fold(&base, &s, &shuffled), base);
    }

    #[test]
    fn fold_adds_a_target_only_the_channel_carries_unless_it_is_the_bots() {
        let base = baseline();
        let mut s = state();
        s.helpers.insert(UserId::new(2));
        let theirs = ow(r(30), C, Permissions::SPEAK);
        let someone = ow(m(7), Permissions::empty(), V);
        let cached: Vec<_> = room_overwrites(G, BOT, &base, &s)
            .into_iter()
            .chain([
                theirs,
                someone,
                // Left over from a lock (unlocked now): the owner's, a
                // helper's and CrackTunes' member allows are the bot's.
                ow(m(1), C, Permissions::empty()),
                ow(m(2), C, Permissions::empty()),
                ow(m(99), C, Permissions::empty()),
                // A neutral entry: nothing to keep.
                ow(r(31), Permissions::empty(), Permissions::empty()),
            ])
            .collect();
        let mut want = base.clone();
        want.extend([theirs, someone]);
        assert_eq!(fold(&base, &s, &cached), want);
    }

    /// While a room is unlocked its owner, helpers, permitted people and
    /// CrackTunes have no entry of the bot's, so one the channel carries for
    /// them is someone else's: a category member deny Discord synced in, an
    /// admin's deny on a helper, an admin's grant to CrackTunes. It is kept,
    /// all but the member allows the bot writes itself (Connect, Send
    /// Messages), which may be a lock's leftover.
    #[test]
    fn fold_keeps_a_managed_members_entry_the_bot_did_not_write() {
        let none = Permissions::empty();
        let mc = Permissions::MANAGE_CHANNELS;
        let mm = Permissions::MANAGE_MESSAGES;
        let mut s = state();
        s.helpers.insert(UserId::new(2));
        s.helpers.insert(UserId::new(4));
        s.permitted_users.insert(UserId::new(3));
        let base = baseline();
        let cached: Vec<_> = room_overwrites(G, BOT, &base, &s)
            .into_iter()
            .chain([
                // The category keeps the owner out; Discord synced it in.
                ow(m(1), none, V),
                // An admin's deny on a helper.
                ow(m(2), none, C),
                // An admin's grant to CrackTunes.
                ow(m(99), V | mc, none),
                // A lock's and a chat lock's leftovers, and an admin's bit.
                ow(m(4), C | S | mm, none),
                // Only a lock's leftover: nothing to keep.
                ow(m(3), C, none),
            ])
            .collect();
        let mut want = base.clone();
        want.extend([
            ow(m(1), none, V),
            ow(m(2), none, C),
            ow(m(99), V | mc, none),
            ow(m(4), mm, none),
        ]);
        assert_eq!(fold(&base, &s, &cached), want);
    }

    /// Built on that folded baseline, a lock adds to those entries and an
    /// unlock puts them back rather than deleting them: CrackTunes keeps what
    /// an admin gave it, and whom the category keeps out stays out.
    #[test]
    fn lock_and_unlock_keep_a_folded_managed_entry() {
        let none = Permissions::empty();
        let mc = Permissions::MANAGE_CHANNELS;
        let mut s = state();
        let base = vec![ow(m(1), none, V), ow(m(99), V | mc, none)];
        let unlocked = room_overwrites(G, BOT, &base, &s);
        s.locked = true;
        let locked = room_overwrites(G, BOT, &base, &s);
        assert_eq!(get(&locked, m(1)), Some((C, V)));
        assert_eq!(get(&locked, m(99)), Some((V | mc | C, none)));
        let back = diff(&locked, &unlocked);
        assert!(
            back.contains(&OverwriteChange::Put(ow(m(1), none, V))),
            "{back:?}"
        );
        assert!(back.contains(&OverwriteChange::Put(ow(m(99), V | mc, none))));
        assert!(!back.contains(&OverwriteChange::Delete(m(99))));
        assert!(!back.contains(&OverwriteChange::Delete(m(1))));
    }

    /// The same goes for an entry the bot does write (a locked room's):
    /// what the channel's entry carries beyond the bot's is kept, but for the
    /// member allows, and never a bit the baseline denies.
    #[test]
    fn fold_keeps_extra_allows_on_a_managed_entry_the_bot_writes() {
        let none = Permissions::empty();
        let mc = Permissions::MANAGE_CHANNELS;
        let mut s = state();
        s.locked = true;
        s.chat_locked = true;
        s.permitted_users.insert(UserId::new(3));
        // The category keeps the owner from seeing the room, and lets a
        // permitted member see it.
        let base = vec![ow(m(1), none, V), ow(m(3), V, none)];
        let mut cached = room_overwrites(G, BOT, &base, &s);
        for o in &mut cached {
            if o.target == m(99) {
                // An admin gives CrackTunes View and Manage Channels.
                o.allow |= V | mc;
            } else if o.target == m(1) {
                // An admin flipped the owner's deny View to an allow.
                o.allow |= V;
                o.deny &= !V;
            } else if o.target == m(3) {
                // Send Messages, which a chat lock gives helpers: the bot's,
                // left over from when they were one.
                o.allow |= S;
            }
        }
        let mut want = base.clone();
        want.push(ow(m(99), V | mc, none));
        assert_eq!(fold(&base, &s, &cached), want);
    }

    #[test]
    fn fold_keeps_the_baseline_order_and_changes_nothing_when_in_step() {
        // Stored as Discord listed it, @everyone (100) before the adult
        // role (18): not sorted, and kept so, so nothing is saved again.
        let base = baseline();
        let mut cached = baseline();
        cached.reverse();
        assert_eq!(fold(&base, &state(), &cached), base);
        assert_eq!(fold(&[], &state(), &[]), vec![]);
    }

    #[test]
    fn managed_members_are_the_rooms_people_and_crack_tunes() {
        let mut s = state();
        s.helpers.insert(UserId::new(2));
        s.permitted_users.insert(UserId::new(3));
        s.muted.insert(UserId::new(5));
        s.blocked.insert(UserId::new(6));
        let got: Vec<u64> = managed_members(&s, BOT)
            .into_iter()
            .map(UserId::get)
            .collect();
        assert_eq!(got, [1, 2, 3, 5, 6, 99]);
    }

    #[test]
    fn managed_is_exactly_the_four_bits() {
        assert_eq!(MANAGED, V | C | Permissions::SPEAK | S);
    }
}
