//! The digest: a small, line-number-free projection of a topology, stable under edits
//! that only move code around, for comparing two runs.

use crate::model::{Generator, State, StateLines, Topology, UnitId, UnitKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DigestUnit {
    pub kind: UnitKind,
    pub hash: String,
    pub lines: u32,
    pub test: bool,
    /// group -> state, for the groups that have taint for this unit.
    pub states: BTreeMap<String, State>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub generator: Generator,
    pub units: BTreeMap<UnitId, DigestUnit>,
    /// `"<from> -> <to> [signature|body]"`, and `"<from> -> ext:<package> [..]"` for externals.
    pub edges: BTreeMap<String, u32>,
    pub module_lines: BTreeMap<String, BTreeMap<UnitId, StateLines>>,
    pub cycles_by_module: Vec<Vec<UnitId>>,
    pub cycles_by_top_module: Vec<Vec<UnitId>>,
    /// struct -> field -> modules that use it.
    pub wide_structs: BTreeMap<UnitId, BTreeMap<String, BTreeSet<UnitId>>>,
    /// orphan file -> lines
    pub orphans: BTreeMap<String, u32>,
    /// warning kind -> count
    pub warnings: BTreeMap<String, u32>,
}

/// The serde name of a plain enum value, e.g. `Position::Body` -> `body`.
pub fn name_of<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v)
        .and_then(|s| serde_json::from_str::<String>(&s))
        .unwrap_or_default()
}

impl Digest {
    pub fn from_topology(t: &Topology) -> Digest {
        let units = t
            .units
            .iter()
            .map(|u| {
                let states = t
                    .taint
                    .iter()
                    .filter_map(|(g, m)| m.get(&u.id).map(|x| (g.clone(), x.state)))
                    .collect();
                (
                    u.id.clone(),
                    DigestUnit {
                        kind: u.kind,
                        hash: u.hash.clone(),
                        lines: u.lines,
                        test: u.test,
                        states,
                    },
                )
            })
            .collect();
        let mut edges: BTreeMap<String, u32> = BTreeMap::new();
        for e in &t.edges {
            let key = format!("{} -> {} [{}]", e.from.0, e.to.0, name_of(&e.position));
            *edges.entry(key).or_default() += e.count;
        }
        for e in &t.external {
            let key = format!(
                "{} -> ext:{} [{}]",
                e.from.0,
                e.package,
                name_of(&e.position)
            );
            *edges.entry(key).or_default() += e.count;
        }
        let modules = |list: &[crate::model::Cycle]| {
            let mut v: Vec<Vec<UnitId>> = list.iter().map(|c| c.modules.clone()).collect();
            v.sort();
            v
        };
        let wide_structs = t
            .wide_structs
            .iter()
            .map(|w| {
                let fields = w
                    .uses
                    .iter()
                    .map(|(f, by_module)| (f.clone(), by_module.keys().cloned().collect()))
                    .collect();
                (w.id.clone(), fields)
            })
            .collect();
        let mut orphans = BTreeMap::new();
        for o in &t.orphans {
            *orphans.entry(o.file.clone()).or_default() += o.lines;
        }
        let mut warnings = BTreeMap::new();
        for w in &t.warnings {
            *warnings.entry(name_of(&w.kind)).or_default() += 1;
        }
        Digest {
            generator: t.generator.clone(),
            units,
            edges,
            module_lines: t.module_lines.clone(),
            cycles_by_module: modules(&t.cycles.by_module),
            cycles_by_top_module: modules(&t.cycles.by_top_module),
            wide_structs,
            orphans,
            warnings,
        }
    }
}
