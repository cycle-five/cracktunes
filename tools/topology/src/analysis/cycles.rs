//! Cycles in the module graph, at full depth and collapsed to top-level modules.

use crate::graph::Graph;
use crate::model::{Cycle, Cycles, Loc, ModuleEdge, UnitId, UnitKind};
use crate::structure::Structure;
use std::collections::BTreeMap;

type Key = (UnitId, UnitId);
type Edges = BTreeMap<Key, (u32, Loc)>;

pub fn compute(structure: &Structure, graph: &Graph) -> Cycles {
    let units: BTreeMap<&UnitId, _> = structure
        .units
        .iter()
        .map(|d| (&d.unit.id, &d.unit))
        .collect();
    // Edges into modules come from path segments and `mod x;`: ignored (R6).
    let mut edges: Edges = BTreeMap::new();
    let mut krate_of: BTreeMap<UnitId, &str> = BTreeMap::new();
    for e in &graph.edges {
        let (Some(f), Some(t)) = (units.get(&e.from), units.get(&e.to)) else {
            continue;
        };
        if f.test || t.test || t.kind == UnitKind::Module || f.krate != t.krate {
            continue;
        }
        let fm = if f.kind == UnitKind::Module {
            Some(&f.id)
        } else {
            f.module.as_ref()
        };
        let Some(fm) = fm else { continue };
        let Some(tm) = t.module.as_ref() else {
            continue;
        };
        if fm == tm {
            continue;
        }
        krate_of.insert(fm.clone(), &f.krate);
        krate_of.insert(tm.clone(), &f.krate);
        add(&mut edges, fm.clone(), tm.clone(), e.count, &e.first);
    }
    let by_module = cycles_of(&edges, &krate_of);
    let mut top: Edges = BTreeMap::new();
    let mut top_krate: BTreeMap<UnitId, &str> = BTreeMap::new();
    for ((f, t), (count, first)) in &edges {
        let krate = krate_of[f];
        let (cf, ct) = (collapse(f, krate), collapse(t, krate));
        if cf != ct {
            top_krate.insert(cf.clone(), krate);
            top_krate.insert(ct.clone(), krate);
            add(&mut top, cf, ct, *count, first);
        }
    }
    let by_top_module = cycles_of(&top, &top_krate);
    Cycles {
        by_module,
        by_top_module,
    }
}

fn add(edges: &mut Edges, f: UnitId, t: UnitId, count: u32, first: &Loc) {
    let e = edges.entry((f, t)).or_insert((0, first.clone()));
    e.0 += count;
    // `<` vs `<=` is equivalent: an equal location replaces itself.
    if *first < e.1 {
        e.1 = first.clone();
    }
}

/// `krate::a::b::c` -> `krate::a`; the root stays the root.
fn collapse(module: &UnitId, krate: &str) -> UnitId {
    let rest = module
        .0
        .strip_prefix(krate)
        .and_then(|r| r.strip_prefix("::"));
    match rest {
        Some(r) => UnitId(format!("{krate}::{}", r.split("::").next().unwrap_or(r))),
        None => module.clone(),
    }
}

fn cycles_of(edges: &Edges, krate_of: &BTreeMap<UnitId, &str>) -> Vec<Cycle> {
    let mut index: BTreeMap<&UnitId, usize> = BTreeMap::new();
    let mut names: Vec<&UnitId> = Vec::new();
    for (f, t) in edges.keys() {
        for m in [f, t] {
            index.entry(m).or_insert_with(|| {
                names.push(m);
                names.len() - 1
            });
        }
    }
    let mut adj = vec![Vec::new(); names.len()];
    for (f, t) in edges.keys() {
        adj[index[f]].push(index[t]);
    }
    let mut out = Vec::new();
    for comp in scc(&adj).into_iter().filter(|c| c.len() > 1) {
        let mut modules: Vec<UnitId> = comp.iter().map(|&i| names[i].clone()).collect();
        modules.sort();
        let internal = edges
            .iter()
            .filter(|((f, t), _)| {
                modules.binary_search(f).is_ok() && modules.binary_search(t).is_ok()
            })
            .map(|((f, t), (count, first))| ModuleEdge {
                from: f.clone(),
                to: t.clone(),
                count: *count,
                first: first.clone(),
            })
            .collect();
        out.push(Cycle {
            krate: krate_of[&modules[0]].to_string(),
            modules,
            edges: internal,
        });
    }
    out.sort_by(|a, b| (&a.krate, &a.modules[0]).cmp(&(&b.krate, &b.modules[0])));
    out
}

/// Strongly connected components, iterative Tarjan: linear in nodes plus edges.
pub(crate) fn scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut idx = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on = vec![false; n];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut out = Vec::new();
    for root in 0..n {
        if idx[root] != usize::MAX {
            continue;
        }
        let mut work = vec![(root, 0usize)];
        while let Some(&mut (v, ref mut i)) = work.last_mut() {
            if *i == 0 {
                idx[v] = next;
                low[v] = next;
                next += 1;
                stack.push(v);
                on[v] = true;
            }
            if let Some(&w) = adj[v].get(*i) {
                *i += 1;
                if idx[w] == usize::MAX {
                    work.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(idx[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(p, _)) = work.last() {
                low[p] = low[p].min(low[v]);
            }
            if low[v] == idx[v] {
                let mut comp = Vec::new();
                loop {
                    let w = stack.pop().expect("component root is on the stack");
                    on[w] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(comp);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(mut c: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
        for x in &mut c {
            x.sort();
        }
        c.sort();
        c
    }

    #[test]
    fn scc_of_three_modules() {
        // 0 -> 1 -> 2 -> 0, and 3 -> 0 hangs off the ring.
        let adj = vec![vec![1], vec![2], vec![0], vec![0]];
        assert_eq!(norm(scc(&adj)), vec![vec![0, 1, 2], vec![3]]);
    }

    fn unit(id: &str, kind: UnitKind, module: Option<&str>) -> crate::structure::UnitDraft {
        use crate::structure::{Pos, Span, UnitDraft};
        let p = Pos { line: 1, col: 0 };
        UnitDraft {
            unit: crate::model::Unit {
                id: UnitId(id.into()),
                kind,
                krate: "c".into(),
                module: module.map(|m| UnitId(m.into())),
                parent: None,
                file: "f.rs".into(),
                start_line: 1,
                end_line: 1,
                lines: 1,
                own_lines: 1,
                test: false,
                features: Default::default(),
                hash: String::new(),
            },
            ident: None,
            span: Span { start: p, end: p },
            body: None,
        }
    }

    fn graph_of(edges: &[(&str, &str)]) -> Graph {
        use crate::model::{Edge, Position};
        Graph {
            edges: edges
                .iter()
                .map(|(f, t)| Edge {
                    from: UnitId((*f).into()),
                    to: UnitId((*t).into()),
                    position: Position::Body,
                    count: 1,
                    first: Loc {
                        file: "f.rs".into(),
                        line: 1,
                    },
                })
                .collect(),
            ..Default::default()
        }
    }

    fn structure() -> Structure {
        let m = UnitKind::Module;
        let f = UnitKind::Function;
        Structure {
            units: vec![
                unit("c", m, None),
                unit("c::a", m, Some("c")),
                unit("c::b", m, Some("c")),
                unit("c::a::x", m, Some("c::a")),
                unit("c::a::y", m, Some("c::a")),
                unit("c::a::x::f", f, Some("c::a::x")),
                unit("c::a::y::g", f, Some("c::a::y")),
                unit("c::b::h", f, Some("c::b")),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn top_module_collapse_merges_children() {
        // a::x -> b and b -> a::y: a cycle only once a::x and a::y are one module.
        let g = graph_of(&[("c::a::x::f", "c::b::h"), ("c::b::h", "c::a::y::g")]);
        let c = compute(&structure(), &g);
        assert!(c.by_module.is_empty());
        assert_eq!(c.by_top_module.len(), 1);
        assert_eq!(
            c.by_top_module[0].modules,
            vec![UnitId("c::a".into()), UnitId("c::b".into())]
        );
        assert_eq!(c.by_top_module[0].krate, "c");
    }

    #[test]
    fn module_edges_are_ignored() {
        // Path segments (`crate::b`) target a module; they must not close a cycle.
        let g = graph_of(&[
            ("c::a::x::f", "c::b"),
            ("c::b::h", "c::a"),
            ("c::b::h", "c::a::x::f"),
        ]);
        let c = compute(&structure(), &g);
        assert!(c.by_module.is_empty() && c.by_top_module.is_empty());
    }

    #[test]
    fn module_target_inside_a_module_does_not_close_a_cycle() {
        // b -> module `c::a::y` (enclosed by `c::a`) must not count as b -> a.
        let g = graph_of(&[("c::a::x::f", "c::b::h"), ("c::b::h", "c::a::y")]);
        let c = compute(&structure(), &g);
        assert!(c.by_module.is_empty() && c.by_top_module.is_empty());
    }

    #[test]
    fn counts_and_first_accumulate() {
        let mut g = graph_of(&[
            ("c::a::x::f", "c::b::h"),
            ("c::a::y::g", "c::b::h"),
            ("c::b::h", "c::a::x::f"),
        ]);
        g.edges[1].count = 4;
        g.edges[1].first.line = 0;
        let c = compute(&structure(), &g);
        let top = &c.by_top_module[0];
        let ab = top.edges.iter().find(|e| e.from.0 == "c::a").unwrap();
        assert_eq!((ab.count, ab.first.line), (5, 0));
        // At full depth a::x and a::y stay apart.
        let full = &c.by_module[0];
        assert_eq!(full.modules.len(), 2);
        assert_eq!(full.edges.len(), 2);
    }

    #[test]
    fn root_items_stay_in_the_root() {
        assert_eq!(collapse(&UnitId("c".into()), "c"), UnitId("c".into()));
        assert_eq!(
            collapse(&UnitId("c::a::x".into()), "c"),
            UnitId("c::a".into())
        );
        assert_eq!(collapse(&UnitId("c::a".into()), "c"), UnitId("c::a".into()));
    }

    fn ring_graph() -> Graph {
        graph_of(&[("c::a::x::f", "c::b::h"), ("c::b::h", "c::a::x::f")])
    }

    fn with(mut s: Structure, id: &str, f: impl Fn(&mut crate::model::Unit)) -> Structure {
        for d in &mut s.units {
            if d.unit.id.0 == id {
                f(&mut d.unit);
            }
        }
        s
    }

    #[test]
    fn test_and_cross_crate_units_are_left_out() {
        assert_eq!(compute(&structure(), &ring_graph()).by_module.len(), 1);
        for id in ["c::a::x::f", "c::b::h"] {
            let t = with(structure(), id, |u| u.test = true);
            assert!(compute(&t, &ring_graph()).by_module.is_empty(), "test {id}");
            let x = with(structure(), id, |u| u.krate = "other".into());
            assert!(
                compute(&x, &ring_graph()).by_module.is_empty(),
                "crate {id}"
            );
        }
    }
}
