//! Who forced a voice disconnect or move: audit count deltas matched to the
//! leaves and moves the bot saw. Pure. When it cannot tell, it says so
//! ([`Attribution::OneOf`], [`ModLogEvent::DisconnectBatch`]) or says nothing.
use super::event::{Attribution, ModLogEvent};
use serenity::all::{ChannelId, UserId};
use std::collections::{BTreeMap, HashMap};

/// The two audit action types the log reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditKind {
    /// `MEMBER_DISCONNECT` (27).
    Disconnect,
    /// `MEMBER_MOVE` (26).
    Move,
}

/// One audit log entry, reduced to what attribution needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub id: u64,
    pub kind: AuditKind,
    pub actor: Option<UserId>,
    pub target: Option<UserId>,
    pub count: u32,
    pub channel: Option<ChannelId>,
}

/// Entry id to the count last seen, for one (guild, kind).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub seeded: bool,
    pub counts: HashMap<u64, u32>,
}

/// The two snapshots of one guild, one per audit action type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshots {
    pub disconnect: Snapshot,
    pub moves: Snapshot,
}

impl Snapshots {
    fn of(&self, kind: AuditKind) -> &Snapshot {
        match kind {
            AuditKind::Disconnect => &self.disconnect,
            AuditKind::Move => &self.moves,
        }
    }
}

/// A voice change the bot observed. Callers pass the bot's own changes too,
/// so their audit units are consumed; they are never reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Left {
        target: UserId,
        from: ChannelId,
    },
    Moved {
        target: UserId,
        from: ChannelId,
        to: ChannelId,
    },
}

impl Change {
    fn target(&self) -> UserId {
        match self {
            Self::Left { target, .. } | Self::Moved { target, .. } => *target,
        }
    }

    fn kind(&self) -> AuditKind {
        match self {
            Self::Left { .. } => AuditKind::Disconnect,
            Self::Moved { .. } => AuditKind::Move,
        }
    }

    fn event(&self, by: Attribution) -> ModLogEvent {
        match self {
            Self::Left { target, from } => ModLogEvent::Disconnect {
                target: *target,
                from: *from,
                by,
            },
            Self::Moved { target, from, to } => ModLogEvent::Move {
                target: *target,
                from: *from,
                to: *to,
                by,
            },
        }
    }
}

/// How many entry ids a snapshot keeps (the highest).
const SNAPSHOT_KEEP: usize = 100;

/// Merge the rows' counts (never lowering a stored one) into the snapshot of each kind in `fetched` (ids
/// not fetched keep their counts), prune each to the highest
/// [`SNAPSHOT_KEEP`] ids, and mark it seeded: a fetched kind with no rows is
/// seeded too. A kind not in `fetched` is left untouched, and its rows are
/// ignored.
pub fn advance(snaps: &mut Snapshots, fetched: &[AuditKind], rows: &[AuditRow]) {
    for kind in [AuditKind::Disconnect, AuditKind::Move] {
        if !fetched.contains(&kind) {
            continue;
        }
        let snap = match kind {
            AuditKind::Disconnect => &mut snaps.disconnect,
            AuditKind::Move => &mut snaps.moves,
        };
        for r in rows.iter().filter(|r| r.kind == kind) {
            let c = snap.counts.entry(r.id).or_insert(0);
            *c = (*c).max(r.count);
        }
        if snap.counts.len() > SNAPSHOT_KEEP {
            let mut ids: Vec<u64> = snap.counts.keys().copied().collect();
            ids.sort_unstable_by(|a, b| b.cmp(a));
            for id in &ids[SNAPSHOT_KEEP..] {
                snap.counts.remove(id);
            }
        }
        snap.seeded = true;
    }
    if rows.iter().any(|r| !fetched.contains(&r.kind)) {
        tracing::debug!("modlog: advance ignored rows of a kind that was not fetched");
    }
}

/// A row that counts: not the bot's, with an actor, on a seeded snapshot.
struct Live<'a> {
    row: &'a AuditRow,
    actor: UserId,
    delta: u32,
}

/// What to report for a group of changes given the actors' units. Bot
/// changes count towards L but are never named.
fn decide(
    changes: &[&Change],
    units: &BTreeMap<UserId, u64>,
    batch_ok: bool,
    bot: UserId,
) -> Vec<ModLogEvent> {
    let d: u64 = units.values().fold(0, |a, u| a.saturating_add(*u));
    let l = changes.len() as u64;
    if d == 0 || l == 0 {
        if d > 0 {
            tracing::debug!("modlog: {d} audit units with no change to explain");
        }
        return Vec::new();
    }
    if d > l {
        tracing::debug!("modlog: {d} audit units for {l} changes: not attributed");
        return Vec::new();
    }
    let actors: Vec<UserId> = units.keys().copied().collect();
    if d < l {
        if !batch_ok {
            tracing::debug!("modlog: {d} audit units for {l} moves: not attributed");
            return Vec::new();
        }
        let mut among: Vec<UserId> = changes
            .iter()
            .map(|c| c.target())
            .filter(|t| *t != bot)
            .collect();
        among.dedup();
        // Claim only what is certain: units may have gone to the bot's own
        // changes, or to one member leaving twice.
        let named = changes.iter().filter(|c| c.target() != bot).count();
        let own = changes.len() - named;
        let dups = named - among.len();
        let count = d.saturating_sub((own + dups) as u64);
        if among.is_empty() || count == 0 {
            tracing::debug!("modlog: no unit of {d} is certain for {l} leaves");
            return Vec::new();
        }
        return vec![ModLogEvent::DisconnectBatch {
            actors,
            count: u32::try_from(count).unwrap_or(u32::MAX),
            among,
        }];
    }
    let by = || match actors.as_slice() {
        [one] => Attribution::By(*one),
        _ => Attribution::OneOf(actors.clone()),
    };
    changes
        .iter()
        .filter(|c| c.target() != bot)
        .map(|c| c.event(by()))
        .collect()
}

/// Turn audit deltas and observed voice changes into events (spec 2.3).
/// Output order is stable: exact pairs (by row id), then leaves, then moves
/// by destination id; within a group by target id, actors sorted.
///
/// A row naming a target never feeds the pools: it only pairs exactly, and
/// only when new since the snapshot.
#[must_use]
pub fn attribute(
    snaps: &Snapshots,
    rows: &[AuditRow],
    changes: &[Change],
    bot: UserId,
) -> Vec<ModLogEvent> {
    let mut sorted_changes: Vec<&Change> = changes.iter().collect();
    sorted_changes.sort_by_key(|c| c.target());
    let mut paired = vec![false; sorted_changes.len()];
    let mut events: Vec<ModLogEvent> = Vec::new();

    let mut live: Vec<Live> = rows
        .iter()
        .filter_map(|r| {
            let snap = snaps.of(r.kind);
            if !snap.seeded {
                return None;
            }
            let actor = r.actor.filter(|a| *a != bot)?;
            let delta = match snap.counts.get(&r.id) {
                Some(prev) => r.count.saturating_sub(*prev),
                None => r.count,
            };
            (delta > 0).then_some(Live {
                row: r,
                actor,
                delta,
            })
        })
        .collect();
    live.sort_by_key(|l| l.row.id);

    // Exact pairs: targeted rows only, and only new single-count rows.
    for l in live.iter().filter(|l| l.row.target.is_some()) {
        let Some(target) = l.row.target else { continue };
        let new = !snaps.of(l.row.kind).counts.contains_key(&l.row.id);
        if !new || l.row.count != 1 {
            tracing::debug!(
                "modlog: entry {} (count {}) names a target but cannot pair",
                l.row.id,
                l.row.count
            );
            continue;
        }
        let hit = sorted_changes.iter().enumerate().find(|(i, c)| {
            !paired[*i]
                && c.target() == target
                && c.kind() == l.row.kind
                && match c {
                    Change::Moved { to, .. } => l.row.channel == Some(*to),
                    Change::Left { .. } => true,
                }
        });
        if let Some((i, c)) = hit {
            paired[i] = true;
            if c.target() != bot {
                events.push(c.event(Attribution::By(l.actor)));
            }
        }
    }

    // Leaves: one group.
    let leaves: Vec<&Change> = sorted_changes
        .iter()
        .enumerate()
        .filter(|(i, c)| !paired[*i] && c.kind() == AuditKind::Disconnect)
        .map(|(_, c)| *c)
        .collect();
    let mut units: BTreeMap<UserId, u64> = BTreeMap::new();
    for l in &live {
        if l.row.target.is_none() && l.row.kind == AuditKind::Disconnect {
            let u = units.entry(l.actor).or_default();
            *u = u.saturating_add(u64::from(l.delta));
        }
    }
    events.extend(decide(&leaves, &units, true, bot));

    // Moves: one group per destination.
    let mut by_dest: BTreeMap<ChannelId, Vec<&Change>> = BTreeMap::new();
    for (i, c) in sorted_changes.iter().enumerate() {
        if let (false, Change::Moved { to, .. }) = (paired[i], c) {
            by_dest.entry(*to).or_default().push(c);
        }
    }
    for (dest, group) in &by_dest {
        let mut units: BTreeMap<UserId, u64> = BTreeMap::new();
        for l in &live {
            if l.row.target.is_none()
                && l.row.kind == AuditKind::Move
                && l.row.channel == Some(*dest)
            {
                let u = units.entry(l.actor).or_default();
                *u = u.saturating_add(u64::from(l.delta));
            }
        }
        events.extend(decide(group, &units, false, bot));
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT: UserId = UserId::new(99);
    const M1: UserId = UserId::new(1);
    const M2: UserId = UserId::new(2);
    const T1: UserId = UserId::new(11);
    const T2: UserId = UserId::new(12);
    const T3: UserId = UserId::new(13);
    const C1: ChannelId = ChannelId::new(101);
    const C2: ChannelId = ChannelId::new(102);
    const C3: ChannelId = ChannelId::new(103);

    fn seeded(counts: &[(u64, u32)]) -> Snapshots {
        let s = Snapshot {
            seeded: true,
            counts: counts.iter().copied().collect(),
        };
        Snapshots {
            disconnect: s.clone(),
            moves: s,
        }
    }

    fn row(id: u64, kind: AuditKind, actor: UserId, count: u32) -> AuditRow {
        AuditRow {
            id,
            kind,
            actor: Some(actor),
            target: None,
            count,
            channel: None,
        }
    }

    fn dc(id: u64, actor: UserId, count: u32) -> AuditRow {
        row(id, AuditKind::Disconnect, actor, count)
    }

    fn mv(id: u64, actor: UserId, count: u32, to: ChannelId) -> AuditRow {
        AuditRow {
            channel: Some(to),
            ..row(id, AuditKind::Move, actor, count)
        }
    }

    fn left(t: UserId) -> Change {
        Change::Left {
            target: t,
            from: C1,
        }
    }

    fn moved(t: UserId, to: ChannelId) -> Change {
        Change::Moved {
            target: t,
            from: C1,
            to,
        }
    }

    fn disc(t: UserId, by: Attribution) -> ModLogEvent {
        ModLogEvent::Disconnect {
            target: t,
            from: C1,
            by,
        }
    }

    fn mov(t: UserId, to: ChannelId, by: Attribution) -> ModLogEvent {
        ModLogEvent::Move {
            target: t,
            from: C1,
            to,
            by,
        }
    }

    #[test]
    fn no_delta_logs_nothing() {
        let snap = seeded(&[(1, 3)]);
        let out = attribute(&snap, &[dc(1, M1, 3)], &[left(T1)], BOT);
        assert!(out.is_empty(), "a voluntary leave: {out:?}");
    }

    #[test]
    fn one_actor_one_leave_is_by() {
        let snap = seeded(&[(1, 3)]);
        let out = attribute(&snap, &[dc(1, M1, 4)], &[left(T1)], BOT);
        assert_eq!(out, vec![disc(T1, Attribution::By(M1))]);
    }

    #[test]
    fn two_actors_two_leaves_is_one_of() {
        let snap = seeded(&[(1, 1), (2, 1)]);
        let rows = [dc(2, M2, 2), dc(1, M1, 2)];
        let out = attribute(&snap, &rows, &[left(T2), left(T1)], BOT);
        let both = Attribution::OneOf(vec![M1, M2]);
        assert_eq!(
            out,
            vec![disc(T1, both.clone()), disc(T2, both)],
            "sorted by target, actors sorted"
        );
    }

    #[test]
    fn fewer_deltas_than_leaves_is_a_batch() {
        let snap = seeded(&[(1, 1)]);
        let out = attribute(&snap, &[dc(1, M1, 3)], &[left(T3), left(T1), left(T2)], BOT);
        assert_eq!(
            out,
            vec![ModLogEvent::DisconnectBatch {
                actors: vec![M1],
                count: 2,
                among: vec![T1, T2, T3],
            }]
        );
    }

    #[test]
    fn more_deltas_than_leaves_attributes_nothing() {
        let snap = seeded(&[(1, 1)]);
        let out = attribute(&snap, &[dc(1, M1, 5)], &[left(T1)], BOT);
        assert!(out.is_empty(), "a surplus is not self-contained: {out:?}");
    }

    #[test]
    fn bot_authored_rows_never_count() {
        let snap = seeded(&[]);
        let rows = [dc(1, BOT, 1)];
        assert!(attribute(&snap, &rows, &[left(T1)], BOT).is_empty());
        let rows = [
            dc(1, BOT, 4),
            AuditRow {
                actor: None,
                ..dc(2, M1, 1)
            },
            dc(3, M2, 1),
        ];
        let out = attribute(&snap, &rows, &[left(T1)], BOT);
        assert_eq!(out, vec![disc(T1, Attribution::By(M2))], "only M2 counts");
    }

    #[test]
    fn a_target_id_pairs_exactly() {
        let snap = seeded(&[]);
        let rows = [
            AuditRow {
                target: Some(T2),
                ..dc(1, M1, 1)
            },
            dc(2, M2, 1),
        ];
        let out = attribute(&snap, &rows, &[left(T1), left(T2)], BOT);
        assert_eq!(
            out,
            vec![disc(T2, Attribution::By(M1)), disc(T1, Attribution::By(M2)),],
            "T2 pairs to M1 exactly; the one unit left explains T1"
        );
    }

    #[test]
    fn moves_match_their_destination() {
        let snap = seeded(&[]);
        let rows = [mv(1, M1, 1, C2), mv(2, M2, 1, C3)];
        let out = attribute(&snap, &rows, &[moved(T2, C3), moved(T1, C2)], BOT);
        assert_eq!(
            out,
            vec![
                mov(T1, C2, Attribution::By(M1)),
                mov(T2, C3, Attribution::By(M2)),
            ]
        );
    }

    #[test]
    fn fewer_deltas_than_moves_logs_nothing() {
        let snap = seeded(&[]);
        let out = attribute(
            &snap,
            &[mv(1, M1, 1, C2)],
            &[moved(T1, C2), moved(T2, C2)],
            BOT,
        );
        assert!(out.is_empty(), "never a guess: {out:?}");
    }

    #[test]
    fn unseeded_snapshot_attributes_nothing() {
        let snap = Snapshots::default();
        let out = attribute(&snap, &[dc(1, M1, 1)], &[left(T1)], BOT);
        assert!(out.is_empty());
    }

    #[test]
    fn new_row_on_seeded_snapshot_counts_fully() {
        let snap = seeded(&[(1, 9)]);
        let rows = [dc(1, M1, 9), dc(2, M2, 2)];
        let out = attribute(&snap, &rows, &[left(T1), left(T2)], BOT);
        let m2 = Attribution::By(M2);
        assert_eq!(out, vec![disc(T1, m2.clone()), disc(T2, m2)]);
    }

    #[test]
    fn advance_records_counts_and_seeds() {
        let mut snaps = Snapshots::default();
        advance(
            &mut snaps,
            &[AuditKind::Disconnect, AuditKind::Move],
            &[dc(1, M1, 2), dc(2, BOT, 7), mv(5, M2, 1, C2)],
        );
        assert!(snaps.disconnect.seeded && snaps.moves.seeded);
        assert_eq!(snaps.disconnect.counts, HashMap::from([(1, 2), (2, 7)]));
        assert_eq!(snaps.moves.counts, HashMap::from([(5, 1)]));
        advance(&mut snaps, &[AuditKind::Disconnect], &[dc(1, M1, 3)]);
        assert_eq!(
            snaps.disconnect.counts,
            HashMap::from([(1, 3), (2, 7)]),
            "merge keeps ids not fetched"
        );
    }

    #[test]
    fn advance_prunes_to_the_hundred_highest_ids() {
        let mut snaps = Snapshots::default();
        let rows: Vec<AuditRow> = (1..=150).map(|i| dc(i, M1, 1)).collect();
        advance(&mut snaps, &[AuditKind::Disconnect], &rows);
        assert_eq!(snaps.disconnect.counts.len(), 100);
        assert!(snaps.disconnect.counts.contains_key(&150));
        assert!(snaps.disconnect.counts.contains_key(&51));
        assert!(!snaps.disconnect.counts.contains_key(&50));
    }

    fn targeted(r: AuditRow, t: UserId) -> AuditRow {
        AuditRow {
            target: Some(t),
            ..r
        }
    }

    #[test]
    fn targeted_row_never_pairs_across_kinds() {
        let snap = seeded(&[]);
        let rows = [targeted(dc(1, M1, 1), T1)];
        let changes = [moved(T1, C2), left(T3)];
        assert!(attribute(&snap, &rows, &changes, BOT).is_empty());
        let rows = [targeted(mv(1, M1, 1, C2), T1)];
        let changes = [left(T1), moved(T3, C2)];
        assert!(attribute(&snap, &rows, &changes, BOT).is_empty());
    }

    #[test]
    fn targeted_move_pairs_by_destination_through_two_hops() {
        let snap = seeded(&[]);
        let rows = [targeted(mv(1, M1, 1, C2), T1)];
        let hop1 = Change::Moved {
            target: T1,
            from: C1,
            to: C3,
        };
        let hop2 = Change::Moved {
            target: T1,
            from: C3,
            to: C2,
        };
        let out = attribute(&snap, &rows, &[hop1, hop2], BOT);
        assert_eq!(
            out,
            vec![ModLogEvent::Move {
                target: T1,
                from: C3,
                to: C2,
                by: Attribution::By(M1),
            }]
        );
    }

    #[test]
    fn incremented_targeted_row_pairs_nothing() {
        let snap = seeded(&[(1, 1)]);
        let rows = [targeted(dc(1, M1, 2), T1)];
        assert!(attribute(&snap, &rows, &[left(T1)], BOT).is_empty());
    }

    #[test]
    fn unobserved_target_adds_nothing_to_the_pool() {
        let snap = seeded(&[]);
        let rows = [targeted(dc(1, M1, 1), T2)];
        assert!(attribute(&snap, &rows, &[left(T1)], BOT).is_empty());
    }

    #[test]
    fn mods_disconnecting_the_bot_is_consumed_and_silent() {
        let mut snaps = seeded(&[]);
        let bot_left = Change::Left {
            target: BOT,
            from: C1,
        };
        let out = attribute(&snaps, &[dc(1, M1, 1)], &[bot_left], BOT);
        assert!(out.is_empty(), "{out:?}");
        advance(&mut snaps, &[AuditKind::Disconnect], &[dc(1, M1, 1)]);
        let out = attribute(&snaps, &[dc(1, M1, 1)], &[left(T1)], BOT);
        assert!(out.is_empty(), "a voluntary leave after it: {out:?}");
    }

    #[test]
    fn bot_change_counts_in_l_but_is_not_named() {
        let snap = seeded(&[]);
        let bot_left = Change::Left {
            target: BOT,
            from: C1,
        };
        let out = attribute(&snap, &[dc(1, M1, 2)], &[bot_left.clone(), left(T1)], BOT);
        assert_eq!(out, vec![disc(T1, Attribution::By(M1))]);
        let out = attribute(&snap, &[dc(1, M1, 1)], &[bot_left.clone(), left(T1)], BOT);
        assert!(out.is_empty(), "the one unit may be the bot's: {out:?}");
        let out = attribute(&snap, &[dc(1, M1, 2)], &[bot_left, left(T1), left(T2)], BOT);
        assert_eq!(
            out,
            vec![ModLogEvent::DisconnectBatch {
                actors: vec![M1],
                count: 1,
                among: vec![T1, T2],
            }],
            "the bot is not among them, and one unit is its own"
        );
    }

    #[test]
    fn a_batch_claims_one_unit_less_for_a_repeated_leaver() {
        let snap = seeded(&[]);
        let out = attribute(&snap, &[dc(1, M1, 2)], &[left(T1), left(T1), left(T2)], BOT);
        assert_eq!(
            out,
            vec![ModLogEvent::DisconnectBatch {
                actors: vec![M1],
                count: 1,
                among: vec![T1, T2],
            }]
        );
    }

    #[test]
    fn a_targeted_row_with_count_two_pairs_nothing() {
        let snap = seeded(&[]);
        let rows = [targeted(dc(1, M1, 2), T1)];
        let out = attribute(&snap, &rows, &[left(T1), left(T1)], BOT);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn advance_never_lowers_a_stored_count() {
        let mut snaps = seeded(&[(1, 5)]);
        advance(&mut snaps, &[AuditKind::Disconnect], &[dc(1, M1, 3)]);
        assert_eq!(snaps.disconnect.counts[&1], 5);
        let out = attribute(&snaps, &[dc(1, M1, 5)], &[left(T1), left(T2)], BOT);
        assert!(
            out.is_empty(),
            "a stale read must not replay units: {out:?}"
        );
    }

    #[test]
    fn mixed_batch_keeps_kinds_apart() {
        let snap = seeded(&[]);
        let dc_with_channel = AuditRow {
            channel: Some(C2),
            ..dc(2, M2, 1)
        };
        let rows = [mv(1, M1, 1, C2), dc_with_channel];
        let changes = [moved(T1, C2), left(T2), left(T3)];
        let out = attribute(&snap, &rows, &changes, BOT);
        assert_eq!(
            out,
            vec![
                ModLogEvent::DisconnectBatch {
                    actors: vec![M2],
                    count: 1,
                    among: vec![T2, T3],
                },
                mov(T1, C2, Attribution::By(M1)),
            ]
        );
    }

    #[test]
    fn a_kind_with_an_unseeded_snapshot_attributes_nothing() {
        let mut snap = seeded(&[]);
        snap.moves.seeded = false;
        let rows = [dc(1, M1, 1), mv(2, M2, 1, C2)];
        let out = attribute(&snap, &rows, &[left(T1), moved(T2, C2)], BOT);
        assert_eq!(out, vec![disc(T1, Attribution::By(M1))]);
    }

    #[test]
    fn a_count_below_the_snapshot_is_no_delta() {
        let snap = seeded(&[(1, 5)]);
        assert!(attribute(&snap, &[dc(1, M1, 3)], &[left(T1)], BOT).is_empty());
    }

    #[test]
    fn absurd_counts_cannot_wrap() {
        let snap = seeded(&[]);
        let rows = [dc(1, M1, u32::MAX), dc(2, M2, u32::MAX)];
        assert!(attribute(&snap, &rows, &[left(T1)], BOT).is_empty());
    }

    #[test]
    fn exact_pair_with_the_bot_is_consumed_and_silent() {
        let snap = seeded(&[]);
        let rows = [targeted(dc(1, M1, 1), BOT)];
        let bot_left = Change::Left {
            target: BOT,
            from: C1,
        };
        let out = attribute(&snap, &rows, &[bot_left, left(T1)], BOT);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn a_member_leaving_twice_is_listed_once_in_a_batch() {
        let snap = seeded(&[]);
        let changes = [left(T1), left(T1), left(T2), left(T3)];
        let out = attribute(&snap, &[dc(1, M1, 2)], &changes, BOT);
        assert_eq!(
            out,
            vec![ModLogEvent::DisconnectBatch {
                actors: vec![M1],
                count: 1,
                among: vec![T1, T2, T3],
            }]
        );
    }

    #[test]
    fn advance_leaves_an_unfetched_kind_unseeded() {
        let mut snaps = Snapshots::default();
        advance(&mut snaps, &[AuditKind::Disconnect], &[dc(1, M1, 2)]);
        assert!(snaps.disconnect.seeded);
        assert_eq!(snaps.moves, Snapshot::default());
    }

    #[test]
    fn advance_seeds_a_fetched_kind_with_no_rows() {
        let mut snaps = Snapshots::default();
        advance(&mut snaps, &[AuditKind::Move], &[]);
        assert!(snaps.moves.seeded && snaps.moves.counts.is_empty());
        assert!(!snaps.disconnect.seeded);
    }

    #[test]
    fn advance_ignores_rows_of_an_unfetched_kind() {
        let mut snaps = seeded(&[(5, 1)]);
        let before = snaps.clone();
        advance(&mut snaps, &[AuditKind::Disconnect], &[mv(9, M1, 4, C2)]);
        assert_eq!(snaps.moves, before.moves, "moves untouched");
        assert!(!snaps.disconnect.counts.contains_key(&9));
    }

    #[test]
    fn a_known_targeted_row_with_count_one_pairs_nothing() {
        let snap = seeded(&[(1, 0)]);
        let rows = [targeted(dc(1, M1, 1), T1)];
        assert!(attribute(&snap, &rows, &[left(T1)], BOT).is_empty());
    }
}
