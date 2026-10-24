//! Wide structs and which modules touch which of their fields.

use crate::graph::Graph;
use crate::model::{UnitId, WideStruct};
use crate::structure::Structure;
use std::collections::{BTreeMap, BTreeSet};

pub fn compute(structure: &Structure, graph: &Graph, min_fields: usize) -> Vec<WideStruct> {
    let module_of: BTreeMap<&UnitId, &UnitId> = structure
        .units
        .iter()
        .filter_map(|d| d.unit.module.as_ref().map(|m| (&d.unit.id, m)))
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
