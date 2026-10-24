//! Shared by the `fixture` and `e2e` test binaries.
#![allow(dead_code)]

use rust_topology::config::Config;
use std::path::PathBuf;

pub fn fixture_ws() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ws")
}

pub fn fixture_config() -> Config {
    let text = std::fs::read_to_string(fixture_ws().join("topology.toml")).unwrap();
    Config::from_toml_str(&text).unwrap()
}

/// The fixture's SCIP index. rust-analyzer runs once per test binary and the
/// result is cached under `CARGO_TARGET_TMPDIR`, keyed by a hash of the fixture
/// sources (everything under `tests/fixtures/ws` except `target/`).
pub fn fixture_index() -> &'static rust_topology::scip::ScipIndex {
    use sha2::{Digest, Sha256};
    use std::sync::OnceLock;
    static INDEX: OnceLock<rust_topology::scip::ScipIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let root = fixture_ws();
        let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_entry(|e| e.file_name() != "target")
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.into_path())
            .collect();
        files.sort();
        let mut h = Sha256::new();
        for f in &files {
            h.update(f.strip_prefix(&root).unwrap().to_string_lossy().as_bytes());
            h.update([0]);
            h.update(std::fs::read(f).unwrap());
            h.update([0]);
        }
        let hash: String = h
            .finalize()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect();
        let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let cached = tmp.join(format!("fixture-{hash}.scip"));
        if !cached.exists() {
            let ws =
                rust_topology::workspace::Workspace::load(&root, &["apptesting".into()]).unwrap();
            let out = tmp.join(format!("fixture-run-{hash}-{}", std::process::id()));
            let started = std::time::Instant::now();
            let run = rust_topology::rust_analyzer::run(&ws, &fixture_config(), &out).unwrap();
            eprintln!("fixture index: rust-analyzer took {:?}", started.elapsed());
            let tmp_name = tmp.join(format!("fixture-{hash}.scip.{}", std::process::id()));
            std::fs::copy(&run.index, &tmp_name).unwrap();
            std::fs::rename(&tmp_name, &cached).unwrap();
            let _ = std::fs::remove_dir_all(&out);
        }
        rust_topology::scip::read(&cached).unwrap()
    })
}
