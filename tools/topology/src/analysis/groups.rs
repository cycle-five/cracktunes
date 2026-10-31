//! How strongly each module reaches into each group of external packages.

use crate::config::Config;
use crate::model::{UnitId, UnitKind};
use crate::placement::{Placed, RefTarget};
use crate::structure::Structure;
use std::collections::{BTreeMap, BTreeSet};

/// Per module: external refs of its non-test units per group, plus one for each
/// group its `use` items import. A module whose own package is in group G gets no G count.
pub fn module_groups(
    structure: &Structure,
    placed: &Placed,
    cfg: &Config,
) -> BTreeMap<UnitId, BTreeMap<String, u32>> {
    let package_of: BTreeMap<&str, &str> = structure
        .crates
        .iter()
        .map(|c| (c.id.as_str(), c.package.as_str()))
        .collect();
    let units: BTreeMap<&UnitId, _> = structure
        .units
        .iter()
        .map(|d| (&d.unit.id, &d.unit))
        .collect();
    let own_group = |krate: &str| package_of.get(krate).and_then(|p| cfg.group_of(p));
    let mut out: BTreeMap<UnitId, BTreeMap<String, u32>> = BTreeMap::new();
    for r in &placed.refs {
        let RefTarget::External { package, .. } = &r.target else {
            continue;
        };
        let Some(unit) = units.get(&r.from).filter(|u| !u.test) else {
            continue;
        };
        let module = if unit.kind == UnitKind::Module {
            Some(&unit.id)
        } else {
            unit.module.as_ref()
        };
        let (Some(module), Some(group)) = (module, cfg.group_of(package)) else {
            continue;
        };
        if own_group(&unit.krate) == Some(group) {
            continue;
        }
        *out.entry(module.clone())
            .or_default()
            .entry(group.to_string())
            .or_default() += 1;
    }
    for (module, packages) in &placed.imports {
        let Some(unit) = units.get(module).filter(|u| !u.test) else {
            continue;
        };
        let groups: BTreeSet<&str> = packages
            .iter()
            .filter_map(|p| cfg.group_of(p))
            .filter(|g| own_group(&unit.krate) != Some(*g))
            .collect();
        for g in groups {
            *out.entry(module.clone())
                .or_default()
                .entry(g.to_string())
                .or_default() += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CrateInfo, Loc, Position, TargetKind, Unit};
    use crate::placement::RawRef;
    use crate::structure::{Pos, Span, UnitDraft};

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    fn unit(i: &str, kind: UnitKind, krate: &str, module: Option<&str>, test: bool) -> UnitDraft {
        let p = Pos { line: 1, col: 0 };
        UnitDraft {
            unit: Unit {
                id: id(i),
                kind,
                krate: krate.into(),
                module: module.map(id),
                parent: None,
                file: "f.rs".into(),
                start_line: 1,
                end_line: 1,
                lines: 1,
                own_lines: 1,
                test,
                features: Default::default(),
                hash: String::new(),
            },
            ident: None,
            span: Span { start: p, end: p },
            body: None,
        }
    }

    fn ext(from: &str, package: &str) -> RawRef {
        RawRef {
            from: id(from),
            target: RefTarget::External {
                package: package.into(),
                symbol: "s".into(),
            },
            position: Position::Body,
            loc: Loc {
                file: "f.rs".into(),
                line: 1,
            },
        }
    }

    #[test]
    fn counts_refs_and_imports_with_exclusions() {
        let cfg = Config::from_toml_str("[groups]\ng = [\"ext\"]\n").unwrap();
        let krate = |i: &str, p: &str| CrateInfo {
            id: i.into(),
            package: p.into(),
            kind: TargetKind::Lib,
            root_file: "lib.rs".into(),
            test_only: false,
            lines: 1,
        };
        let m = UnitKind::Module;
        let f = UnitKind::Function;
        let s = Structure {
            crates: vec![krate("c1", "app"), krate("c2", "ext")],
            units: vec![
                unit("c1", m, "c1", None, false),
                unit("c1::m", m, "c1", Some("c1"), false),
                unit("c1::m::f", f, "c1", Some("c1::m"), false),
                unit("c1::m::t", f, "c1", Some("c1::m"), true),
                unit("c1::tm", m, "c1", Some("c1"), true),
                unit("c2", m, "c2", None, false),
                unit("c2::g", f, "c2", Some("c2"), false),
            ],
            ..Default::default()
        };
        let placed = Placed {
            refs: vec![
                ext("c1::m::f", "ext"),
                ext("c1::m::f", "ext"),
                ext("c1::m::f", "serde"),
                ext("c1::m::t", "ext"),
                ext("c2::g", "ext"),
                RawRef {
                    target: RefTarget::Unit(id("c1::m::f")),
                    ..ext("c1::m::f", "x")
                },
            ],
            imports: [
                (
                    id("c1::m"),
                    ["ext".to_string(), "ext".to_string(), "serde".to_string()].into(),
                ),
                (id("c1::tm"), ["ext".to_string()].into()),
                (id("c2"), ["ext".to_string()].into()),
            ]
            .into(),
            ..Default::default()
        };
        let got = module_groups(&s, &placed, &cfg);
        // Two refs plus one import; tests, serde, unit refs and group-internal use are out.
        assert_eq!(got, [(id("c1::m"), [("g".to_string(), 3)].into())].into());
    }
}
