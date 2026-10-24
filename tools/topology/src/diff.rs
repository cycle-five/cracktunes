//! Compare two maps (topology.json or digest.json) and recognise moved functions.

use crate::digest::{Digest, DigestUnit};
use crate::model::{State, StateLines, Topology, UnitId, UnitKind};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Move {
    pub from: UnitId,
    pub to: UnitId,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transition {
    pub unit: UnitId,
    pub before: Option<State>,
    pub after: Option<State>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleLinesChange {
    pub module: UnitId,
    pub before: StateLines,
    pub after: StateLines,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrateLinesChange {
    #[serde(rename = "crate")]
    pub krate: String,
    pub before: StateLines,
    pub after: StateLines,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct DiffReport {
    /// The group `render` lists first; set by the caller (the digest does not know it).
    pub headline: Option<String>,
    pub notes: Vec<String>,
    pub added: Vec<UnitId>,
    pub removed: Vec<UnitId>,
    pub changed: Vec<UnitId>,
    pub moved: Vec<Move>,
    pub transitions: BTreeMap<String, Vec<Transition>>,
    pub module_lines: BTreeMap<String, Vec<ModuleLinesChange>>,
    /// group -> crates whose lines per state changed (sums of the module lines).
    pub crate_lines: BTreeMap<String, Vec<CrateLinesChange>>,
    pub cycles_added: Vec<Vec<UnitId>>,
    pub cycles_removed: Vec<Vec<UnitId>>,
    /// Modules that are in a cycle now and were not.
    pub cycle_joined: Vec<UnitId>,
    /// Modules that were in a cycle and are not now.
    pub cycle_left: Vec<UnitId>,
    pub wide_changes: Vec<String>,
    pub orphans_added: Vec<String>,
    pub orphans_removed: Vec<String>,
    pub warnings: (BTreeMap<String, u32>, BTreeMap<String, u32>),
}

/// Read a `topology.json` or a `digest.json`.
pub fn load(path: &Path) -> Result<Digest> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let topology_err = match serde_json::from_str::<Topology>(&text) {
        Ok(t) => return Ok(Digest::from_topology(&t)),
        Err(e) => e,
    };
    match serde_json::from_str::<Digest>(&text) {
        Ok(d) => Ok(d),
        Err(digest_err) => bail!(
            "{} is neither a topology.json ({topology_err}) nor a digest.json ({digest_err})",
            path.display()
        ),
    }
}

/// An id without its positional `#N` suffix, and that N (1 when there is none).
fn split_ordinal(id: &UnitId) -> (&str, u32) {
    match id.0.rsplit_once('#') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => {
            (head, n.parse().unwrap_or(u32::MAX))
        },
        _ => (id.0.as_str(), 1),
    }
}

/// Which ids of the two sides are the same unit.
struct Pairing {
    /// (before id, after id), identical or changed.
    matched: Vec<(UnitId, UnitId)>,
    removed: BTreeSet<UnitId>,
    added: BTreeSet<UnitId>,
}

/// Pair ids across the two sides. Positional `#N` ids (`<impl From for E>#2`) are
/// grouped by their base id and paired by equal hash first, so taking one block out
/// does not renumber its neighbours into a cascade of changes. What is left pairs in
/// order of N.
fn pair_units(a: &Digest, b: &Digest) -> Pairing {
    type Members<'x> = Vec<(u32, &'x UnitId)>;
    let mut groups: BTreeMap<&str, (Members, Members)> = BTreeMap::new();
    for id in a.units.keys() {
        let (base, n) = split_ordinal(id);
        groups.entry(base).or_default().0.push((n, id));
    }
    for id in b.units.keys() {
        let (base, n) = split_ordinal(id);
        groups.entry(base).or_default().1.push((n, id));
    }
    let mut out = Pairing {
        matched: Vec::new(),
        removed: BTreeSet::new(),
        added: BTreeSet::new(),
    };
    for (mut before, mut after) in groups.into_values() {
        before.sort();
        after.sort();
        // Equal hashes first.
        let mut left_b: Members = Vec::new();
        for (n, id) in before {
            let hash = &a.units[id].hash;
            match after.iter().position(|(_, x)| b.units[*x].hash == *hash) {
                Some(i) => out.matched.push((id.clone(), after.remove(i).1.clone())),
                None => left_b.push((n, id)),
            }
        }
        // The rest in order of N.
        let mut rest = after.into_iter();
        for (_, id) in left_b {
            match rest.next() {
                Some((_, x)) => out.matched.push((id.clone(), x.clone())),
                None => {
                    out.removed.insert(id.clone());
                },
            }
        }
        out.added.extend(rest.map(|(_, x)| x.clone()));
    }
    out
}

/// The last path segment of a unit id (the last two for methods).
fn match_name(id: &UnitId, kind: UnitKind) -> String {
    let keep = if kind == UnitKind::Method { 2 } else { 1 };
    let base = match id.0.rsplit_once('#') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => head,
        _ => id.0.as_str(),
    };
    let parts: Vec<&str> = base.split("::").collect();
    parts[parts.len().saturating_sub(keep)..].join("::")
}

type Key = (UnitKind, String);
type Sides<'a> = (Vec<&'a UnitId>, Vec<&'a UnitId>);

fn group<'a>(
    ids: &[&'a UnitId],
    units: &BTreeMap<UnitId, DigestUnit>,
) -> BTreeMap<Key, Vec<&'a UnitId>> {
    let mut m: BTreeMap<Key, Vec<&UnitId>> = BTreeMap::new();
    for id in ids {
        let kind = units[*id].kind;
        m.entry((kind, match_name(id, kind))).or_default().push(id);
    }
    m
}

fn find_moves(
    a: &Digest,
    b: &Digest,
    removed: &mut BTreeSet<UnitId>,
    added: &mut BTreeSet<UnitId>,
) -> Vec<Move> {
    let mut moves = Vec::new();
    // Pass 1: same kind, name and hash, unique on both sides.
    let mut by_hash: BTreeMap<(UnitKind, String, &str), Sides> = BTreeMap::new();
    for id in removed.iter() {
        let u = &a.units[id];
        by_hash
            .entry((u.kind, match_name(id, u.kind), u.hash.as_str()))
            .or_default()
            .0
            .push(id);
    }
    for id in added.iter() {
        let u = &b.units[id];
        by_hash
            .entry((u.kind, match_name(id, u.kind), u.hash.as_str()))
            .or_default()
            .1
            .push(id);
    }
    let mut pairs: Vec<(UnitId, UnitId, bool)> = Vec::new();
    for (from, to) in by_hash.values() {
        if let ([f], [t]) = (from.as_slice(), to.as_slice()) {
            pairs.push(((*f).clone(), (*t).clone(), false));
        }
    }
    for (f, t, _) in &pairs {
        removed.remove(f);
        added.remove(t);
    }
    // Pass 2: same kind and name, different hash, unique on both sides.
    let rem: Vec<&UnitId> = removed.iter().collect();
    let add: Vec<&UnitId> = added.iter().collect();
    let rg = group(&rem, &a.units);
    let ag = group(&add, &b.units);
    let mut pairs2: Vec<(UnitId, UnitId, bool)> = Vec::new();
    for (k, from) in &rg {
        if let Some(to) = ag.get(k)
            && let ([f], [t]) = (from.as_slice(), to.as_slice())
        {
            pairs2.push(((*f).clone(), (*t).clone(), true));
        }
    }
    for (f, t, _) in &pairs2 {
        removed.remove(f);
        added.remove(t);
    }
    for (from, to, changed) in pairs.into_iter().chain(pairs2) {
        moves.push(Move { from, to, changed });
    }
    moves.sort_by(|x, y| (&x.from, &x.to).cmp(&(&y.from, &y.to)));
    moves
}

fn generator_notes(a: &Digest, b: &Digest) -> Vec<String> {
    let (ga, gb) = (&a.generator, &b.generator);
    let mut notes = Vec::new();
    if ga.rust_analyzer != gb.rust_analyzer {
        notes.push(format!(
            "rust-analyzer differs ({} vs {}): differences may come from the indexer, not the code",
            ga.rust_analyzer, gb.rust_analyzer
        ));
    }
    if ga.tool_hash != gb.tool_hash {
        notes.push(format!(
            "tool version differs ({} vs {}): differences may come from the tool",
            ga.tool_hash, gb.tool_hash
        ));
    }
    if ga.config_hash != gb.config_hash {
        notes.push(format!(
            "config differs ({} vs {}): groups or settings changed",
            ga.config_hash, gb.config_hash
        ));
    }
    notes
}

fn cycle_set(d: &Digest) -> BTreeSet<&Vec<UnitId>> {
    d.cycles_by_module
        .iter()
        .chain(&d.cycles_by_top_module)
        .collect()
}

fn cycle_members(d: &Digest) -> BTreeSet<&UnitId> {
    cycle_set(d).into_iter().flatten().collect()
}

/// A module id's crate: its first path segment.
fn crate_of(module: &UnitId) -> &str {
    module.0.split("::").next().unwrap_or(&module.0)
}

fn crate_totals(modules: &BTreeMap<UnitId, StateLines>) -> BTreeMap<&str, StateLines> {
    let mut m: BTreeMap<&str, StateLines> = BTreeMap::new();
    for (id, l) in modules {
        let t = m.entry(crate_of(id)).or_default();
        t.clean += l.clean;
        t.signature += l.signature;
        t.body += l.body;
        t.inherited += l.inherited;
    }
    m
}

fn wide_changes(a: &Digest, b: &Digest) -> Vec<String> {
    let empty = BTreeMap::new();
    let none = BTreeSet::new();
    let ids: BTreeSet<&UnitId> = a.wide_structs.keys().chain(b.wide_structs.keys()).collect();
    let mut out = Vec::new();
    for id in ids {
        let fa = a.wide_structs.get(id).unwrap_or(&empty);
        let fb = b.wide_structs.get(id).unwrap_or(&empty);
        let fields: BTreeSet<&String> = fa.keys().chain(fb.keys()).collect();
        for f in fields {
            let ma = fa.get(f).unwrap_or(&none);
            let mb = fb.get(f).unwrap_or(&none);
            let plus: Vec<&str> = mb.difference(ma).map(|m| m.0.as_str()).collect();
            let minus: Vec<&str> = ma.difference(mb).map(|m| m.0.as_str()).collect();
            if plus.is_empty() && minus.is_empty() {
                continue;
            }
            let mut s = format!("{}.{}:", id.0, f);
            if !plus.is_empty() {
                let _ = write!(s, " +{}", plus.join(", +"));
            }
            if !minus.is_empty() {
                let _ = write!(s, " -{}", minus.join(", -"));
            }
            out.push(s);
        }
    }
    out
}

pub fn compare(a: &Digest, b: &Digest) -> DiffReport {
    let Pairing {
        matched,
        mut removed,
        mut added,
    } = pair_units(a, b);
    let mut changed: Vec<UnitId> = matched
        .iter()
        .filter(|(f, t)| a.units[f].hash != b.units[t].hash)
        .map(|(_, t)| t.clone())
        .collect();
    changed.sort();
    let moved = find_moves(a, b, &mut removed, &mut added);

    // Unit pairs whose state is compared: paired ids, or a recognised move.
    let mut pairs: Vec<(&UnitId, &UnitId)> = matched.iter().map(|(f, t)| (f, t)).collect();
    pairs.extend(moved.iter().map(|m| (&m.from, &m.to)));
    let mut transitions: BTreeMap<String, Vec<Transition>> = BTreeMap::new();
    for (from, to) in pairs {
        let (sa, sb) = (&a.units[from].states, &b.units[to].states);
        let groups: BTreeSet<&String> = sa.keys().chain(sb.keys()).collect();
        for g in groups {
            let (before, after) = (sa.get(g).copied(), sb.get(g).copied());
            if before != after {
                transitions.entry(g.clone()).or_default().push(Transition {
                    unit: to.clone(),
                    before,
                    after,
                });
            }
        }
    }
    for id in &added {
        for (g, st) in &b.units[id].states {
            transitions.entry(g.clone()).or_default().push(Transition {
                unit: id.clone(),
                before: None,
                after: Some(*st),
            });
        }
    }
    for id in &removed {
        for (g, st) in &a.units[id].states {
            transitions.entry(g.clone()).or_default().push(Transition {
                unit: id.clone(),
                before: Some(*st),
                after: None,
            });
        }
    }
    for v in transitions.values_mut() {
        v.sort_by(|x, y| x.unit.cmp(&y.unit));
    }

    let zero = StateLines::default();
    let mut module_lines: BTreeMap<String, Vec<ModuleLinesChange>> = BTreeMap::new();
    let line_groups: BTreeSet<&String> =
        a.module_lines.keys().chain(b.module_lines.keys()).collect();
    let no_modules = BTreeMap::new();
    for g in line_groups {
        let ma = a.module_lines.get(g).unwrap_or(&no_modules);
        let mb = b.module_lines.get(g).unwrap_or(&no_modules);
        let modules: BTreeSet<&UnitId> = ma.keys().chain(mb.keys()).collect();
        for m in modules {
            let before = ma.get(m).unwrap_or(&zero);
            let after = mb.get(m).unwrap_or(&zero);
            if before != after {
                module_lines
                    .entry(g.clone())
                    .or_default()
                    .push(ModuleLinesChange {
                        module: m.clone(),
                        before: before.clone(),
                        after: after.clone(),
                    });
            }
        }
    }

    let mut crate_lines: BTreeMap<String, Vec<CrateLinesChange>> = BTreeMap::new();
    let line_groups: BTreeSet<&String> =
        a.module_lines.keys().chain(b.module_lines.keys()).collect();
    for g in line_groups {
        let ta = crate_totals(a.module_lines.get(g).unwrap_or(&no_modules));
        let tb = crate_totals(b.module_lines.get(g).unwrap_or(&no_modules));
        let crates: BTreeSet<&&str> = ta.keys().chain(tb.keys()).collect();
        for c in crates {
            let before = ta.get(*c).cloned().unwrap_or_default();
            let after = tb.get(*c).cloned().unwrap_or_default();
            if before != after {
                crate_lines
                    .entry(g.clone())
                    .or_default()
                    .push(CrateLinesChange {
                        krate: (*c).to_string(),
                        before,
                        after,
                    });
            }
        }
    }

    let (ma, mb) = (cycle_members(a), cycle_members(b));
    let cycle_joined: Vec<UnitId> = mb.difference(&ma).map(|m| (*m).clone()).collect();
    let cycle_left: Vec<UnitId> = ma.difference(&mb).map(|m| (*m).clone()).collect();
    let (ca, cb) = (cycle_set(a), cycle_set(b));
    let cycles_added: Vec<Vec<UnitId>> = cb.difference(&ca).map(|c| (*c).clone()).collect();
    let cycles_removed: Vec<Vec<UnitId>> = ca.difference(&cb).map(|c| (*c).clone()).collect();

    let orphans_added = b
        .orphans
        .keys()
        .filter(|k| !a.orphans.contains_key(*k))
        .cloned()
        .collect();
    let orphans_removed = a
        .orphans
        .keys()
        .filter(|k| !b.orphans.contains_key(*k))
        .cloned()
        .collect();

    let kinds: BTreeSet<&String> = a.warnings.keys().chain(b.warnings.keys()).collect();
    let mut wa = BTreeMap::new();
    let mut wb = BTreeMap::new();
    for k in kinds {
        let (x, y) = (
            a.warnings.get(k).copied().unwrap_or(0),
            b.warnings.get(k).copied().unwrap_or(0),
        );
        if x != y {
            wa.insert(k.clone(), x);
            wb.insert(k.clone(), y);
        }
    }

    DiffReport {
        headline: None,
        notes: generator_notes(a, b),
        added: added.into_iter().collect(),
        removed: removed.into_iter().collect(),
        changed,
        moved,
        transitions,
        module_lines,
        crate_lines,
        cycles_added,
        cycles_removed,
        cycle_joined,
        cycle_left,
        wide_changes: wide_changes(a, b),
        orphans_added,
        orphans_removed,
        warnings: (wa, wb),
    }
}

fn state_name(s: Option<State>) -> String {
    s.map_or_else(|| "-".to_string(), |s| crate::digest::name_of(&s))
}

fn section(out: &mut String, title: &str, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    let _ = writeln!(out, "{title} ({})", lines.len());
    for l in lines {
        let _ = writeln!(out, "  {l}");
    }
}

fn lines_line(l: &StateLines, m: &StateLines) -> String {
    format!(
        "clean {}->{}, signature {}->{}, body {}->{}, inherited {}->{}",
        l.clean, m.clean, l.signature, m.signature, l.body, m.body, l.inherited, m.inherited
    )
}

/// The map's entries with the headline group first, the rest in name order.
fn headline_first<'a, V>(
    m: &'a BTreeMap<String, V>,
    headline: Option<&str>,
) -> Vec<(&'a String, &'a V)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by_key(|(g, _)| Some(g.as_str()) != headline);
    v
}

pub fn render(r: &DiffReport) -> String {
    let mut out = String::new();
    for n in &r.notes {
        let _ = writeln!(out, "NOTE: {n}");
    }
    section(
        &mut out,
        "moved",
        r.moved
            .iter()
            .map(|m| {
                format!(
                    "{} -> {}{}",
                    m.from.0,
                    m.to.0,
                    if m.changed { " (changed)" } else { "" }
                )
            })
            .collect(),
    );
    let ids = |v: &[UnitId]| v.iter().map(|u| u.0.clone()).collect::<Vec<_>>();
    section(&mut out, "added", ids(&r.added));
    section(&mut out, "removed", ids(&r.removed));
    section(&mut out, "changed", ids(&r.changed));
    for (g, cs) in headline_first(&r.crate_lines, r.headline.as_deref()) {
        section(
            &mut out,
            &format!("lines per state for {g}, per crate"),
            cs.iter()
                .map(|c| format!("{}: {}", c.krate, lines_line(&c.before, &c.after)))
                .collect(),
        );
    }
    for (g, ts) in headline_first(&r.transitions, r.headline.as_deref()) {
        section(
            &mut out,
            &format!("state changes for {g}"),
            ts.iter()
                .map(|t| {
                    let note = match (t.before, t.after) {
                        (None, _) => " (new unit)",
                        (_, None) => " (unit gone)",
                        _ => "",
                    };
                    format!(
                        "{}: {} -> {}{note}",
                        t.unit.0,
                        state_name(t.before),
                        state_name(t.after)
                    )
                })
                .collect(),
        );
    }
    for (g, ms) in headline_first(&r.module_lines, r.headline.as_deref()) {
        section(
            &mut out,
            &format!("module lines for {g}"),
            ms.iter()
                .map(|m| {
                    let note = if m.before == StateLines::default() {
                        " (new module)"
                    } else if m.after == StateLines::default() {
                        " (module gone)"
                    } else {
                        ""
                    };
                    format!("{}: {}", m.module.0, lines_line(&m.before, &m.after)) + note
                })
                .collect(),
        );
    }
    section(
        &mut out,
        "modules that joined a cycle",
        ids(&r.cycle_joined),
    );
    section(&mut out, "modules that left a cycle", ids(&r.cycle_left));
    section(&mut out, "wide struct changes", r.wide_changes.clone());
    section(&mut out, "orphans added", r.orphans_added.clone());
    section(&mut out, "orphans removed", r.orphans_removed.clone());
    section(
        &mut out,
        "warnings",
        r.warnings
            .0
            .iter()
            .map(|(k, x)| format!("{k}: {x} -> {}", r.warnings.1.get(k).copied().unwrap_or(0)))
            .collect(),
    );
    if out.lines().all(|l| l.starts_with("NOTE: ")) {
        out.push_str("no differences\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::DigestGenerator;
    fn generator() -> DigestGenerator {
        DigestGenerator {
            tool_hash: "t".into(),
            rust_analyzer: "ra 1".into(),
            config_hash: "c".into(),
        }
    }

    fn digest(units: &[(&str, UnitKind, &str)]) -> Digest {
        Digest {
            generator: generator(),
            units: units
                .iter()
                .map(|(id, kind, hash)| {
                    (
                        UnitId((*id).into()),
                        DigestUnit {
                            kind: *kind,
                            hash: (*hash).into(),
                            lines: 3,
                            test: false,
                            states: BTreeMap::new(),
                        },
                    )
                })
                .collect(),
            edges: BTreeMap::new(),
            module_lines: BTreeMap::new(),
            cycles_by_module: vec![],
            cycles_by_top_module: vec![],
            wide_structs: BTreeMap::new(),
            orphans: BTreeMap::new(),
            warnings: BTreeMap::new(),
        }
    }

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    use UnitKind::{Function, Method};

    #[test]
    fn identical_digests_empty_report() {
        let d = digest(&[("a::f", Function, "h")]);
        let r = compare(&d, &d.clone());
        assert_eq!(r, DiffReport::default());
        assert_eq!(render(&r), "no differences\n");
    }

    #[test]
    fn added_removed_changed() {
        let a = digest(&[("a::gone", Function, "1"), ("a::edit", Function, "2")]);
        let b = digest(&[("a::edit", Function, "3"), ("a::new", Method, "4")]);
        let r = compare(&a, &b);
        assert_eq!(r.removed, vec![id("a::gone")]);
        assert_eq!(r.added, vec![id("a::new")]);
        assert_eq!(r.changed, vec![id("a::edit")]);
        assert!(r.moved.is_empty());
        let text = render(&r);
        assert!(
            text.contains("added (1)") && text.contains("a::new"),
            "{text}"
        );
        assert!(
            text.contains("removed (1)") && text.contains("a::gone"),
            "{text}"
        );
        assert!(
            text.contains("changed (1)") && text.contains("a::edit"),
            "{text}"
        );
    }

    #[test]
    fn move_same_hash() {
        let a = digest(&[("a::x::f", Function, "h")]);
        let b = digest(&[("b::y::f", Function, "h")]);
        let r = compare(&a, &b);
        assert_eq!(
            r.moved,
            vec![Move {
                from: id("a::x::f"),
                to: id("b::y::f"),
                changed: false
            }]
        );
        assert!(r.added.is_empty() && r.removed.is_empty() && r.changed.is_empty());
        assert!(render(&r).contains("a::x::f -> b::y::f"));
    }

    #[test]
    fn move_changed_unique() {
        let a = digest(&[("a::f", Function, "1")]);
        let b = digest(&[("b::f", Function, "2")]);
        let r = compare(&a, &b);
        assert_eq!(
            r.moved,
            vec![Move {
                from: id("a::f"),
                to: id("b::f"),
                changed: true
            }]
        );
        assert!(render(&r).contains("(changed)"));
    }

    #[test]
    fn methods_match_on_last_two_segments() {
        let a = digest(&[("a::Bot::ping", Method, "h"), ("a::Cat::ping", Method, "h")]);
        let b = digest(&[("b::Bot::ping", Method, "h")]);
        let r = compare(&a, &b);
        assert_eq!(
            r.moved,
            vec![Move {
                from: id("a::Bot::ping"),
                to: id("b::Bot::ping"),
                changed: false
            }]
        );
        assert_eq!(r.removed, vec![id("a::Cat::ping")]);
    }

    #[test]
    fn kinds_must_match() {
        let a = digest(&[("a::f", Function, "h")]);
        let b = digest(&[("b::f", UnitKind::Struct, "h")]);
        let r = compare(&a, &b);
        assert!(r.moved.is_empty());
        assert_eq!((r.removed.len(), r.added.len()), (1, 1));
    }

    #[test]
    fn ambiguous_names_not_matched() {
        for hashes in [("1", "2", "3"), ("1", "1", "1")] {
            let a = digest(&[
                ("x::new", Function, hashes.0),
                ("z::new", Function, hashes.1),
            ]);
            let b = digest(&[("y::new", Function, hashes.2)]);
            let r = compare(&a, &b);
            assert!(r.moved.is_empty(), "{hashes:?}: {:?}", r.moved);
            assert_eq!(r.removed.len(), 2);
            assert_eq!(r.added, vec![id("y::new")]);
        }
    }

    #[test]
    fn unique_hash_match_beats_ambiguous_name() {
        let a = digest(&[("x::new", Function, "1"), ("z::new", Function, "2")]);
        let b = digest(&[("y::new", Function, "1")]);
        let r = compare(&a, &b);
        assert_eq!(
            r.moved,
            vec![Move {
                from: id("x::new"),
                to: id("y::new"),
                changed: false
            }]
        );
        assert_eq!(r.removed, vec![id("z::new")]);
    }

    #[test]
    fn transition_to_clean_reported() {
        let mut a = digest(&[("a::f", Function, "h")]);
        let mut b = a.clone();
        a.units
            .get_mut(&id("a::f"))
            .unwrap()
            .states
            .insert("g".into(), State::Signature);
        b.units
            .get_mut(&id("a::f"))
            .unwrap()
            .states
            .insert("g".into(), State::Clean);
        let r = compare(&a, &b);
        assert_eq!(
            r.transitions["g"],
            vec![Transition {
                unit: id("a::f"),
                before: Some(State::Signature),
                after: Some(State::Clean)
            }]
        );
        assert!(render(&r).contains("a::f: signature -> clean"));
    }

    #[test]
    fn moved_unit_transition_compares_old_to_new() {
        let mut a = digest(&[("a::f", Function, "h")]);
        let mut b = digest(&[("b::f", Function, "h")]);
        a.units
            .get_mut(&id("a::f"))
            .unwrap()
            .states
            .insert("g".into(), State::Body);
        b.units
            .get_mut(&id("b::f"))
            .unwrap()
            .states
            .insert("g".into(), State::Clean);
        let r = compare(&a, &b);
        assert_eq!(r.transitions["g"].len(), 1);
        assert_eq!(r.transitions["g"][0].unit, id("b::f"));
        assert_eq!(r.transitions["g"][0].before, Some(State::Body));
    }

    #[test]
    fn module_lines_cycles_orphans_warnings_wide() {
        let lines = |c| StateLines {
            clean: c,
            signature: 0,
            body: 0,
            inherited: 0,
        };
        let mut a = digest(&[]);
        let mut b = digest(&[]);
        a.module_lines.insert(
            "g".into(),
            BTreeMap::from([(id("m"), lines(1)), (id("n"), lines(5))]),
        );
        b.module_lines.insert(
            "g".into(),
            BTreeMap::from([(id("m"), lines(2)), (id("n"), lines(5))]),
        );
        a.cycles_by_module = vec![vec![id("p"), id("q")]];
        b.cycles_by_module = vec![vec![id("r"), id("s")]];
        a.cycles_by_top_module = vec![vec![id("p"), id("q")]];
        a.orphans.insert("old.rs".into(), 3);
        b.orphans.insert("new.rs".into(), 4);
        a.warnings.insert("k".into(), 1);
        b.warnings.insert("k".into(), 2);
        a.warnings.insert("same".into(), 1);
        b.warnings.insert("same".into(), 1);
        a.wide_structs.insert(
            id("S"),
            BTreeMap::from([("f".into(), BTreeSet::from([id("m1")]))]),
        );
        b.wide_structs.insert(
            id("S"),
            BTreeMap::from([("f".into(), BTreeSet::from([id("m2")]))]),
        );
        a.wide_structs.insert(id("T"), BTreeMap::new());
        b.wide_structs.insert(
            id("T"),
            BTreeMap::from([("g".into(), BTreeSet::from([id("m3")]))]),
        );
        let r = compare(&a, &b);
        assert_eq!(r.module_lines["g"].len(), 1);
        assert_eq!(r.module_lines["g"][0].module, id("m"));
        assert_eq!(r.module_lines["g"][0].after.clean, 2);
        assert_eq!(r.cycles_added, vec![vec![id("r"), id("s")]]);
        assert_eq!(r.cycles_removed, vec![vec![id("p"), id("q")]]);
        assert_eq!(r.orphans_added, vec!["new.rs".to_string()]);
        assert_eq!(r.orphans_removed, vec!["old.rs".to_string()]);
        assert_eq!(r.warnings.0, BTreeMap::from([("k".to_string(), 1)]));
        assert_eq!(r.warnings.1, BTreeMap::from([("k".to_string(), 2)]));
        assert_eq!(
            r.wide_changes,
            vec!["S.f: +m2 -m1".to_string(), "T.g: +m3".to_string()]
        );
        let text = render(&r);
        for want in [
            "module lines for g",
            "modules that joined a cycle",
            "modules that left a cycle",
            "orphans added",
            "orphans removed",
            "warnings",
            "k: 1 -> 2",
            "S.f: +m2 -m1",
            "T.g: +m3",
            "  r\n",
            "  s\n",
            "  p\n",
            "  q\n",
        ] {
            assert!(text.contains(want), "{want} missing in {text}");
        }
    }

    #[test]
    fn generator_mismatch_is_noted_first() {
        let a = digest(&[("a::f", Function, "1")]);
        let mut b = digest(&[("a::f", Function, "2")]);
        b.generator.rust_analyzer = "ra 2".into();
        let r = compare(&a, &b);
        assert!(r.notes[0].contains("rust-analyzer"), "{:?}", r.notes);
        assert!(render(&r).starts_with("NOTE: rust-analyzer"));
        let mut c = digest(&[]);
        c.generator.tool_hash = "t2".into();
        c.generator.config_hash = "c2".into();
        let notes = compare(&digest(&[]), &c).notes;
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("tool") && notes[1].contains("config"));
        assert_eq!(
            render(&compare(&digest(&[]), &c)).lines().last(),
            Some("no differences")
        );
    }

    #[test]
    fn load_missing_file_is_an_error() {
        let err = load(Path::new("/nonexistent/dir/x.json")).unwrap_err();
        assert!(format!("{err}").contains("x.json"), "{err}");
    }

    #[test]
    fn duplicate_suffix_is_ignored_when_matching() {
        let a = digest(&[("a::Bot::ping#2", Method, "h")]);
        let b = digest(&[("b::Bot::ping", Method, "h")]);
        let r = compare(&a, &b);
        assert_eq!(
            r.moved,
            vec![Move {
                from: id("a::Bot::ping#2"),
                to: id("b::Bot::ping"),
                changed: false
            }]
        );
        assert_eq!(match_name(&id("a::f#x"), Function), "f#x");
        assert_eq!(match_name(&id("a::f#"), Function), "f#");
    }

    #[test]
    fn added_and_removed_units_have_transitions() {
        let mut a = digest(&[("a::gone", Function, "1")]);
        let mut b = digest(&[("a::new", Function, "2")]);
        a.units
            .get_mut(&id("a::gone"))
            .unwrap()
            .states
            .insert("g".into(), State::Body);
        b.units
            .get_mut(&id("a::new"))
            .unwrap()
            .states
            .insert("g".into(), State::Clean);
        let r = compare(&a, &b);
        assert_eq!(
            r.transitions["g"],
            vec![
                Transition {
                    unit: id("a::gone"),
                    before: Some(State::Body),
                    after: None
                },
                Transition {
                    unit: id("a::new"),
                    before: None,
                    after: Some(State::Clean)
                },
            ]
        );
        let text = render(&r);
        assert!(text.contains("a::new: - -> clean (new unit)"), "{text}");
        assert!(text.contains("a::gone: body -> - (unit gone)"), "{text}");
    }

    #[test]
    fn one_sided_modules_get_zeroed_lines() {
        let lines = |c| StateLines {
            clean: c,
            signature: 0,
            body: 0,
            inherited: 0,
        };
        let mut a = digest(&[]);
        let mut b = digest(&[]);
        a.module_lines
            .insert("g".into(), BTreeMap::from([(id("old"), lines(4))]));
        b.module_lines
            .insert("g".into(), BTreeMap::from([(id("new"), lines(6))]));
        b.module_lines
            .insert("h".into(), BTreeMap::from([(id("only"), lines(1))]));
        let r = compare(&a, &b);
        assert_eq!(
            r.module_lines["g"],
            vec![
                ModuleLinesChange {
                    module: id("new"),
                    before: StateLines::default(),
                    after: lines(6)
                },
                ModuleLinesChange {
                    module: id("old"),
                    before: lines(4),
                    after: StateLines::default()
                },
            ]
        );
        assert_eq!(r.module_lines["h"].len(), 1);
        let text = render(&r);
        assert!(
            text.contains("new: clean 0->6") && text.contains("(new module)"),
            "{text}"
        );
        assert!(
            text.contains("old: clean 4->0") && text.contains("(module gone)"),
            "{text}"
        );
    }

    #[test]
    fn positional_ids_pair_by_hash_not_by_number() {
        let a = digest(&[
            ("a::<impl X>", UnitKind::Impl, "h1"),
            ("a::<impl X>#2", UnitKind::Impl, "h2"),
            ("a::<impl X>#3", UnitKind::Impl, "h3"),
        ]);
        let b = digest(&[
            ("a::<impl X>", UnitKind::Impl, "h1"),
            ("a::<impl X>#2", UnitKind::Impl, "h3"),
        ]);
        let r = compare(&a, &b);
        assert!(r.changed.is_empty(), "{:?}", r.changed);
        assert_eq!(r.removed, vec![id("a::<impl X>#2")]);
        assert!(r.added.is_empty() && r.moved.is_empty());
    }

    #[test]
    fn positional_leftovers_pair_in_order_and_count_as_changed() {
        let a = digest(&[
            ("a::<impl X>", UnitKind::Impl, "h1"),
            ("a::<impl X>#2", UnitKind::Impl, "h2"),
        ]);
        let b = digest(&[
            ("a::<impl X>", UnitKind::Impl, "h1"),
            ("a::<impl X>#2", UnitKind::Impl, "h9"),
            ("a::<impl X>#3", UnitKind::Impl, "h8"),
        ]);
        let r = compare(&a, &b);
        assert_eq!(r.changed, vec![id("a::<impl X>#2")]);
        assert_eq!(r.added, vec![id("a::<impl X>#3")]);
        assert!(r.removed.is_empty());
    }

    #[test]
    fn positional_states_follow_the_hash_pairing() {
        let mut a = digest(&[
            ("a::<impl X>", UnitKind::Impl, "h1"),
            ("a::<impl X>#2", UnitKind::Impl, "h2"),
        ]);
        let mut b = digest(&[("a::<impl X>", UnitKind::Impl, "h2")]);
        a.units
            .get_mut(&id("a::<impl X>#2"))
            .unwrap()
            .states
            .insert("g".into(), State::Body);
        b.units
            .get_mut(&id("a::<impl X>"))
            .unwrap()
            .states
            .insert("g".into(), State::Body);
        let r = compare(&a, &b);
        assert!(r.transitions.is_empty(), "{:?}", r.transitions);
    }

    #[test]
    fn a_cycle_that_grows_reports_only_the_new_member() {
        let mut a = digest(&[]);
        a.cycles_by_module = vec![vec![id("p"), id("q")]];
        let mut b = digest(&[]);
        b.cycles_by_module = vec![vec![id("p"), id("q"), id("t")]];
        let r = compare(&a, &b);
        assert_eq!(r.cycle_joined, vec![id("t")]);
        assert!(r.cycle_left.is_empty());
        let text = render(&r);
        assert!(
            text.contains("modules that joined a cycle (1)\n  t\n"),
            "{text}"
        );
        assert!(!text.contains("left a cycle"), "{text}");
        assert!(!text.contains(" -> "), "{text}");
    }

    #[test]
    fn crate_lines_sum_modules_and_list_the_headline_group_first() {
        let lines = |c, s, b, i| StateLines {
            clean: c,
            signature: s,
            body: b,
            inherited: i,
        };
        let mut a = digest(&[]);
        let mut b = digest(&[]);
        for (d, k) in [(&mut a, 1), (&mut b, 2)] {
            d.module_lines.insert(
                "zeta".into(),
                BTreeMap::from([
                    (id("core"), lines(10, 20, 30, 40)),
                    (id("core::m"), lines(100, 200, 300, 400)),
                    (id("other"), lines(1, 1, 1, 1)),
                ]),
            );
            d.module_lines.insert(
                "alpha".into(),
                BTreeMap::from([(id("core::m"), lines(10 * k, 20 * k, 30 * k, 40 * k))]),
            );
        }
        let mut r = compare(&a, &b);
        // zeta is the same on both sides: only alpha changes.
        assert!(!r.crate_lines.contains_key("zeta"), "{:?}", r.crate_lines);
        assert_eq!(r.crate_lines["alpha"].len(), 1);
        assert_eq!(r.crate_lines["alpha"][0].krate, "core");
        assert_eq!(r.crate_lines["alpha"][0].before, lines(10, 20, 30, 40));
        assert_eq!(r.crate_lines["alpha"][0].after, lines(20, 40, 60, 80));
        // Make zeta change too, with zeta as the headline.
        b.module_lines
            .get_mut("zeta")
            .unwrap()
            .insert(id("core"), lines(11, 22, 33, 44));
        r = compare(&a, &b);
        assert_eq!(r.crate_lines["zeta"].len(), 1);
        assert_eq!(r.crate_lines["zeta"][0].before, lines(110, 220, 330, 440));
        assert_eq!(r.crate_lines["zeta"][0].after, lines(111, 222, 333, 444));
        r.headline = Some("zeta".into());
        let text = render(&r);
        assert!(
            text.contains(
                "core: clean 110->111, signature 220->222, body 330->333, inherited 440->444"
            ),
            "{text}"
        );
        let (z, al) = (
            text.find("per state for zeta").expect("zeta"),
            text.find("per state for alpha").expect("alpha"),
        );
        assert!(z < al, "{text}");
        r.headline = None;
        let text = render(&r);
        assert!(text.find("for alpha").unwrap() < text.find("for zeta").unwrap());
    }

    #[test]
    fn only_numeric_suffixes_are_positional() {
        let a = digest(&[("a::f#x", Function, "h")]);
        let b = digest(&[("a::f#y", Function, "h")]);
        let r = compare(&a, &b);
        assert_eq!((r.removed.len(), r.added.len()), (1, 1), "{r:?}");
        assert!(r.moved.is_empty());
        let a = digest(&[("a::f#", Function, "h")]);
        let b = digest(&[("a::f", Function, "h")]);
        let r = compare(&a, &b);
        assert_eq!(r.removed, vec![id("a::f#")]);
        assert_eq!(r.added, vec![id("a::f")]);
        assert!(r.moved.is_empty() && r.changed.is_empty());
    }
}
