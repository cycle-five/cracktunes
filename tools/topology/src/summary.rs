//! A one-screen text summary of a topology.

use crate::config::Config;
use crate::digest::name_of;
use crate::model::{State, Topology, TopologyStats, Unit, UnitKind};
use crate::taint;
use std::collections::BTreeMap;
use std::fmt::Write;

const TOP_SEAMS: usize = 15;

/// `references: N workspace, M unmapped (P%)`; the unmapped share should stay under 1%.
pub fn references_line(st: &TopologyStats) -> String {
    let pct = if st.workspace_references == 0 {
        0.0
    } else {
        100.0 * f64::from(st.references_unmapped) / f64::from(st.workspace_references)
    };
    format!(
        "references: {} workspace, {} unmapped ({pct:.2}%)",
        st.workspace_references, st.references_unmapped
    )
}

pub fn render(t: &Topology, cfg: &Config) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", t.name);
    let _ = writeln!(s, "\ncrates:");
    for c in &t.crates {
        let _ = writeln!(
            s,
            "  {:<32} {:>8} lines{}",
            c.id,
            c.lines,
            if c.test_only { "  (test)" } else { "" }
        );
    }
    // The biggest non-test crate is the one the groups are measured on.
    let biggest = t
        .crates
        .iter()
        .filter(|c| !c.test_only)
        .max_by_key(|c| (c.lines, std::cmp::Reverse(&c.id)));
    for group in &t.groups {
        let headline = *group == cfg.headline_group;
        let _ = writeln!(
            s,
            "\ngroup {group}{}:",
            if headline { " (headline)" } else { "" }
        );
        let Some(c) = biggest else { continue };
        let mut lines: BTreeMap<State, u32> = BTreeMap::new();
        let mut counts: BTreeMap<State, u32> = BTreeMap::new();
        for u in t
            .units
            .iter()
            .filter(|u| u.krate == c.id && !u.test && u.kind != UnitKind::Module)
        {
            let Some(x) = t.taint.get(group).and_then(|m| m.get(&u.id)) else {
                continue;
            };
            *lines.entry(x.state).or_default() += u.own_lines;
            *counts.entry(x.state).or_default() += 1;
        }
        let _ = writeln!(s, "  {} (non-test):", c.id);
        for st in [
            State::Clean,
            State::Signature,
            State::Body,
            State::Inherited,
        ] {
            let _ = writeln!(
                s,
                "    {:<10} {:>8} lines {:>6} units",
                name_of(&st),
                lines.get(&st).copied().unwrap_or(0),
                counts.get(&st).copied().unwrap_or(0)
            );
        }
        if headline {
            let units: Vec<Unit> = t.units.clone();
            let empty = BTreeMap::new();
            let seams = taint::seams(
                t.taint.get(group).unwrap_or(&empty),
                &units,
                cfg.near_clean_max_cuts,
            );
            let _ = writeln!(s, "  top seams (at most {} cuts):", cfg.near_clean_max_cuts);
            let kind_of: BTreeMap<_, _> = units.iter().map(|u| (&u.id, u.kind)).collect();
            for seam in seams.iter().take(TOP_SEAMS) {
                let _ = writeln!(
                    s,
                    "    {} {} {} cuts={} lines={}",
                    seam.unit.0,
                    kind_of.get(&seam.unit).map_or_else(String::new, name_of),
                    name_of(&seam.state),
                    seam.cuts,
                    seam.lines
                );
            }
        }
    }
    let _ = writeln!(
        s,
        "\ncycles: {} by module, {} by top module",
        t.cycles.by_module.len(),
        t.cycles.by_top_module.len()
    );
    let _ = writeln!(
        s,
        "orphans: {} files, {} lines",
        t.orphans.len(),
        t.orphans.iter().map(|o| o.lines).sum::<u32>()
    );
    let _ = writeln!(s, "{}", references_line(&t.stats));
    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    for w in &t.warnings {
        *kinds.entry(name_of(&w.kind)).or_default() += 1;
    }
    let _ = writeln!(s, "warnings:");
    for (k, n) in &kinds {
        let _ = writeln!(s, "  {k}: {n}");
    }
    let _ = writeln!(
        s,
        "rust-analyzer error lines: {}",
        t.generator.ra_error_lines
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_line_gives_the_unmapped_share() {
        let st = |w, u| TopologyStats {
            workspace_references: w,
            references_unmapped: u,
            definitions_unmapped: 9,
        };
        assert_eq!(
            references_line(&st(1000, 5)),
            "references: 1000 workspace, 5 unmapped (0.50%)"
        );
        assert_eq!(
            references_line(&st(0, 0)),
            "references: 0 workspace, 0 unmapped (0.00%)"
        );
    }
}
