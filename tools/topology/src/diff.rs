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

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct DiffReport {
    pub notes: Vec<String>,
    pub added: Vec<UnitId>,
    pub removed: Vec<UnitId>,
    pub changed: Vec<UnitId>,
    pub moved: Vec<Move>,
    pub transitions: BTreeMap<String, Vec<Transition>>,
    pub module_lines: BTreeMap<String, Vec<ModuleLinesChange>>,
    pub cycles_added: Vec<Vec<UnitId>>,
    pub cycles_removed: Vec<Vec<UnitId>>,
    pub wide_changes: Vec<String>,
    pub orphans_added: Vec<String>,
    pub orphans_removed: Vec<String>,
    pub warnings: (BTreeMap<String, u32>, BTreeMap<String, u32>),
}

/// Read a `topology.json` or a `digest.json`.
pub fn load(path: &Path) -> Result<Digest> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    if let Ok(t) = serde_json::from_str::<Topology>(&text) {
        return Ok(Digest::from_topology(&t));
    }
    if let Ok(d) = serde_json::from_str::<Digest>(&text) {
        return Ok(d);
    }
    bail!(
        "{} is neither a topology.json nor a digest.json",
        path.display()
    )
}

/// The last path segment of a unit id (the last two for methods).
fn name_of(id: &UnitId, kind: UnitKind) -> String {
    let keep = if kind == UnitKind::Method { 2 } else { 1 };
    let parts: Vec<&str> = id.0.split("::").collect();
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
        m.entry((kind, name_of(id, kind))).or_default().push(id);
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
            .entry((u.kind, name_of(id, u.kind), u.hash.as_str()))
            .or_default()
            .0
            .push(id);
    }
    for id in added.iter() {
        let u = &b.units[id];
        by_hash
            .entry((u.kind, name_of(id, u.kind), u.hash.as_str()))
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
    let mut removed: BTreeSet<UnitId> = a
        .units
        .keys()
        .filter(|k| !b.units.contains_key(*k))
        .cloned()
        .collect();
    let mut added: BTreeSet<UnitId> = b
        .units
        .keys()
        .filter(|k| !a.units.contains_key(*k))
        .cloned()
        .collect();
    let changed: Vec<UnitId> = a
        .units
        .iter()
        .filter(|(k, u)| b.units.get(*k).is_some_and(|v| v.hash != u.hash))
        .map(|(k, _)| k.clone())
        .collect();
    let moved = find_moves(a, b, &mut removed, &mut added);

    // Unit pairs whose state is compared: same id, or a recognised move.
    let mut pairs: Vec<(&UnitId, &UnitId)> = a
        .units
        .keys()
        .filter(|k| b.units.contains_key(*k))
        .map(|k| (k, k))
        .collect();
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
    for v in transitions.values_mut() {
        v.sort_by(|x, y| x.unit.cmp(&y.unit));
    }

    let mut module_lines: BTreeMap<String, Vec<ModuleLinesChange>> = BTreeMap::new();
    for (g, ma) in &a.module_lines {
        let Some(mb) = b.module_lines.get(g) else {
            continue;
        };
        for (m, before) in ma {
            if let Some(after) = mb.get(m)
                && before != after
            {
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
        notes: generator_notes(a, b),
        added: added.into_iter().collect(),
        removed: removed.into_iter().collect(),
        changed,
        moved,
        transitions,
        module_lines,
        cycles_added,
        cycles_removed,
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

fn cycle_line(c: &[UnitId]) -> String {
    c.iter()
        .map(|u| u.0.as_str())
        .collect::<Vec<_>>()
        .join(" -> ")
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
    for (g, ts) in &r.transitions {
        section(
            &mut out,
            &format!("state changes for {g}"),
            ts.iter()
                .map(|t| {
                    format!(
                        "{}: {} -> {}",
                        t.unit.0,
                        state_name(t.before),
                        state_name(t.after)
                    )
                })
                .collect(),
        );
    }
    for (g, ms) in &r.module_lines {
        section(
            &mut out,
            &format!("module lines for {g}"),
            ms.iter()
                .map(|m| {
                    format!(
                        "{}: clean {}->{}, signature {}->{}, body {}->{}, inherited {}->{}",
                        m.module.0,
                        m.before.clean,
                        m.after.clean,
                        m.before.signature,
                        m.after.signature,
                        m.before.body,
                        m.after.body,
                        m.before.inherited,
                        m.after.inherited
                    )
                })
                .collect(),
        );
    }
    section(
        &mut out,
        "cycles added",
        r.cycles_added.iter().map(|c| cycle_line(c)).collect(),
    );
    section(
        &mut out,
        "cycles removed",
        r.cycles_removed.iter().map(|c| cycle_line(c)).collect(),
    );
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
    use crate::model::Generator;

    fn generator() -> Generator {
        Generator {
            tool_hash: "t".into(),
            rust_analyzer: "ra 1".into(),
            config_hash: "c".into(),
            ra_error_lines: 0,
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
            "cycles added",
            "cycles removed",
            "orphans added",
            "orphans removed",
            "warnings",
            "k: 1 -> 2",
            "S.f: +m2 -m1",
            "T.g: +m3",
            "r -> s",
            "p -> q",
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
    fn load_reads_digest_and_rejects_garbage() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        let _ = std::fs::create_dir_all(&dir);
        let good = dir.join(format!("diff-load-{}.json", std::process::id()));
        let d = digest(&[("a::f", Function, "h")]);
        std::fs::write(&good, serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(load(&good).unwrap(), d);
        std::fs::write(&good, "{}").unwrap();
        let err = load(&good).unwrap_err();
        assert!(format!("{err}").contains("diff-load-"), "{err}");
        let _ = std::fs::remove_file(&good);
        assert!(load(&dir.join("does-not-exist.json")).is_err());
    }
}
