//! The `extract` command: SCIP + syn in, `topology.json` and `digest.json` out.

use crate::analysis::{cycles, groups, wide};
use crate::config::Config;
use crate::digest::Digest;
use crate::model::{Generator, Topology, TopologyStats, Warning};
use crate::scip::ScipIndex;
use crate::workspace::Workspace;
use crate::{graph, placement, rust_analyzer, scip, structure, taint};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

/// Groups in output order: the headline group first, the rest by name.
fn group_names(cfg: &Config) -> Vec<String> {
    let mut groups: Vec<String> = cfg
        .groups
        .keys()
        .filter(|g| **g != cfg.headline_group)
        .cloned()
        .collect();
    if cfg.groups.contains_key(&cfg.headline_group) {
        groups.insert(0, cfg.headline_group.clone());
    }
    groups
}

fn warning_order(a: &Warning, b: &Warning) -> std::cmp::Ordering {
    (a.kind, &a.message, &a.loc).cmp(&(b.kind, &b.message, &b.loc))
}

pub fn build(
    ws: &Workspace,
    cfg: &Config,
    index: &ScipIndex,
    generator: Generator,
) -> Result<Topology> {
    let st = structure::read(ws, cfg)?;
    // Workspace packages the config puts in a group are external to placement.
    let external: BTreeSet<String> = ws
        .packages
        .iter()
        .filter(|p| cfg.group_of(&p.name).is_some())
        .map(|p| p.name.clone())
        .collect();
    let placed = placement::place_with(&st, index, ws, &external);
    let graph = graph::aggregate(&placed);
    let tainted = taint::compute(&st, &placed, &graph, cfg);

    let mut cycles = cycles::compute(&st, &graph);
    for list in [&mut cycles.by_module, &mut cycles.by_top_module] {
        list.sort_by(|a, b| (&a.krate, &a.modules).cmp(&(&b.krate, &b.modules)));
    }
    let mut wide_structs = wide::compute(&st, &graph, cfg.wide_struct_min_fields);
    wide_structs.sort_by(|a, b| a.id.cmp(&b.id));

    let mut crates = st.crates.clone();
    crates.sort_by(|a, b| a.id.cmp(&b.id));
    let mut units: Vec<_> = st.units.iter().map(|d| d.unit.clone()).collect();
    units.sort_by(|a, b| a.id.cmp(&b.id));
    let mut edges = graph.edges.clone();
    edges.sort_by(|a, b| (&a.from, &a.to, a.position).cmp(&(&b.from, &b.to, b.position)));
    let mut external_refs = graph.external.clone();
    external_refs.sort_by(|a, b| {
        (&a.from, &a.package, &a.symbol, a.position)
            .cmp(&(&b.from, &b.package, &b.symbol, b.position))
    });
    let mut reexports = placed.reexports.clone();
    reexports
        .sort_by(|a, b| (&a.module, &a.target, &a.first).cmp(&(&b.module, &b.target, &b.first)));
    let mut orphans = st.orphans.clone();
    orphans.sort_by(|a, b| (&a.package, &a.file).cmp(&(&b.package, &b.file)));
    let mut warnings: Vec<Warning> = st
        .warnings
        .iter()
        .chain(&placed.warnings)
        .cloned()
        .collect();
    warnings.sort_by(warning_order);

    Ok(Topology {
        generator,
        stats: TopologyStats {
            workspace_references: placed.stats.references,
            references_unmapped: placed.stats.references_unmapped,
            definitions_unmapped: placed.stats.definitions_unmapped,
        },
        name: if cfg.name.is_empty() {
            ws.name.clone()
        } else {
            cfg.name.clone()
        },
        groups: group_names(cfg),
        crates,
        units,
        edges,
        external: external_refs,
        reexports,
        imports: placed.imports.clone(),
        taint: tainted.by_group,
        module_lines: tainted.module_lines,
        cycles,
        wide_structs,
        module_groups: groups::module_groups(&st, &placed, cfg),
        orphans,
        warnings,
    })
}

/// The config at `config`, else `<workspace>/topology.toml` if present, else defaults.
pub fn load_config(workspace: &Path, config: Option<&Path>) -> Result<Config> {
    let default = workspace.join("topology.toml");
    let path = config
        .map(Path::to_path_buf)
        .or_else(|| default.is_file().then_some(default));
    Config::load(path.as_deref())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

pub fn run(
    workspace: &Path,
    config: Option<&Path>,
    out: &Path,
    scip_path: Option<&Path>,
) -> Result<Topology> {
    let cfg = load_config(workspace, config)?;
    let ws = Workspace::load(workspace, &cfg.test_crates)?;
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let (index, version, error_lines) = match scip_path {
        Some(p) => (scip::read(p)?, "external index".to_string(), 0),
        None => {
            let ra = rust_analyzer::run(&ws, &cfg, out)?;
            (scip::read(&ra.index)?, ra.version, ra.error_lines)
        },
    };
    let generator = Generator {
        tool_hash: crate::TOOL_HASH.to_string(),
        rust_analyzer: version,
        config_hash: cfg.hash(),
        ra_error_lines: error_lines,
    };
    let topology = build(&ws, &cfg, &index, generator)?;
    write_json(&out.join("topology.json"), &topology)?;
    write_json(&out.join("digest.json"), &Digest::from_topology(&topology))?;
    Ok(topology)
}
