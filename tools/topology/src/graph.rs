//! Aggregates placed references into unit-to-unit edges and external references.

use crate::model::{Edge, ExternalRef, Loc, Position, UnitId};
use crate::placement::{Placed, RefTarget};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub edges: Vec<Edge>,
    pub external: Vec<ExternalRef>,
    /// (owner, field, from)
    pub field_uses: Vec<(UnitId, String, UnitId)>,
}

/// Running count and the smallest location seen.
struct Tally {
    count: u32,
    first: Loc,
}

fn tally<K: Ord>(t: &mut BTreeMap<K, Tally>, key: K, loc: &Loc) {
    let e = t.entry(key).or_insert_with(|| Tally {
        count: 0,
        first: loc.clone(),
    });
    e.count += 1;
    if *loc < e.first {
        e.first = loc.clone();
    }
}

pub fn aggregate(placed: &Placed) -> Graph {
    let mut edges: BTreeMap<(UnitId, UnitId, Position), Tally> = BTreeMap::new();
    let mut external: BTreeMap<(UnitId, String, String, Position), Tally> = BTreeMap::new();
    let mut field_uses: BTreeSet<(UnitId, String, UnitId)> = BTreeSet::new();
    for r in &placed.refs {
        let to = match &r.target {
            RefTarget::Unit(t) | RefTarget::Module(t) => Some(t),
            RefTarget::Field { owner, field } => {
                field_uses.insert((owner.clone(), field.clone(), r.from.clone()));
                Some(owner)
            },
            RefTarget::External { package, symbol } => {
                let key = (r.from.clone(), package.clone(), symbol.clone(), r.position);
                tally(&mut external, key, &r.loc);
                None
            },
        };
        if let Some(to) = to.filter(|t| **t != r.from) {
            tally(&mut edges, (r.from.clone(), to.clone(), r.position), &r.loc);
        }
    }
    Graph {
        edges: edges
            .into_iter()
            .map(|((from, to, position), t)| Edge {
                from,
                to,
                position,
                count: t.count,
                first: t.first,
            })
            .collect(),
        external: external
            .into_iter()
            .map(|((from, package, symbol, position), t)| ExternalRef {
                from,
                package,
                symbol,
                position,
                count: t.count,
                first: t.first,
            })
            .collect(),
        field_uses: field_uses.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::RawRef;

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    fn r(from: &str, target: RefTarget, position: Position, file: &str, line: u32) -> RawRef {
        RawRef {
            from: id(from),
            target,
            position,
            loc: Loc {
                file: file.into(),
                line,
            },
        }
    }

    fn unit(t: &str) -> RefTarget {
        RefTarget::Unit(id(t))
    }

    #[test]
    fn counts_sum_and_first_is_the_smallest_location() {
        let b = Position::Body;
        let placed = Placed {
            refs: vec![
                r("a", unit("b"), b, "z.rs", 5),
                r("a", unit("b"), b, "z.rs", 3),
                r("a", unit("b"), b, "z.rs", 9),
                r("a", unit("b"), b, "a.rs", 40),
                r("a", unit("b"), Position::Signature, "z.rs", 7),
            ],
            ..Placed::default()
        };
        let g = aggregate(&placed);
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[0].position, Position::Signature);
        assert_eq!(g.edges[0].count, 1);
        assert_eq!(g.edges[1].count, 4);
        assert_eq!(
            g.edges[1].first,
            Loc {
                file: "a.rs".into(),
                line: 40
            }
        );
    }

    #[test]
    fn first_location_ignores_later_larger_ones() {
        let placed = Placed {
            refs: vec![
                r("a", unit("b"), Position::Body, "z.rs", 3),
                r("a", unit("b"), Position::Body, "z.rs", 9),
            ],
            ..Placed::default()
        };
        assert_eq!(aggregate(&placed).edges[0].first.line, 3);
    }

    #[test]
    fn self_edges_dropped_module_and_field_targets_edge() {
        let b = Position::Body;
        let placed = Placed {
            refs: vec![
                r("a", unit("a"), b, "f.rs", 1),
                r("a", RefTarget::Module(id("m")), b, "f.rs", 2),
                r(
                    "a",
                    RefTarget::Field {
                        owner: id("o"),
                        field: "x".into(),
                    },
                    b,
                    "f.rs",
                    3,
                ),
                r(
                    "a",
                    RefTarget::Field {
                        owner: id("o"),
                        field: "x".into(),
                    },
                    b,
                    "f.rs",
                    4,
                ),
                r(
                    "o",
                    RefTarget::Field {
                        owner: id("o"),
                        field: "y".into(),
                    },
                    b,
                    "f.rs",
                    5,
                ),
            ],
            ..Placed::default()
        };
        let g = aggregate(&placed);
        let edges: Vec<_> = g
            .edges
            .iter()
            .map(|e| (e.from.0.as_str(), e.to.0.as_str(), e.count))
            .collect();
        assert_eq!(edges, vec![("a", "m", 1), ("a", "o", 2)]);
        assert_eq!(
            g.field_uses,
            vec![
                (id("o"), "x".into(), id("a")),
                (id("o"), "y".into(), id("o"))
            ]
        );
        assert!(g.external.is_empty());
    }

    #[test]
    fn external_refs_aggregate_per_symbol_and_position() {
        let ext = |sym: &str| RefTarget::External {
            package: "p".into(),
            symbol: sym.into(),
        };
        let placed = Placed {
            refs: vec![
                r("a", ext("S#"), Position::Body, "f.rs", 8),
                r("a", ext("S#"), Position::Body, "f.rs", 2),
                r("a", ext("S#"), Position::Signature, "f.rs", 1),
                r("a", ext("T#"), Position::Body, "f.rs", 4),
                r("b", ext("S#"), Position::Body, "f.rs", 4),
            ],
            ..Placed::default()
        };
        let g = aggregate(&placed);
        assert!(g.edges.is_empty());
        let got: Vec<_> = g
            .external
            .iter()
            .map(|e| {
                (
                    e.from.0.as_str(),
                    e.symbol.as_str(),
                    e.position,
                    e.count,
                    e.first.line,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("a", "S#", Position::Signature, 1, 1),
                ("a", "S#", Position::Body, 2, 2),
                ("a", "T#", Position::Body, 1, 4),
                ("b", "S#", Position::Body, 1, 4),
            ]
        );
    }
}
