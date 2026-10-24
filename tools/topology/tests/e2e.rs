#[path = "common/mod.rs"]
mod common;

use common::{fixture_config, fixture_ws};
use rust_topology::extract;
use rust_topology::rust_analyzer;
use rust_topology::workspace::Workspace;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

#[test]
fn missing_binary_is_explained() {
    let ws = Workspace::load(&fixture_ws(), &[]).unwrap();
    let out = scratch("missing");
    let err =
        rust_analyzer::run_with("/nonexistent".as_ref(), &ws, &fixture_config(), &out).unwrap_err();
    assert!(
        format!("{err:#}").contains("rustup component add rust-analyzer"),
        "{err:#}"
    );
}

#[test]
fn run_records_version_and_log() {
    let ws = Workspace::load(&fixture_ws(), &["apptesting".into()]).unwrap();
    let out = scratch("run");
    let run = rust_analyzer::run(&ws, &fixture_config(), &out).unwrap();
    assert!(run.version.starts_with("rust-analyzer"), "{}", run.version);
    assert!(run.index.is_file());
    assert!(out.join("rust-analyzer.log").is_file());
    #[derive(serde::Deserialize)]
    struct Cargo {
        features: String,
    }
    #[derive(serde::Deserialize)]
    struct RaConfig {
        cargo: Cargo,
    }
    let text = std::fs::read_to_string(out.join("ra-config.json")).unwrap();
    let cfg: RaConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(cfg.cargo.features, "all");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn extract_twice_is_byte_identical() {
    let scip = common::fixture_index_path();
    let mut outs = Vec::new();
    for n in ["a", "b"] {
        let out = scratch(&format!("twice-{n}"));
        extract::run(&fixture_ws(), None, &out, Some(scip)).unwrap();
        outs.push(out);
    }
    for f in ["topology.json", "digest.json"] {
        let a = std::fs::read(outs[0].join(f)).unwrap();
        let b = std::fs::read(outs[1].join(f)).unwrap();
        assert!(!a.is_empty() && a.ends_with(b"\n"));
        assert!(a == b, "{f} differs between runs");
    }
    for o in outs {
        let _ = std::fs::remove_dir_all(o);
    }
}

#[test]
fn extract_from_other_cwd_has_relative_paths() {
    let scip = common::fixture_index_path();
    let out = scratch("cwd");
    let cwd = scratch("cwd-dir");
    std::fs::create_dir_all(&cwd).unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_topology"))
        .current_dir(&cwd)
        .arg("--workspace")
        .arg(fixture_ws())
        .arg("--out")
        .arg(&out)
        .arg("extract")
        .arg("--scip")
        .arg(scip)
        .status()
        .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(out.join("topology.json")).unwrap();
    let t: rust_topology::model::Topology = serde_json::from_str(&text).unwrap();
    assert!(!t.units.is_empty());
    for u in &t.units {
        assert!(!u.file.starts_with('/'), "{}", u.file);
        assert!(u.file.contains('/'), "{}", u.file);
    }
    assert!(t.units.iter().any(|u| u.file.starts_with("appcore/")));
    let _ = std::fs::remove_dir_all(out);
    let _ = std::fs::remove_dir_all(cwd);
}

#[test]
fn summary_mentions_headline() {
    let scip = common::fixture_index_path();
    let out = scratch("summary");
    let cfg = fixture_config();
    let t = extract::run(&fixture_ws(), None, &out, Some(scip)).unwrap();
    let s = rust_topology::summary::render(&t, &cfg);
    assert!(s.contains("discord"), "{s}");
    assert!(s.contains("appcore::rules::wrapper"), "{s}");
    assert!(s.contains("orphans: 1 files, 1 lines"), "{s}");
    let _ = std::fs::remove_dir_all(out);
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    for e in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_entry(|e| e.file_name() != "target")
    {
        let e = e.unwrap();
        let dest = to.join(e.path().strip_prefix(from).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&dest).unwrap();
        } else {
            std::fs::copy(e.path(), &dest).unwrap();
        }
    }
}

#[test]
fn moved_function_reads_as_move() {
    use rust_topology::diff;
    use rust_topology::model::UnitId;
    let moved = scratch("moved-ws");
    copy_tree(&fixture_ws(), &moved);
    let src = "pub fn score(t: &Track) -> u32 {\n    t.id * 2\n}\n";
    let rules = moved.join("appcore/src/rules.rs");
    let text = std::fs::read_to_string(&rules).unwrap();
    assert!(text.contains(src));
    std::fs::write(&rules, text.replace(&format!("{src}\n"), "")).unwrap();
    let texts = moved.join("appcore/src/text.rs");
    let mut t = std::fs::read_to_string(&texts).unwrap();
    t.push_str(&format!("\nuse crate::rules::Track;\n\n{src}"));
    std::fs::write(&texts, t).unwrap();
    let handlers = moved.join("appcore/src/handlers.rs");
    let h = std::fs::read_to_string(&handlers).unwrap();
    assert!(h.contains("crate::rules::score"));
    std::fs::write(
        &handlers,
        h.replace("crate::rules::score", "crate::text::score"),
    )
    .unwrap();

    let out_a = scratch("moved-a");
    let out_b = scratch("moved-b");
    extract::run(&fixture_ws(), None, &out_a, None).unwrap();
    extract::run(&moved, None, &out_b, None).unwrap();
    let a = diff::load(&out_a.join("topology.json")).unwrap();
    let b = diff::load(&out_b.join("digest.json")).unwrap();
    let r = diff::compare(&a, &b);
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    let want = diff::Move {
        from: UnitId("appcore::rules::score".into()),
        to: UnitId("appcore::text::score".into()),
        changed: false,
    };
    assert!(r.moved.contains(&want), "{:?}", r.moved);
    assert!(
        r.changed.contains(&UnitId("appcore::handlers::cmd".into())),
        "{:?}",
        r.changed
    );
    assert!(!r.added.contains(&want.to) && !r.removed.contains(&want.from));
    for o in [moved, out_a, out_b] {
        let _ = std::fs::remove_dir_all(o);
    }
}

#[test]
fn load_reads_both_formats_and_explains_garbage() {
    use rust_topology::diff;
    let out = scratch("load");
    extract::run(
        &fixture_ws(),
        None,
        &out,
        Some(common::fixture_index_path()),
    )
    .unwrap();
    let from_topology = diff::load(&out.join("topology.json")).unwrap();
    let from_digest = diff::load(&out.join("digest.json")).unwrap();
    assert_eq!(from_topology, from_digest);
    assert!(!from_digest.units.is_empty());
    let bad = out.join("garbage.json");
    std::fs::write(&bad, "{}").unwrap();
    let msg = format!("{:#}", diff::load(&bad).unwrap_err());
    assert!(msg.contains("garbage.json"), "{msg}");
    assert!(msg.contains("missing field"), "{msg}");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn timings_on_fixture() {
    use rust_topology::timings::{self, Timings};
    let ws = Workspace::load(&fixture_ws(), &["apptesting".into()]).unwrap();
    let mut cfg = fixture_config();
    cfg.timings.rebuild_package = Some("appcore".into());
    cfg.timings.profile = "dev".into();
    cfg.timings.through_package = Some("appbin".into());
    let out = scratch("timings");
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("timings-target");
    let started = std::time::Instant::now();
    let t = timings::run(&ws, &cfg, &out, 1, Some(&target)).unwrap();
    eprintln!("timings_on_fixture took {:?}", started.elapsed());
    for p in &ws.packages {
        for tg in p
            .targets
            .iter()
            .filter(|t| matches!(t.kind, rust_topology::model::TargetKind::Lib))
        {
            let c = t
                .crates
                .iter()
                .find(|c| c.package == p.name && c.target == tg.crate_id)
                .unwrap_or_else(|| panic!("no timing for {}", tg.crate_id));
            assert_eq!(c.samples.len(), 1);
            assert!(c.samples.iter().all(|s| *s > 0.0), "{c:?}");
        }
    }
    let rb = t.rebuild.as_ref().expect("rebuild timing");
    assert_eq!(
        (rb.package.as_str(), rb.through.as_str()),
        ("appcore", "appbin")
    );
    assert_eq!(rb.samples.len(), 1);
    assert!(rb.samples.iter().all(|s| *s > 0.0));
    let back: Timings =
        serde_json::from_str(&std::fs::read_to_string(out.join("timings.json")).unwrap()).unwrap();
    assert_eq!(back, t);
}
