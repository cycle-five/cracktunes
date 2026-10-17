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

/// A voice change the bot observed.
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
}

/// Record every row's count and mark the snapshot seeded. The counts are
/// replaced by the rows given: ids that fell out of the fetch window go.
pub fn advance(snap: &mut Snapshot, rows: &[AuditRow]) {
    snap.counts = rows.iter().map(|r| (r.id, r.count)).collect();
    snap.seeded = true;
}

/// A row's new units: its whole count when unseen, else the rise.
fn delta(snap: &Snapshot, row: &AuditRow) -> u32 {
    match snap.counts.get(&row.id) {
        Some(prev) => row.count.saturating_sub(*prev),
        None => row.count,
    }
}

/// What to report for a group of changes given the actors' units.
/// `batch_ok` is true for leaves, where too few units become a batch.
fn decide(changes: &[&Change], units: &BTreeMap<UserId, u32>, batch_ok: bool) -> Vec<ModLogEvent> {
    let d: u32 = units.values().sum();
    let l = changes.len();
    if d == 0 || l == 0 {
        if d > 0 {
            tracing::debug!("modlog: {d} audit units with no change to explain");
        }
        return Vec::new();
    }
    let actors: Vec<UserId> = units.keys().copied().collect();
    if (d as usize) < l {
        if !batch_ok {
            tracing::debug!("modlog: {d} audit units for {l} moves: not attributed");
            return Vec::new();
        }
        return vec![ModLogEvent::DisconnectBatch {
            actors,
            count: d,
            among: changes.iter().map(|c| c.target()).collect(),
        }];
    }
    if (d as usize) > l {
        tracing::debug!(
            "modlog: {} audit units beyond the {l} changes",
            d as usize - l
        );
    }
    let by = || match actors.as_slice() {
        [one] => Attribution::By(*one),
        _ => Attribution::OneOf(actors.clone()),
    };
    changes
        .iter()
        .map(|c| match c {
            Change::Left { target, from } => ModLogEvent::Disconnect {
                target: *target,
                from: *from,
                by: by(),
            },
            Change::Moved { target, from, to } => ModLogEvent::Move {
                target: *target,
                from: *from,
                to: *to,
                by: by(),
            },
        })
        .collect()
}

/// Turn audit deltas and observed voice changes into events (spec 2.3).
/// Output order is stable: leaves, then moves by destination id; within a
/// group by target id.
#[must_use]
pub fn attribute(
    snap: &Snapshot,
    rows: &[AuditRow],
    changes: &[Change],
    bot: UserId,
) -> Vec<ModLogEvent> {
    if !snap.seeded {
        return Vec::new();
    }
    let mut sorted_changes: Vec<&Change> = changes.iter().collect();
    sorted_changes.sort_by_key(|c| c.target());
    let mut paired = vec![false; sorted_changes.len()];
    let mut events: Vec<ModLogEvent> = Vec::new();

    // (row, remaining units), bot-authored and unauthored rows never count.
    let mut live: Vec<(&AuditRow, UserId, u32)> = rows
        .iter()
        .filter_map(|r| {
            let actor = r.actor.filter(|a| *a != bot)?;
            let d = delta(snap, r);
            (d > 0).then_some((r, actor, d))
        })
        .collect();
    live.sort_by_key(|(r, ..)| r.id);

    // Exact pairs.
    for (row, actor, units) in &mut live {
        let Some(target) = row.target else { continue };
        let hit = sorted_changes
            .iter()
            .enumerate()
            .find(|(i, c)| !paired[*i] && c.target() == target && c.kind() == row.kind);
        if let Some((i, c)) = hit {
            paired[i] = true;
            *units -= 1;
            events.push(match c {
                Change::Left { target, from } => ModLogEvent::Disconnect {
                    target: *target,
                    from: *from,
                    by: Attribution::By(*actor),
                },
                Change::Moved { target, from, to } => ModLogEvent::Move {
                    target: *target,
                    from: *from,
                    to: *to,
                    by: Attribution::By(*actor),
                },
            });
        }
    }

    // Leaves: one group.
    let leaves: Vec<&Change> = sorted_changes
        .iter()
        .enumerate()
        .filter(|(i, c)| !paired[*i] && c.kind() == AuditKind::Disconnect)
        .map(|(_, c)| *c)
        .collect();
    let mut units: BTreeMap<UserId, u32> = BTreeMap::new();
    for (row, actor, u) in &live {
        if row.kind == AuditKind::Disconnect && *u > 0 {
            *units.entry(*actor).or_default() += u;
        }
    }
    events.extend(decide(&leaves, &units, true));

    // Moves: one group per destination.
    let mut by_dest: BTreeMap<ChannelId, Vec<&Change>> = BTreeMap::new();
    for (i, c) in sorted_changes.iter().enumerate() {
        if let (false, Change::Moved { to, .. }) = (paired[i], c) {
            by_dest.entry(*to).or_default().push(c);
        }
    }
    for (dest, group) in &by_dest {
        let mut units: BTreeMap<UserId, u32> = BTreeMap::new();
        for (row, actor, u) in &live {
            if row.kind == AuditKind::Move && row.channel == Some(*dest) && *u > 0 {
                *units.entry(*actor).or_default() += u;
            }
        }
        events.extend(decide(group, &units, false));
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

    fn seeded(counts: &[(u64, u32)]) -> Snapshot {
        Snapshot {
            seeded: true,
            counts: counts.iter().copied().collect(),
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
    fn more_deltas_than_leaves_attributes_all() {
        let snap = seeded(&[(1, 1)]);
        let out = attribute(&snap, &[dc(1, M1, 5)], &[left(T1)], BOT);
        assert_eq!(out, vec![disc(T1, Attribution::By(M1))]);
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
        let snap = Snapshot::default();
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
        let mut snap = Snapshot::default();
        advance(&mut snap, &[dc(1, M1, 2), dc(2, BOT, 7)]);
        assert!(snap.seeded);
        assert_eq!(snap.counts, HashMap::from([(1, 2), (2, 7)]));
        advance(&mut snap, &[dc(1, M1, 3)]);
        assert_eq!(snap.counts, HashMap::from([(1, 3)]));
    }
}
