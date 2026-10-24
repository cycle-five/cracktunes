//! Wide structs and which modules touch which of their fields.

use crate::graph::Graph;
use crate::model::{UnitId, WideStruct};
use crate::structure::Structure;
use std::collections::{BTreeMap, BTreeSet};

pub fn compute(structure: &Structure, graph: &Graph, min_fields: usize) -> Vec<WideStruct> {
    // Test units, and everything in a test module, say nothing about production use.
    let test_units: BTreeSet<&UnitId> = structure
        .units
        .iter()
        .filter(|d| d.unit.test)
        .map(|d| &d.unit.id)
        .collect();
    let module_of: BTreeMap<&UnitId, &UnitId> = structure
        .units
        .iter()
        .filter(|d| !d.unit.test)
        .filter_map(|d| d.unit.module.as_ref().map(|m| (&d.unit.id, m)))
        .filter(|(_, m)| !test_units.contains(m))
        .collect();
    let mut fields: BTreeMap<&UnitId, BTreeSet<&str>> = BTreeMap::new();
    for f in &structure.fields {
        fields.entry(&f.owner).or_default().insert(&f.name);
    }
    let mut uses: BTreeMap<(&UnitId, &str), BTreeMap<UnitId, BTreeSet<UnitId>>> = BTreeMap::new();
    for (owner, field, from) in &graph.field_uses {
        let Some(&module) = module_of.get(from) else {
            continue;
        };
        uses.entry((owner, field.as_str()))
            .or_default()
            .entry(module.clone())
            .or_default()
            .insert(from.clone());
    }
    fields
        .into_iter()
        .filter(|(_, names)| names.len() >= min_fields)
        .map(|(owner, names)| WideStruct {
            id: owner.clone(),
            uses: names
                .iter()
                .map(|n| (n.to_string(), uses.remove(&(owner, *n)).unwrap_or_default()))
                .collect(),
            fields: names.iter().map(|n| n.to_string()).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Unit, UnitKind};
    use crate::structure::{FieldDef, Pos, Span, UnitDraft};

    fn id(s: &str) -> UnitId {
        UnitId(s.into())
    }

    fn draft(name: &str, kind: UnitKind, module: Option<&str>, test: bool) -> UnitDraft {
        let p = Pos { line: 1, col: 0 };
        UnitDraft {
            unit: Unit {
                id: id(name),
                kind,
                krate: "c".into(),
                module: module.map(id),
                parent: None,
                file: "f.rs".into(),
                start_line: 1,
                end_line: 1,
                lines: 1,
                own_lines: 1,
                test,
                features: BTreeSet::new(),
                hash: String::new(),
            },
            ident: None,
            span: Span { start: p, end: p },
            body: None,
        }
    }

    fn setup() -> (Structure, Graph) {
        let structure = Structure {
            units: vec![
                draft("c::S", UnitKind::Struct, Some("c"), false),
                draft("c::prod", UnitKind::Function, Some("c::m"), false),
                draft("c::unit_test", UnitKind::Function, Some("c::m"), true),
                draft(
                    "c::tests::helper",
                    UnitKind::Function,
                    Some("c::tests"),
                    false,
                ),
                draft("c::tests", UnitKind::Module, Some("c"), true),
            ],
            fields: ["a", "b"]
                .into_iter()
                .map(|n| FieldDef {
                    owner: id("c::S"),
                    name: n.into(),
                    file: "f.rs".into(),
                    ident: Pos { line: 1, col: 0 },
                })
                .collect(),
            ..Structure::default()
        };
        let graph = Graph {
            field_uses: ["c::prod", "c::unit_test", "c::tests::helper"]
                .into_iter()
                .map(|f| (id("c::S"), "a".to_string(), id(f)))
                .collect(),
            ..Graph::default()
        };
        (structure, graph)
    }

    #[test]
    fn test_units_and_test_modules_are_left_out_of_the_matrix() {
        let (s, g) = setup();
        let w = compute(&s, &g, 2);
        assert_eq!(w.len(), 1);
        let a = &w[0].uses["a"];
        assert_eq!(a.keys().collect::<Vec<_>>(), vec![&id("c::m")]);
        assert_eq!(a[&id("c::m")], BTreeSet::from([id("c::prod")]));
        assert!(w[0].uses["b"].is_empty());
    }

    #[test]
    fn narrow_structs_are_dropped() {
        let (s, g) = setup();
        assert!(compute(&s, &g, 3).is_empty());
    }
}
