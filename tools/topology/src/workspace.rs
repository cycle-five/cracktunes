//! Workspace discovery through `cargo metadata --no-deps`.

use crate::model::TargetKind;
use anyhow::{Context, Result};
use cargo_metadata::{MetadataCommand, TargetKind as CmKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub crate_id: String,
    pub name: String,
    pub kind: TargetKind,
    pub root_file: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub dir: String,
    pub test_only: bool,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub packages: Vec<Package>,
}

fn rel(root: &Path, p: &Path) -> String {
    let r = p.strip_prefix(root).unwrap_or(p);
    r.to_string_lossy().replace('\\', "/")
}

fn kind_of(kinds: &[CmKind]) -> Option<TargetKind> {
    kinds.iter().find_map(|k| match k {
        CmKind::Lib | CmKind::RLib | CmKind::CDyLib | CmKind::DyLib | CmKind::StaticLib => {
            Some(TargetKind::Lib)
        },
        CmKind::ProcMacro => Some(TargetKind::ProcMacro),
        CmKind::Bin => Some(TargetKind::Bin),
        CmKind::Test => Some(TargetKind::Test),
        CmKind::Example => Some(TargetKind::Example),
        CmKind::Bench => Some(TargetKind::Bench),
        _ => None,
    })
}

fn normalise(name: &str) -> String {
    name.replace('-', "_")
}

/// Two targets with one crate id (each package's `tests/it.rs` is `it[test]`) would
/// merge their units. Qualify every colliding id with its package: `<package>/<id>`.
fn qualify_collisions(packages: &mut [Package]) {
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    for t in packages.iter().flat_map(|p| &p.targets) {
        *seen.entry(t.crate_id.clone()).or_default() += 1;
    }
    for p in packages.iter_mut() {
        for t in &mut p.targets {
            if seen[&t.crate_id] > 1 {
                t.crate_id = format!("{}/{}", p.name, t.crate_id);
            }
        }
    }
}

impl Workspace {
    pub fn load(root: &Path, test_crates: &[String]) -> Result<Workspace> {
        let root = root
            .canonicalize()
            .with_context(|| format!("workspace {}", root.display()))?;
        let meta = MetadataCommand::new()
            .manifest_path(root.join("Cargo.toml"))
            .no_deps()
            .exec()
            .context("cargo metadata")?;
        let members: Vec<_> = meta.workspace_packages();
        let mut packages = Vec::new();
        for p in members {
            let has_lib = p.targets.iter().any(|t| {
                matches!(
                    kind_of(&t.kind),
                    Some(TargetKind::Lib | TargetKind::ProcMacro)
                )
            });
            let mut targets = Vec::new();
            for t in &p.targets {
                let Some(kind) = kind_of(&t.kind) else {
                    continue; // build scripts are not crates
                };
                let n = normalise(&t.name);
                let crate_id = match kind {
                    TargetKind::Lib | TargetKind::ProcMacro => n,
                    TargetKind::Bin if has_lib && n == normalise(&p.name) => format!("{n}[bin]"),
                    TargetKind::Bin => n,
                    TargetKind::Test => format!("{n}[test]"),
                    TargetKind::Example => format!("{n}[example]"),
                    TargetKind::Bench => format!("{n}[bench]"),
                };
                targets.push(Target {
                    crate_id,
                    name: t.name.clone(),
                    kind,
                    root_file: rel(&root, t.src_path.as_std_path()),
                });
            }
            let dir = p
                .manifest_path
                .as_std_path()
                .parent()
                .map(|d| rel(&root, d))
                .unwrap_or_default();
            packages.push(Package {
                name: p.name.to_string(),
                version: p.version.to_string(),
                dir,
                test_only: test_crates.iter().any(|t| *t == *p.name),
                targets,
            });
        }
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        qualify_collisions(&mut packages);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Workspace {
            root,
            name,
            packages,
        })
    }

    pub fn is_member(&self, package: &str) -> bool {
        self.packages.iter().any(|p| p.name == package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, ids: &[&str]) -> Package {
        Package {
            name: name.into(),
            version: "0.1.0".into(),
            dir: name.into(),
            test_only: false,
            targets: ids
                .iter()
                .map(|id| Target {
                    crate_id: (*id).into(),
                    name: (*id).into(),
                    kind: TargetKind::Lib,
                    root_file: format!("{name}/{id}.rs"),
                })
                .collect(),
        }
    }

    fn ids(p: &[Package]) -> Vec<Vec<&str>> {
        p.iter()
            .map(|p| p.targets.iter().map(|t| t.crate_id.as_str()).collect())
            .collect()
    }

    #[test]
    fn colliding_crate_ids_are_qualified_by_package() {
        let mut ps = vec![
            package("a", &["a", "it[test]"]),
            package("b", &["b", "it[test]"]),
            package("c", &["c"]),
        ];
        qualify_collisions(&mut ps);
        assert_eq!(
            ids(&ps),
            vec![vec!["a", "a/it[test]"], vec!["b", "b/it[test]"], vec!["c"]]
        );
    }

    #[test]
    fn distinct_ids_are_left_alone() {
        let mut ps = vec![package("a", &["a", "x[test]"]), package("b", &["b"])];
        qualify_collisions(&mut ps);
        assert_eq!(ids(&ps), vec![vec!["a", "x[test]"], vec!["b"]]);
    }
}
