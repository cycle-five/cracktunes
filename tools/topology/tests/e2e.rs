#[path = "common/mod.rs"]
mod common;

use common::{fixture_config, fixture_ws};
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
