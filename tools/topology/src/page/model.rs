//! Everything the review page shows, computed here. The page's JS only renders.

use crate::config::Config;
use crate::model::{
    CrateInfo, Cycles, Generator, Loc, Orphan, Position, State, StateLines, TargetKind, Topology,
    UnitId, UnitKind, Warning, WarningKind,
};
use crate::taint;
use crate::timings::Timings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Warnings kept per kind; the rest are only counted.
const WARNINGS_SHOWN: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageModel {
    pub name: String,
    /// What produced the map: the tool and rust-analyzer versions.
    pub generator: Generator,
    /// Every file named by a unit, an edge or evidence, sorted; a `PageLoc` indexes it.
    pub files: Vec<String>,
    pub groups: Vec<String>,
    pub headline_group: String,
    pub near_clean_max_cuts: u32,
    pub overview: Overview,
    /// Every unit, modules included. Other sections refer to a unit by its index here.
    pub units: Vec<PageUnit>,
    /// Unit-to-unit edges; units list theirs by index.
    pub edges: Vec<PageEdge>,
    pub modules: Vec<GroupModules>,
    pub seams: Vec<GroupSeams>,
    pub tests: Vec<GroupTests>,
    pub cycles: Cycles,
    pub wide_structs: Vec<WideRow>,
    pub orphans: Vec<Orphan>,
    pub warnings: Vec<WarningGroup>,
}

/// A source location as `[index into PageModel::files, line]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageLoc(pub u32, pub u32);

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A unit's taint for one group. A clean unit serialises as its state alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageTaint {
    pub state: State,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cuts: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct: Vec<PageEvidence>,
    /// Indices into `PageModel::units`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageEvidence {
    pub package: String,
    pub symbol: String,
    pub position: Position,
    pub count: u32,
    pub first: PageLoc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Overview {
    pub total_lines: u32,
    pub crates: Vec<CrateRow>,
    /// The headline group's lines per state, over all crates.
    pub headline: StateLines,
    pub timings: Option<Timings>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrateRow {
    pub id: String,
    pub package: String,
    pub kind: TargetKind,
    pub test_only: bool,
    pub lines: u32,
    /// Headline-group lines per state; `None` for crates of the group itself.
    pub split: Option<StateLines>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageUnit {
    pub id: UnitId,
    pub kind: UnitKind,
    #[serde(rename = "crate")]
    pub krate: String,
    pub loc: PageLoc,
    pub lines: u32,
    pub test: bool,
    /// The module this unit is in, as an index into `PageModel::units`.
    pub module: Option<u32>,
    /// One entry per group, in `PageModel::groups` order; `None` where the unit has no taint.
    pub taint: Vec<Option<PageTaint>>,
    /// Indices into `PageModel::edges`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageEdge {
    /// Indices into `PageModel::units`.
    pub from: u32,
    pub to: u32,
    pub position: Position,
    pub count: u32,
    pub first: PageLoc,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateCounts {
    pub clean: u32,
    pub signature: u32,
    pub body: u32,
    pub inherited: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleRow {
    pub module: UnitId,
    pub lines: StateLines,
    pub units: StateCounts,
    /// Groups the module's units reach, with the number of references.
    pub groups_reached: BTreeMap<String, u32>,
    pub imports: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupModules {
    pub group: String,
    pub rows: Vec<ModuleRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeamRow {
    /// Index into `PageModel::units`.
    pub index: u32,
    pub state: State,
    pub cuts: u32,
    pub lines: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupSeams {
    pub group: String,
    pub seams: Vec<SeamRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestRow {
    /// Index into `PageModel::units`.
    pub unit: u32,
    pub state: State,
    pub cuts: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupTests {
    pub group: String,
    pub units: Vec<TestRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WideRow {
    pub id: UnitId,
    pub loc: Option<Loc>,
    pub fields: Vec<String>,
    /// field -> module -> number of units of that module using it.
    pub uses: BTreeMap<String, BTreeMap<UnitId, u32>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarningGroup {
    pub kind: WarningKind,
    pub count: u32,
    pub first: Vec<Warning>,
}

fn add_lines(into: &mut StateLines, l: &StateLines) {
    into.clean += l.clean;
    into.signature += l.signature;
    into.body += l.body;
    into.inherited += l.inherited;
}

/// The sorted, de-duplicated files that locations in the page refer to.
fn interned_files(t: &Topology) -> Vec<String> {
    let mut files: BTreeSet<&str> = BTreeSet::new();
    files.extend(t.units.iter().map(|u| u.file.as_str()));
    files.extend(t.edges.iter().map(|e| e.first.file.as_str()));
    for m in t.taint.values() {
        for x in m.values() {
            files.extend(x.direct.iter().map(|e| e.first.file.as_str()));
        }
    }
    files.into_iter().map(str::to_string).collect()
}

pub fn build(t: &Topology, timings: Option<&Timings>, cfg: &Config) -> PageModel {
    let files = interned_files(t);
    let file_index: BTreeMap<&str, u32> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (f.as_str(), i as u32))
        .collect();
    let page_loc = |l: &Loc| {
        PageLoc(
            file_index.get(l.file.as_str()).copied().unwrap_or(0),
            l.line,
        )
    };
    let index: BTreeMap<&UnitId, u32> = t
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (&u.id, i as u32))
        .collect();

    let mut edges = Vec::new();
    let mut uses: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut used_by: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for e in &t.edges {
        let (Some(&from), Some(&to)) = (index.get(&e.from), index.get(&e.to)) else {
            continue;
        };
        let n = edges.len() as u32;
        edges.push(PageEdge {
            from,
            to,
            position: e.position,
            count: e.count,
            first: page_loc(&e.first),
        });
        uses.entry(from).or_default().push(n);
        used_by.entry(to).or_default().push(n);
    }

    let units: Vec<PageUnit> = t
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| PageUnit {
            id: u.id.clone(),
            kind: u.kind,
            krate: u.krate.clone(),
            loc: page_loc(&Loc {
                file: u.file.clone(),
                line: u.start_line,
            }),
            lines: u.lines,
            test: u.test,
            module: u.module.as_ref().and_then(|m| index.get(m).copied()),
            taint: t
                .groups
                .iter()
                .map(|g| {
                    let x = t.taint.get(g)?.get(&u.id)?;
                    Some(PageTaint {
                        state: x.state,
                        cuts: x.cuts,
                        direct: x
                            .direct
                            .iter()
                            .map(|e| PageEvidence {
                                package: e.package.clone(),
                                symbol: e.symbol.clone(),
                                position: e.position,
                                count: e.count,
                                first: page_loc(&e.first),
                            })
                            .collect(),
                        via: x.via.iter().filter_map(|v| index.get(v).copied()).collect(),
                    })
                })
                .collect(),
            uses: uses.remove(&(i as u32)).unwrap_or_default(),
            used_by: used_by.remove(&(i as u32)).unwrap_or_default(),
        })
        .collect();

    let loc_of = |id: &UnitId| -> Loc {
        index.get(id).map_or_else(
            || Loc {
                file: String::new(),
                line: 0,
            },
            |&i| {
                let u = &t.units[i as usize];
                Loc {
                    file: u.file.clone(),
                    line: u.start_line,
                }
            },
        )
    };

    PageModel {
        name: t.name.clone(),
        generator: t.generator.clone(),
        files,
        groups: t.groups.clone(),
        headline_group: cfg.headline_group.clone(),
        near_clean_max_cuts: cfg.near_clean_max_cuts,
        overview: overview(t, timings, cfg),
        modules: modules(t),
        seams: t
            .groups
            .iter()
            .map(|g| {
                let empty = BTreeMap::new();
                let rows = taint::seams(
                    t.taint.get(g).unwrap_or(&empty),
                    &t.units,
                    cfg.near_clean_max_cuts,
                )
                .into_iter()
                .map(|s| SeamRow {
                    index: index.get(&s.unit).copied().unwrap_or(0),
                    state: s.state,
                    cuts: s.cuts,
                    lines: s.lines,
                })
                .collect();
                GroupSeams {
                    group: g.clone(),
                    seams: rows,
                }
            })
            .collect(),
        tests: t
            .groups
            .iter()
            .map(|g| GroupTests {
                group: g.clone(),
                units: t
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| u.test)
                    .filter_map(|(i, u)| {
                        let x = t.taint.get(g)?.get(&u.id)?;
                        (x.state != State::Clean).then_some(TestRow {
                            unit: i as u32,
                            state: x.state,
                            cuts: x.cuts,
                        })
                    })
                    .collect(),
            })
            .collect(),
        cycles: t.cycles.clone(),
        wide_structs: t
            .wide_structs
            .iter()
            .map(|w| WideRow {
                id: w.id.clone(),
                loc: index.get(&w.id).map(|_| loc_of(&w.id)),
                fields: w.fields.clone(),
                uses: w
                    .uses
                    .iter()
                    .map(|(f, mods)| {
                        (
                            f.clone(),
                            mods.iter()
                                .map(|(m, us)| (m.clone(), us.len() as u32))
                                .collect(),
                        )
                    })
                    .collect(),
            })
            .collect(),
        orphans: t.orphans.clone(),
        warnings: warnings(&t.warnings),
        units,
        edges,
    }
}

fn overview(t: &Topology, timings: Option<&Timings>, cfg: &Config) -> Overview {
    let module_crate: BTreeMap<&UnitId, &str> = t
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Module)
        .map(|u| (&u.id, u.krate.as_str()))
        .collect();
    let mut per_crate: BTreeMap<&str, StateLines> = BTreeMap::new();
    let mut headline = StateLines::default();
    if let Some(lines) = t.module_lines.get(&cfg.headline_group) {
        for (m, l) in lines {
            if let Some(c) = module_crate.get(m) {
                add_lines(per_crate.entry(c).or_default(), l);
            }
            add_lines(&mut headline, l);
        }
    }
    Overview {
        total_lines: t.crates.iter().map(|c| c.lines).sum(),
        crates: t
            .crates
            .iter()
            .map(|c: &CrateInfo| CrateRow {
                id: c.id.clone(),
                package: c.package.clone(),
                kind: c.kind,
                test_only: c.test_only,
                lines: c.lines,
                split: per_crate.get(c.id.as_str()).cloned(),
            })
            .collect(),
        headline,
        timings: timings.cloned(),
    }
}

fn modules(t: &Topology) -> Vec<GroupModules> {
    let mut counts: BTreeMap<(&str, &UnitId), StateCounts> = BTreeMap::new();
    for g in &t.groups {
        let Some(taints) = t.taint.get(g) else {
            continue;
        };
        // Test units are left out, as they are from the lines.
        for u in t.units.iter().filter(|u| !u.test) {
            let (Some(m), Some(x)) = (&u.module, taints.get(&u.id)) else {
                continue;
            };
            let c = counts.entry((g.as_str(), m)).or_default();
            match x.state {
                State::Clean => c.clean += 1,
                State::Signature => c.signature += 1,
                State::Body => c.body += 1,
                State::Inherited => c.inherited += 1,
            }
        }
    }
    t.groups
        .iter()
        .map(|g| GroupModules {
            group: g.clone(),
            rows: t
                .module_lines
                .get(g)
                .into_iter()
                .flatten()
                .map(|(m, lines)| ModuleRow {
                    module: m.clone(),
                    lines: lines.clone(),
                    units: counts.remove(&(g.as_str(), m)).unwrap_or_default(),
                    groups_reached: t.module_groups.get(m).cloned().unwrap_or_default(),
                    imports: t.imports.get(m).cloned().unwrap_or_default(),
                })
                .collect(),
        })
        .collect()
}

fn warnings(all: &[Warning]) -> Vec<WarningGroup> {
    let mut by_kind: BTreeMap<WarningKind, WarningGroup> = BTreeMap::new();
    for w in all {
        let g = by_kind.entry(w.kind).or_insert_with(|| WarningGroup {
            kind: w.kind,
            count: 0,
            first: Vec::new(),
        });
        g.count += 1;
        if g.first.len() < WARNINGS_SHOWN {
            g.first.push(w.clone());
        }
    }
    by_kind.into_values().collect()
}
