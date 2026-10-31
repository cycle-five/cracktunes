use crate::common::fixture_structure;
use rust_topology::model::{UnitKind, WarningKind};
use rust_topology::structure::{Pos, UnitDraft};
use std::collections::BTreeSet;

fn unit(id: &str) -> &'static UnitDraft {
    fixture_structure()
        .units
        .iter()
        .find(|u| u.unit.id.0 == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

#[test]
fn fixture_units_present() {
    for id in [
        "appcore::rules::greet",
        "appcore::handlers::<impl EventHandler for Handler>",
        "appcore::handlers::<Handler as EventHandler>::on_event",
        "appcore::handlers::Bot::ping",
        "appcore::text::after_emoji",
        "appcore::text::BANNER",
        "appbin[bin]::run",
        "appbin::run",
        "it[test]::it_runs",
    ] {
        unit(id);
    }
    assert_eq!(unit("appcore").unit.kind, UnitKind::Module);
    assert_eq!(unit("appcore::rules").unit.file, "appcore/src/rules.rs");
    assert_eq!(
        unit("appcore::rules").unit.module.as_ref().unwrap().0,
        "appcore"
    );
    assert_eq!(
        unit("appcore::handlers::Bot::ping")
            .unit
            .parent
            .as_ref()
            .unwrap()
            .0,
        "appcore::handlers::<impl Bot>"
    );
    assert!(fixture_structure().warnings.is_empty());
}

#[test]
fn gated_unit_has_feature() {
    let f: BTreeSet<_> = ["extra".to_string()].into();
    assert_eq!(unit("appcore::gated::gated_fn").unit.features, f);
    assert!(unit("appcore::rules::greet").unit.features.is_empty());
}

#[test]
fn test_units_marked() {
    for id in [
        "appcore::tests::t",
        "it[test]::it_runs",
        "apptesting::helper",
    ] {
        assert!(unit(id).unit.test, "{id}");
    }
    assert!(!unit("appcore::rules::greet").unit.test);
    assert!(!unit("appcore").unit.test);
}

#[test]
fn orphan_found() {
    let o = &fixture_structure().orphans;
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(o[0].file, "appcore/src/orphan.rs");
    assert_eq!(o[0].package, "appcore");
    assert_eq!(o[0].lines, 1);
}

#[test]
fn wide_fields_recorded() {
    let n = fixture_structure()
        .fields
        .iter()
        .filter(|f| f.owner.0 == "appcore::state::Wide")
        .count();
    assert_eq!(n, 10);
    let f1 = fixture_structure()
        .fields
        .iter()
        .find(|f| f.name == "f1")
        .unwrap();
    assert_eq!(
        (f1.file.as_str(), f1.ident),
        ("appcore/src/state.rs", Pos { line: 2, col: 8 })
    );
}

#[test]
fn use_span_recorded() {
    let s = fixture_structure();
    let text = s
        .uses
        .iter()
        .find(|u| u.file == "appcore/src/text.rs")
        .unwrap();
    assert!(!text.public);
    assert_eq!(text.module.0, "appcore::text");
    assert_eq!(text.span.start, Pos { line: 1, col: 0 });
    let types = s
        .uses
        .iter()
        .find(|u| u.file == "apptypes/src/lib.rs")
        .unwrap();
    assert!(types.public);
    assert_eq!(types.module.0, "apptypes");
}

#[test]
fn crate_lines_and_files() {
    let s = fixture_structure();
    let core = s.crates.iter().find(|c| c.id == "appcore").unwrap();
    let want: u32 = s
        .files
        .iter()
        .filter(|(_, f)| f.krate == "appcore")
        .map(|(_, f)| f.text.lines().count() as u32)
        .sum();
    assert_eq!(core.lines, want);
    assert!(core.lines > 40);
    // the orphan is not counted
    assert!(!s.files.contains_key("appcore/src/orphan.rs"));
    assert_eq!(s.files["appcore/src/rules.rs"].module.0, "appcore::rules");
    // `mod tests;` is only reached through `#[cfg(test)]`, still followed
    assert!(s.files.contains_key("appcore/src/tests.rs"));
    assert!(s.files.contains_key("appcore/src/gated.rs"));
}

#[test]
fn emoji_line_regions_are_exact() {
    // line 11: `pub const BANNER: &str = "🎵🎵"; pub fn after_emoji() -> u32 { 7 }`
    let b = unit("appcore::text::BANNER");
    let a = unit("appcore::text::after_emoji");
    assert_eq!(b.span.start, Pos { line: 11, col: 0 });
    assert_eq!(b.ident, Some(Pos { line: 11, col: 10 }));
    assert_eq!(a.span.start, Pos { line: 11, col: 31 }); // chars, not bytes
    assert_eq!(a.ident, Some(Pos { line: 11, col: 38 }));
    assert!(b.span.end <= a.span.start);
    let body = a.body.unwrap();
    assert_eq!(body.start, Pos { line: 11, col: 59 });
}

#[test]
fn attribute_is_inside_the_span_and_body_is_the_block() {
    let c = unit("appcore::handlers::cmd");
    assert_eq!(c.span.start, Pos { line: 17, col: 0 });
    assert!(c.span.contains(Pos { line: 17, col: 3 }));
    let body = c.body.unwrap();
    assert_eq!(body.start, Pos { line: 18, col: 20 });
    assert_eq!(body.end, Pos { line: 20, col: 1 });
    assert!(!body.contains(body.end));
    assert_eq!(c.unit.start_line, 17);
    assert_eq!(c.unit.end_line, 20);
    assert_eq!(c.unit.lines, 4);
}

#[test]
fn structs_traits_and_modules_have_no_body() {
    assert!(unit("appcore::handlers::Bot").body.is_none());
    assert!(unit("appcore::handlers::<impl Bot>").body.is_none());
    assert!(unit("appcore::rules").body.is_none());
    let k = unit("appcore::text::BANNER");
    assert_eq!(k.unit.kind, UnitKind::Const);
    assert!(k.body.is_some());
}

#[test]
fn own_lines_exclude_children_and_modules_span_the_file() {
    let m = unit("appcore::rules");
    assert_eq!(m.span.start, Pos { line: 1, col: 0 });
    let text = &fixture_structure().files["appcore/src/rules.rs"].text;
    assert_eq!(m.unit.lines, text.lines().count() as u32);
    // every non-blank line of rules.rs belongs to an item
    let blank = text.lines().filter(|l| l.is_empty()).count() as u32;
    assert_eq!(m.unit.own_lines, blank);
    let imp = unit("appcore::handlers::<impl Bot>");
    assert_eq!((imp.unit.lines, imp.unit.own_lines), (5, 2));
    let root = unit("appcore");
    // `mod x;` declarations live in lib.rs, the files they load do not
    assert_eq!(root.unit.own_lines, root.unit.lines);
    assert_eq!(root.unit.file, "appcore/src/lib.rs");
}

#[test]
fn hashes_differ_between_units_and_are_hex() {
    let a = &unit("appcore::ring_a::a").unit.hash;
    let b = &unit("appcore::ring_b::b").unit.hash;
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn unparsed_kind_is_not_used_on_good_workspace() {
    assert!(
        fixture_structure()
            .warnings
            .iter()
            .all(|w| w.kind != WarningKind::UnparsedFile)
    );
}

mod scratch {
    use super::*;
    use rust_topology::model::TargetKind;
    use rust_topology::structure::{Structure, read};
    use rust_topology::workspace::{Package, Target, Workspace};
    use std::path::PathBuf;

    fn dir(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("structure-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &std::path::Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn target(id: &str, kind: TargetKind, root_file: &str) -> Target {
        Target {
            crate_id: id.into(),
            name: id.into(),
            kind,
            root_file: root_file.into(),
        }
    }

    fn read_ws(root: PathBuf, targets: Vec<Target>) -> Structure {
        let ws = Workspace {
            root,
            name: "w".into(),
            packages: vec![Package {
                name: "p".into(),
                version: "0.1.0".into(),
                dir: "p".into(),
                test_only: false,
                targets,
            }],
        };
        read(
            &ws,
            &rust_topology::config::Config::from_toml_str("").unwrap(),
        )
        .unwrap()
    }

    fn ids(s: &Structure) -> Vec<&str> {
        s.units.iter().map(|u| u.unit.id.0.as_str()).collect()
    }

    #[test]
    fn mod_files_nested_dirs_and_path_attributes() {
        let r = dir("paths");
        put(
            &r,
            "p/src/lib.rs",
            "mod a;\nmod b;\nmod inl { mod deep; }\n#[path = \"other/named.rs\"] mod renamed;\n#[cfg(windows)] mod gone;\n",
        );
        put(&r, "p/src/a.rs", "pub mod sub;\npub fn fa() {}\n");
        put(&r, "p/src/a/sub.rs", "pub fn fsub() {}\n");
        put(&r, "p/src/b/mod.rs", "pub mod c;\n");
        put(&r, "p/src/b/c.rs", "pub fn fc() {}\n");
        put(&r, "p/src/inl/deep.rs", "pub fn fdeep() {}\n");
        put(&r, "p/src/other/named.rs", "pub fn fnamed() {}\nmod kid;\n");
        put(&r, "p/src/other/kid.rs", "pub fn fkid() {}\n");
        let s = read_ws(r, vec![target("c", TargetKind::Lib, "p/src/lib.rs")]);
        for id in [
            "c::a::fa",
            "c::a::sub::fsub",
            "c::b::c::fc",
            "c::inl::deep::fdeep",
            "c::renamed::fnamed",
            "c::renamed::kid::fkid",
        ] {
            assert!(ids(&s).contains(&id), "{id} in {:?}", ids(&s));
        }
        // `#[cfg(windows)] mod gone;` is followed regardless; no file, so a warning
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert_eq!(s.warnings[0].kind, WarningKind::UnparsedFile);
        assert!(s.warnings[0].message.contains("gone"));
        let loc = s.warnings[0].loc.as_ref().unwrap();
        assert_eq!((loc.file.as_str(), loc.line), ("p/src/lib.rs", 5));
        assert_eq!(s.files["p/src/a/sub.rs"].module.0, "c::a::sub");
        assert!(s.orphans.is_empty(), "{:?}", s.orphans);
        assert_eq!(s.crates[0].lines, 5 + 2 + 1 + 1 + 1 + 1 + 2 + 1);
    }

    #[test]
    fn a_shared_file_belongs_to_the_first_target() {
        let r = dir("shared");
        put(&r, "p/src/lib.rs", "pub mod shared;\n");
        put(&r, "p/src/main.rs", "mod shared;\nfn main() {}\n");
        put(&r, "p/src/shared.rs", "pub fn f() {}\n");
        // listed bin first on purpose: the lib still wins
        let s = read_ws(
            r,
            vec![
                target("c[bin]", TargetKind::Bin, "p/src/main.rs"),
                target("c", TargetKind::Lib, "p/src/lib.rs"),
            ],
        );
        assert_eq!(s.files["p/src/shared.rs"].krate, "c");
        assert!(ids(&s).contains(&"c::shared::f"));
        assert!(!ids(&s).contains(&"c[bin]::shared::f"));
        let bin = s.crates.iter().find(|c| c.id == "c[bin]").unwrap();
        assert_eq!(bin.lines, 2);
        assert_eq!(bin.kind, TargetKind::Bin);
        assert_eq!(bin.package, "p");
    }

    #[test]
    fn test_targets_and_test_only_packages_mark_units() {
        let r = dir("tests");
        put(&r, "p/src/lib.rs", "pub fn f() {}\n");
        put(&r, "p/tests/t.rs", "fn g() {}\n");
        put(&r, "p/examples/e.rs", "fn h() {}\n");
        let s = read_ws(
            r,
            vec![
                target("c", TargetKind::Lib, "p/src/lib.rs"),
                target("t[test]", TargetKind::Test, "p/tests/t.rs"),
                target("e[example]", TargetKind::Example, "p/examples/e.rs"),
            ],
        );
        let t = |id: &str| {
            s.units
                .iter()
                .find(|u| u.unit.id.0 == id)
                .unwrap()
                .unit
                .test
        };
        assert!(!t("c::f") && t("t[test]::g") && t("e[example]::h"));
    }

    #[test]
    fn broken_files_warn_and_are_skipped() {
        let r = dir("broken");
        put(&r, "p/src/lib.rs", "mod bad;\nfn ok() {}\n");
        put(&r, "p/src/bad.rs", "fn (\n");
        let s = read_ws(r, vec![target("c", TargetKind::Lib, "p/src/lib.rs")]);
        assert!(ids(&s).contains(&"c::ok"));
        assert!(!s.files.contains_key("p/src/bad.rs"));
        assert_eq!(s.warnings.len(), 1);
        assert_eq!(s.warnings[0].kind, WarningKind::UnparsedFile);
        assert_eq!(s.warnings[0].loc.as_ref().unwrap().file, "p/src/bad.rs");
        // an unparsed file is still reached by the tree, so it is no orphan
        assert!(s.orphans.is_empty());
    }

    #[test]
    fn orphans_skip_target_build_script_hidden_and_nested_packages() {
        let r = dir("orphans");
        put(&r, "p/src/lib.rs", "");
        put(&r, "p/build.rs", "fn main() {}\n");
        put(&r, "p/src/stray.rs", "a\nb\nc\n");
        put(&r, "p/src/deeper/build.rs", "x\n");
        put(&r, "p/target/debug/gen.rs", "x\n");
        put(&r, "p/.hidden/h.rs", "x\n");
        put(&r, "p/vendored/Cargo.toml", "");
        put(&r, "p/vendored/src/v.rs", "x\n");
        put(&r, "p/notes.txt", "x\n");
        let s = read_ws(r, vec![target("c", TargetKind::Lib, "p/src/lib.rs")]);
        let found: Vec<_> = s
            .orphans
            .iter()
            .map(|o| (o.package.as_str(), o.file.as_str(), o.lines))
            .collect();
        assert_eq!(
            found,
            [
                ("p", "p/src/deeper/build.rs", 1),
                ("p", "p/src/stray.rs", 3)
            ]
        );
    }
}
