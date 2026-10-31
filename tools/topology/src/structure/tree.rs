//! Module trees: following `mod x;` from each target's root file, and finding
//! the `.rs` files no tree reaches.

use super::items::{Collector, ModCtx};
use super::{FileInfo, Structure};
use crate::config::Config;
use crate::model::{CrateInfo, Loc, Orphan, TargetKind, WarningKind};
use crate::workspace::Workspace;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;
use syn::{Attribute, ItemMod};

/// Reads every target's module tree and the orphans no tree reaches.
pub fn read(ws: &Workspace, _cfg: &Config) -> Result<Structure> {
    let mut col = Collector::new(Some(&ws.root));
    let mut crates = Vec::new();
    for pkg in &ws.packages {
        let mut targets: Vec<_> = pkg.targets.iter().collect();
        targets.sort_by_key(|t| rank(t.kind)); // stable: lib, bin, then the rest
        for t in targets {
            let test = pkg.test_only
                || matches!(
                    t.kind,
                    TargetKind::Test | TargetKind::Example | TargetKind::Bench
                );
            col.begin_crate(&t.crate_id, test);
            if col.visited.insert(t.root_file.clone()) {
                match std::fs::read_to_string(ws.root.join(&t.root_file)) {
                    Ok(text) => {
                        let dir = parent_dir(&t.root_file);
                        col.add_file_module(&t.root_file, text, None, dir);
                    },
                    Err(e) => col.warn(
                        WarningKind::UnparsedFile,
                        format!("{}: {e}", t.root_file),
                        None,
                    ),
                }
            }
            crates.push(CrateInfo {
                id: t.crate_id.clone(),
                package: pkg.name.clone(),
                kind: t.kind,
                root_file: t.root_file.clone(),
                test_only: pkg.test_only,
                lines: 0,
            });
        }
    }
    let reached = std::mem::take(&mut col.visited);
    let mut out = col.finish();
    for c in &mut crates {
        c.lines = out
            .files
            .values()
            .filter(|f| f.krate == c.id)
            .map(line_count)
            .sum();
    }
    crates.sort_by(|a, b| a.id.cmp(&b.id));
    out.crates = crates;
    out.orphans = orphans(ws, &reached);
    out.warnings.sort_by(|a, b| {
        (a.kind, a.loc.as_ref(), &a.message).cmp(&(b.kind, b.loc.as_ref(), &b.message))
    });
    Ok(out)
}

fn rank(k: TargetKind) -> u8 {
    match k {
        TargetKind::Lib | TargetKind::ProcMacro => 0,
        TargetKind::Bin => 1,
        _ => 2,
    }
}

fn line_count(f: &FileInfo) -> u32 {
    f.text.lines().count() as u32
}

fn parent_dir(file: &str) -> String {
    file.rsplit_once('/')
        .map(|(d, _)| d.to_string())
        .unwrap_or_default()
}

/// `base/rel` with `.` and `..` folded away.
fn join_norm(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {},
            ".." => {
                parts.pop();
            },
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The `#[path = "…"]` value on a `mod`, if any.
fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| {
        if !a.path().is_ident("path") {
            return None;
        }
        match &a.meta {
            syn::Meta::NameValue(nv) => match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        }
    })
}

/// Where `mod name;` may live, as `(file, directory for its own children)`,
/// in the order rustc tries them.
fn candidates(
    dir: &str,
    file: &str,
    in_inline: bool,
    name: &str,
    path: Option<&str>,
) -> Vec<(String, String)> {
    if let Some(p) = path {
        let base = if in_inline {
            dir.to_string()
        } else {
            parent_dir(file)
        };
        let f = join_norm(&base, p);
        let d = parent_dir(&f);
        return vec![(f, d)];
    }
    let child = join_norm(dir, name);
    vec![
        (format!("{child}.rs"), child.clone()),
        (format!("{child}/mod.rs"), child),
    ]
}

impl Collector<'_> {
    /// Follows `mod name;` regardless of `cfg`.
    pub(super) fn file_mod(&mut self, ctx: &ModCtx, name: &str, m: &ItemMod) {
        let Some(root) = self.root else { return };
        let path = path_attr(&m.attrs);
        let found = candidates(&ctx.dir, &ctx.file, ctx.in_inline, name, path.as_deref())
            .into_iter()
            .find(|(f, _)| root.join(f).is_file());
        let loc = Loc {
            file: ctx.file.clone(),
            line: m.ident.span().start().line as u32,
        };
        let Some((file, dir)) = found else {
            self.warn(
                WarningKind::UnparsedFile,
                format!("{}: no file for `mod {name};`", ctx.file),
                Some(loc),
            );
            return;
        };
        if !self.visited.insert(file.clone()) {
            return; // already belongs to the first target that reached it
        }
        match std::fs::read_to_string(root.join(&file)) {
            Ok(text) => self.add_file_module(&file, text, Some((ctx, name, &m.attrs)), dir),
            Err(e) => self.warn(WarningKind::UnparsedFile, format!("{file}: {e}"), Some(loc)),
        }
    }
}

/// `.rs` files under each package that no tree reached.
fn orphans(ws: &Workspace, reached: &BTreeSet<String>) -> Vec<Orphan> {
    let mut found = Vec::new();
    for pkg in &ws.packages {
        let top = ws.root.join(&pkg.dir);
        let walker = walkdir::WalkDir::new(&top).into_iter().filter_entry(|e| {
            if e.depth() == 0 || !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            // another package (or a standalone tool) living inside this one
            name != "target" && !name.starts_with('.') && !e.path().join("Cargo.toml").is_file()
        });
        for e in walker.flatten() {
            if !e.file_type().is_file() || e.path().extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let rel = rel_path(&ws.root, e.path());
            let is_build_script = e.depth() == 1 && e.file_name() == "build.rs";
            if is_build_script || reached.contains(&rel) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(e.path()) {
                found.push(Orphan {
                    package: pkg.name.clone(),
                    file: rel,
                    lines: text.lines().count() as u32,
                });
            }
        }
    }
    found.sort_by(|a, b| (&a.package, &a.file).cmp(&(&b.package, &b.file)));
    found
}

fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_norm_folds_dots() {
        assert_eq!(join_norm("a/b", "c.rs"), "a/b/c.rs");
        assert_eq!(join_norm("a/b", "../c.rs"), "a/c.rs");
        assert_eq!(join_norm("a/b", "./x/../c.rs"), "a/b/c.rs");
        assert_eq!(join_norm("", "c.rs"), "c.rs");
        assert_eq!(join_norm("", "a//b"), "a/b");
    }

    #[test]
    fn parent_dir_of_files() {
        assert_eq!(parent_dir("a/b/c.rs"), "a/b");
        assert_eq!(parent_dir("c.rs"), "");
    }

    #[test]
    fn candidates_follow_rustc_order() {
        assert_eq!(
            candidates("src", "src/lib.rs", false, "x", None),
            [
                ("src/x.rs".to_string(), "src/x".to_string()),
                ("src/x/mod.rs".to_string(), "src/x".to_string())
            ]
        );
        assert_eq!(candidates("", "lib.rs", false, "x", None)[0].0, "x.rs");
    }

    #[test]
    fn path_attribute_is_relative_to_the_file_or_the_inline_dir() {
        // outside an inline module: the declaring file's directory; the loaded file acts like mod.rs
        assert_eq!(
            candidates("src/foo", "src/foo.rs", false, "x", Some("gen/y.rs")),
            [("src/gen/y.rs".to_string(), "src/gen".to_string())]
        );
        // inside an inline module: the module's directory
        assert_eq!(
            candidates("src/foo/inl", "src/foo.rs", true, "x", Some("y.rs")),
            [("src/foo/inl/y.rs".to_string(), "src/foo/inl".to_string())]
        );
    }

    #[test]
    fn path_attr_reads_only_string_path() {
        let m: ItemMod = syn::parse_str("#[path = \"a/b.rs\"] mod x;").unwrap();
        assert_eq!(path_attr(&m.attrs).as_deref(), Some("a/b.rs"));
        let m: ItemMod = syn::parse_str("#[cfg(unix)] mod x;").unwrap();
        assert_eq!(path_attr(&m.attrs), None);
        let m: ItemMod = syn::parse_str("#[path(x)] mod x;").unwrap();
        assert_eq!(path_attr(&m.attrs), None);
        let m: ItemMod = syn::parse_str("#[path = 3] mod x;").unwrap();
        assert_eq!(path_attr(&m.attrs), None);
    }

    #[test]
    fn rank_orders_lib_then_bin_then_rest() {
        assert!(rank(TargetKind::Lib) < rank(TargetKind::Bin));
        assert!(rank(TargetKind::Bin) < rank(TargetKind::Test));
        assert_eq!(rank(TargetKind::ProcMacro), rank(TargetKind::Lib));
        assert_eq!(rank(TargetKind::Example), rank(TargetKind::Bench));
    }

    #[test]
    fn rel_path_uses_slashes() {
        assert_eq!(rel_path(Path::new("/r"), Path::new("/r/a/b.rs")), "a/b.rs");
    }
}
