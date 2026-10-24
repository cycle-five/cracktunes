//! Per-group taint: which units touch a group's packages, how, and how far.

use crate::config::Config;
use crate::graph::Graph;
use crate::model::{
    Edge, Evidence, ExternalRef, Loc, Position, State, StateLines, Unit, UnitId, UnitKind,
    UnitTaint,
};
use crate::placement::{Placed, RefTarget};
use crate::structure::Structure;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Taint {
    /// group -> unit -> taint, for every non-module unit.
    pub by_group: BTreeMap<String, BTreeMap<UnitId, UnitTaint>>,
    pub module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seam {
    pub unit: UnitId,
    pub state: State,
    pub cuts: u32,
    pub lines: u32,
}

fn is_type(kind: UnitKind) -> bool {
    matches!(
        kind,
        UnitKind::Struct | UnitKind::Enum | UnitKind::Union | UnitKind::TypeAlias | UnitKind::Trait
    )
}

pub fn compute(structure: &Structure, placed: &Placed, graph: &Graph, cfg: &Config) -> Taint {
    let units: BTreeMap<&UnitId, &Unit> = structure
        .units
        .iter()
        .map(|d| (&d.unit.id, &d.unit))
        .collect();
    let is_module = |u: &UnitId| units.get(u).is_some_and(|u| u.kind == UnitKind::Module);
    // Workspace edges without the ones into modules.
    let edges: Vec<_> = graph.edges.iter().filter(|e| !is_module(&e.to)).collect();
    let mut out_edges: BTreeMap<&UnitId, Vec<&Edge>> = BTreeMap::new();
    for e in &edges {
        out_edges.entry(&e.from).or_default().push(e);
    }
    let package_of: BTreeMap<&str, &str> = structure
        .crates
        .iter()
        .map(|c| (c.id.as_str(), c.package.as_str()))
        .collect();
    let mut out = Taint::default();
    for group in cfg.groups.keys() {
        let in_group = |package: &str| cfg.group_of(package) == Some(group.as_str());
        let direct: Vec<_> = graph
            .external
            .iter()
            .filter(|e| in_group(&e.package))
            .collect();
        let mut direct_by: BTreeMap<&UnitId, Vec<&ExternalRef>> = BTreeMap::new();
        for e in &direct {
            direct_by.entry(&e.from).or_default().push(e);
        }
        // Units of a package that is itself in this group have no entry for it.
        let skip = |u: &Unit| {
            package_of
                .get(u.krate.as_str())
                .is_some_and(|p| in_group(p))
        };
        let direct_sig: BTreeSet<&UnitId> = direct
            .iter()
            .filter(|e| e.position == Position::Signature)
            .map(|e| &e.from)
            .collect();
        let direct_body: BTreeSet<&UnitId> = direct
            .iter()
            .filter(|e| e.position == Position::Body)
            .map(|e| &e.from)
            .collect();
        let mut lines_of: BTreeMap<&UnitId, BTreeSet<(&str, u32)>> = BTreeMap::new();
        for r in &placed.refs {
            if let RefTarget::External { package, .. } = &r.target
                && in_group(package)
            {
                lines_of
                    .entry(&r.from)
                    .or_default()
                    .insert((r.loc.file.as_str(), r.loc.line));
            }
        }

        // Types whose signature mentions the group, directly or through another such type.
        let mut tainted: BTreeSet<&UnitId> = units
            .values()
            .filter(|u| is_type(u.kind) && direct_sig.contains(&u.id))
            .map(|u| &u.id)
            .collect();
        loop {
            let before = tainted.len();
            for e in &edges {
                if e.position == Position::Signature
                    && tainted.contains(&e.to)
                    && units.get(&e.from).is_some_and(|u| is_type(u.kind))
                {
                    tainted.insert(&e.from);
                }
            }
            if tainted.len() == before {
                break;
            }
        }
        let mut own_sig: BTreeSet<&UnitId> = direct_sig.clone();
        own_sig.extend(
            edges
                .iter()
                .filter(|e| e.position == Position::Signature && tainted.contains(&e.to))
                .map(|e| &e.from),
        );
        let sig: BTreeSet<&UnitId> = units
            .values()
            .filter(|u| u.kind != UnitKind::Module)
            .filter(|u| {
                own_sig.contains(&u.id)
                    || (u.kind == UnitKind::Method
                        && u.parent.as_ref().is_some_and(|p| own_sig.contains(p)))
            })
            .map(|u| &u.id)
            .collect();

        // Everything that reaches a sig or body unit, by walking edges backwards.
        let mut reverse: BTreeMap<&UnitId, Vec<&UnitId>> = BTreeMap::new();
        for e in &edges {
            reverse.entry(&e.to).or_default().push(&e.from);
        }
        let mut reaches: BTreeSet<&UnitId> = BTreeSet::new();
        let mut queue: VecDeque<&UnitId> = sig.iter().chain(direct_body.iter()).copied().collect();
        let mut seen: BTreeSet<&UnitId> = queue.iter().copied().collect();
        while let Some(n) = queue.pop_front() {
            for &p in reverse.get(n).into_iter().flatten() {
                reaches.insert(p);
                if seen.insert(p) {
                    queue.push_back(p);
                }
            }
        }

        let state_of = |u: &UnitId| {
            if sig.contains(u) {
                State::Signature
            } else if direct_body.contains(u) {
                State::Body
            } else if reaches.contains(u) {
                State::Inherited
            } else {
                State::Clean
            }
        };

        let mut by_unit = BTreeMap::new();
        for u in units
            .values()
            .filter(|u| u.kind != UnitKind::Module && !skip(u))
        {
            let state = state_of(&u.id);
            let mut via: BTreeSet<UnitId> = out_edges
                .get(&u.id)
                .into_iter()
                .flatten()
                .filter(|e| state_of(&e.to) != State::Clean)
                .map(|e| e.to.clone())
                .collect();
            if let Some(p) = u
                .parent
                .as_ref()
                .filter(|p| u.kind == UnitKind::Method && own_sig.contains(p))
            {
                via.insert(p.clone());
            }
            let mut ev: BTreeMap<(&str, &str, Position), (u32, &Loc)> = BTreeMap::new();
            for e in direct_by.get(&u.id).into_iter().flatten() {
                ev.insert((&e.package, &e.symbol, e.position), (e.count, &e.first));
            }
            let evidence: Vec<Evidence> = ev
                .into_iter()
                .map(|((package, symbol, position), (count, first))| Evidence {
                    package: package.into(),
                    symbol: symbol.into(),
                    position,
                    count,
                    first: first.clone(),
                })
                .collect();
            let cuts = if state == State::Clean {
                0
            } else {
                lines_of.get(&u.id).map_or(0, BTreeSet::len) as u32 + via.len() as u32
            };
            by_unit.insert(
                u.id.clone(),
                UnitTaint {
                    state,
                    cuts,
                    direct: evidence,
                    via: via.into_iter().collect(),
                },
            );
        }

        let mut lines: BTreeMap<UnitId, StateLines> = units
            .values()
            .filter(|u| u.kind == UnitKind::Module && !skip(u))
            .map(|u| {
                (
                    u.id.clone(),
                    StateLines {
                        clean: 0,
                        signature: 0,
                        body: 0,
                        inherited: 0,
                    },
                )
            })
            .collect();
        for u in units
            .values()
            .filter(|u| u.kind != UnitKind::Module && !u.test && !skip(u))
        {
            let Some(m) = &u.module else { continue };
            let l = lines.entry(m.clone()).or_insert(StateLines {
                clean: 0,
                signature: 0,
                body: 0,
                inherited: 0,
            });
            match by_unit[&u.id].state {
                State::Clean => l.clean += u.own_lines,
                State::Signature => l.signature += u.own_lines,
                State::Body => l.body += u.own_lines,
                State::Inherited => l.inherited += u.own_lines,
            }
        }
        out.by_group.insert(group.clone(), by_unit);
        out.module_lines.insert(group.clone(), lines);
    }
    out
}

/// Non-test, tainted units at most `max_cuts` cuts from clean; cheapest and biggest first.
pub fn seams(
    group_taint: &BTreeMap<UnitId, UnitTaint>,
    units: &[Unit],
    max_cuts: u32,
) -> Vec<Seam> {
    let mut out: Vec<Seam> = units
        .iter()
        .filter(|u| !u.test && u.kind != UnitKind::Module)
        .filter_map(|u| {
            let t = group_taint.get(&u.id)?;
            (t.state != State::Clean && t.cuts <= max_cuts).then(|| Seam {
                unit: u.id.clone(),
                state: t.state,
                cuts: t.cuts,
                lines: u.lines,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.cuts
            .cmp(&b.cuts)
            .then(b.lines.cmp(&a.lines))
            .then(a.unit.cmp(&b.unit))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::aggregate;
    use crate::placement::RawRef;
    use crate::structure::{Pos, Span, UnitDraft};

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    fn unit(name: &str, kind: UnitKind) -> Unit {
        Unit {
            id: id(name),
            kind,
            krate: "c".into(),
            module: Some(id("c")),
            parent: None,
            file: "f.rs".into(),
            start_line: 1,
            end_line: 1,
            lines: 5,
            own_lines: 5,
            test: false,
            features: BTreeSet::new(),
            hash: String::new(),
        }
    }

    fn draft(unit: Unit) -> UnitDraft {
        let p = Pos { line: 1, col: 0 };
        UnitDraft {
            unit,
            ident: None,
            span: Span { start: p, end: p },
            body: None,
        }
    }

    fn structure(units: Vec<Unit>) -> Structure {
        Structure {
            units: units.into_iter().map(draft).collect(),
            ..Structure::default()
        }
    }

    fn cfg() -> Config {
        Config::from_toml_str("[groups]\ng = [\"ext\"]\n").unwrap()
    }

    fn r(from: &str, target: RefTarget, position: Position, line: u32) -> RawRef {
        RawRef {
            from: id(from),
            target,
            position,
            loc: Loc {
                file: "f.rs".into(),
                line,
            },
        }
    }

    fn ext(from: &str, position: Position, line: u32) -> RawRef {
        r(
            from,
            RefTarget::External {
                package: "ext".into(),
                symbol: "S".into(),
            },
            position,
            line,
        )
    }

    fn uref(from: &str, to: &str, position: Position, line: u32) -> RawRef {
        r(from, RefTarget::Unit(id(to)), position, line)
    }

    fn run(units: Vec<Unit>, refs: Vec<RawRef>) -> BTreeMap<UnitId, UnitTaint> {
        let placed = Placed {
            refs,
            ..Placed::default()
        };
        let graph = aggregate(&placed);
        compute(&structure(units), &placed, &graph, &cfg())
            .by_group
            .remove("g")
            .unwrap()
    }

    #[test]
    fn fixpoint_through_two_types() {
        let t = run(
            vec![
                unit("c::A", UnitKind::Struct),
                unit("c::B", UnitKind::Struct),
                unit("c::f", UnitKind::Function),
            ],
            vec![
                ext("c::B", Position::Signature, 2),
                uref("c::A", "c::B", Position::Signature, 1),
                uref("c::f", "c::A", Position::Signature, 3),
            ],
        );
        assert_eq!(t[&id("c::B")].state, State::Signature);
        assert_eq!(t[&id("c::A")].state, State::Signature);
        assert_eq!(t[&id("c::f")].state, State::Signature);
        assert_eq!(t[&id("c::f")].via, vec![id("c::A")]);
    }

    #[test]
    fn body_edge_to_a_tainted_type_is_not_signature() {
        let t = run(
            vec![
                unit("c::B", UnitKind::Struct),
                unit("c::f", UnitKind::Function),
            ],
            vec![
                ext("c::B", Position::Signature, 2),
                uref("c::f", "c::B", Position::Body, 3),
            ],
        );
        assert_eq!(t[&id("c::f")].state, State::Inherited);
    }

    #[test]
    fn a_function_signature_does_not_taint_a_type() {
        // A function mentioning the group is not a type, so users of it only inherit.
        let t = run(
            vec![
                unit("c::g", UnitKind::Function),
                unit("c::S", UnitKind::Struct),
            ],
            vec![
                ext("c::g", Position::Signature, 1),
                uref("c::S", "c::g", Position::Signature, 2),
            ],
        );
        assert_eq!(t[&id("c::S")].state, State::Inherited);
        let t = run(
            vec![
                unit("c::g", UnitKind::Function),
                unit("c::S", UnitKind::Struct),
                unit("c::h", UnitKind::Function),
            ],
            vec![
                ext("c::g", Position::Signature, 1),
                uref("c::h", "c::S", Position::Signature, 2),
                uref("c::S", "c::g", Position::Body, 3),
            ],
        );
        assert_eq!(t[&id("c::h")].state, State::Inherited);
    }

    #[test]
    fn a_body_edge_from_a_type_does_not_taint_it() {
        let t = run(
            vec![
                unit("c::B", UnitKind::Struct),
                unit("c::X", UnitKind::Struct),
                unit("c::f", UnitKind::Function),
            ],
            vec![
                ext("c::B", Position::Signature, 1),
                uref("c::X", "c::B", Position::Body, 2),
                uref("c::f", "c::X", Position::Signature, 3),
            ],
        );
        assert_eq!(t[&id("c::f")].state, State::Inherited);
    }

    #[test]
    fn inherited_is_transitive() {
        let t = run(
            vec![
                unit("c::u", UnitKind::Function),
                unit("c::v", UnitKind::Function),
                unit("c::w", UnitKind::Function),
            ],
            vec![
                uref("c::u", "c::v", Position::Body, 1),
                uref("c::v", "c::w", Position::Body, 2),
                ext("c::w", Position::Body, 3),
            ],
        );
        assert_eq!(t[&id("c::w")].state, State::Body);
        assert_eq!(t[&id("c::v")].state, State::Inherited);
        assert_eq!(t[&id("c::u")].state, State::Inherited);
        assert_eq!(t[&id("c::u")].via, vec![id("c::v")]);
        assert_eq!(t[&id("c::u")].cuts, 1);
        assert!(t[&id("c::u")].direct.is_empty());
    }

    #[test]
    fn cycles_without_taint_stay_clean() {
        let t = run(
            vec![
                unit("c::a", UnitKind::Function),
                unit("c::b", UnitKind::Function),
            ],
            vec![
                uref("c::a", "c::b", Position::Body, 1),
                uref("c::b", "c::a", Position::Body, 2),
            ],
        );
        assert_eq!(t[&id("c::a")].state, State::Clean);
        assert_eq!(t[&id("c::b")].cuts, 0);
    }

    #[test]
    fn module_edges_do_not_taint() {
        let mut m = unit("c::m", UnitKind::Module);
        m.module = None;
        let t = run(
            vec![m, unit("c::f", UnitKind::Function)],
            vec![
                ext("c::m", Position::Body, 1),
                r("c::f", RefTarget::Module(id("c::m")), Position::Body, 2),
            ],
        );
        assert_eq!(t[&id("c::f")].state, State::Clean);
        assert!(!t.contains_key(&id("c::m")));
    }

    #[test]
    fn method_takes_signature_from_parent_and_lists_it_in_via() {
        let mut m = unit("c::<impl T for X>::go", UnitKind::Method);
        m.parent = Some(id("c::<impl T for X>"));
        let t = run(
            vec![unit("c::<impl T for X>", UnitKind::Impl), m],
            vec![ext("c::<impl T for X>", Position::Signature, 1)],
        );
        let m = &t[&id("c::<impl T for X>::go")];
        assert_eq!(m.state, State::Signature);
        assert_eq!(m.via, vec![id("c::<impl T for X>")]);
        assert_eq!(m.cuts, 1);
    }

    #[test]
    fn method_takes_signature_from_a_parent_tainted_through_a_type() {
        let mut m = unit("c::<impl X>::go", UnitKind::Method);
        m.parent = Some(id("c::<impl X>"));
        let t = run(
            vec![
                unit("c::B", UnitKind::Struct),
                unit("c::<impl X>", UnitKind::Impl),
                m,
            ],
            vec![
                ext("c::B", Position::Signature, 1),
                uref("c::<impl X>", "c::B", Position::Signature, 2),
            ],
        );
        let m = &t[&id("c::<impl X>::go")];
        assert_eq!(m.state, State::Signature);
        assert_eq!(m.via, vec![id("c::<impl X>")]);
    }

    #[test]
    fn a_parent_that_is_not_a_method_parent_gives_nothing() {
        let mut f = unit("c::f", UnitKind::Function);
        f.parent = Some(id("c::I"));
        let t = run(
            vec![unit("c::I", UnitKind::Impl), f],
            vec![ext("c::I", Position::Signature, 1)],
        );
        assert_eq!(t[&id("c::f")].state, State::Clean);
        assert!(t[&id("c::f")].via.is_empty());
    }

    #[test]
    fn direct_aggregates_per_symbol_and_position_with_first_loc() {
        let t = run(
            vec![unit("c::f", UnitKind::Function)],
            vec![
                ext("c::f", Position::Body, 9),
                ext("c::f", Position::Body, 4),
                ext("c::f", Position::Body, 4),
                ext("c::f", Position::Signature, 1),
            ],
        );
        let f = &t[&id("c::f")];
        assert_eq!(f.state, State::Signature);
        assert_eq!(f.direct.len(), 2);
        assert_eq!(f.direct[0].position, Position::Signature);
        assert_eq!((f.direct[1].count, f.direct[1].first.line), (3, 4));
        // Lines 1, 4 and 9 are distinct; two refs on line 4 are one cut.
        assert_eq!(f.cuts, 3);
    }

    #[test]
    fn other_groups_packages_do_not_count() {
        let t = run(
            vec![unit("c::f", UnitKind::Function)],
            vec![r(
                "c::f",
                RefTarget::External {
                    package: "elsewhere".into(),
                    symbol: "S".into(),
                },
                Position::Body,
                1,
            )],
        );
        assert_eq!(t[&id("c::f")].state, State::Clean);
        assert!(t[&id("c::f")].direct.is_empty());
    }

    #[test]
    fn module_lines_sum_own_lines() {
        let mut m = unit("c", UnitKind::Module);
        m.module = None;
        let mut empty = unit("c::empty", UnitKind::Module);
        empty.module = Some(id("c"));
        let mut a = unit("c::a", UnitKind::Function);
        a.own_lines = 3;
        let mut b = unit("c::b", UnitKind::Function);
        b.own_lines = 7;
        let mut c = unit("c::c", UnitKind::Function);
        c.own_lines = 11;
        let mut d = unit("c::d", UnitKind::Function);
        d.own_lines = 13;
        let mut tst = unit("c::t", UnitKind::Function);
        tst.test = true;
        tst.own_lines = 100;
        let placed = Placed {
            refs: vec![
                ext("c::a", Position::Signature, 1),
                ext("c::b", Position::Body, 2),
                uref("c::c", "c::b", Position::Body, 3),
                ext("c::t", Position::Body, 4),
            ],
            ..Placed::default()
        };
        let graph = aggregate(&placed);
        let t = compute(
            &structure(vec![m, empty, a, b, c, d, tst]),
            &placed,
            &graph,
            &cfg(),
        );
        assert_eq!(
            t.module_lines["g"][&id("c")],
            StateLines {
                clean: 13,
                signature: 3,
                body: 7,
                inherited: 11
            }
        );
        assert_eq!(
            t.module_lines["g"].keys().collect::<Vec<_>>(),
            vec![&id("c"), &id("c::empty")]
        );
        assert_eq!(t.module_lines["g"][&id("c::empty")].clean, 0);
    }

    #[test]
    fn seams_filter_and_sort() {
        let mut units = vec![
            unit("c::big", UnitKind::Function),
            unit("c::small", UnitKind::Function),
            unit("c::z", UnitKind::Function),
            unit("c::a", UnitKind::Function),
            unit("c::costly", UnitKind::Function),
            unit("c::clean", UnitKind::Function),
            unit("c::t", UnitKind::Function),
            unit("c::m", UnitKind::Module),
        ];
        units[0].lines = 50;
        units[1].lines = 10;
        units[6].test = true;
        let mk = |state, cuts| UnitTaint {
            state,
            cuts,
            direct: vec![],
            via: vec![],
        };
        let t: BTreeMap<_, _> = [
            ("c::big", mk(State::Body, 1)),
            ("c::small", mk(State::Body, 1)),
            ("c::z", mk(State::Body, 0)),
            ("c::a", mk(State::Body, 1)),
            ("c::costly", mk(State::Body, 3)),
            ("c::clean", mk(State::Clean, 0)),
            ("c::t", mk(State::Body, 1)),
            ("c::m", mk(State::Body, 1)),
        ]
        .into_iter()
        .map(|(k, v)| (id(k), v))
        .collect();
        let got: Vec<_> = seams(&t, &units, 2)
            .into_iter()
            .map(|s| (s.unit.0, s.lines))
            .collect();
        assert_eq!(
            got,
            vec![
                ("c::z".to_string(), 5),
                ("c::big".to_string(), 50),
                ("c::small".to_string(), 10),
                ("c::a".to_string(), 5),
            ]
        );
        assert_eq!(seams(&t, &units, 3).len(), 5);
    }
}
