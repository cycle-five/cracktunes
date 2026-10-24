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
