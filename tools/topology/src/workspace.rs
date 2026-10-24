//! Workspace discovery through `cargo metadata --no-deps`.

use crate::model::TargetKind;
use anyhow::{Context, Result};
use cargo_metadata::{MetadataCommand, TargetKind as CmKind};
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
