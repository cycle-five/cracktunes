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
    use std::sync::OnceLock;
    static INDEX: OnceLock<rust_topology::scip::ScipIndex> = OnceLock::new();
    INDEX.get_or_init(|| rust_topology::scip::read(fixture_index_path()).unwrap())
}

/// Path of the cached SCIP file behind [`fixture_index`].
pub fn fixture_index_path() -> &'static std::path::Path {
    use sha2::{Digest, Sha256};
    use std::sync::OnceLock;
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
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
        // A newer rust-analyzer can index differently: it is part of the key.
        let binary = rust_topology::rust_analyzer::resolve_binary(std::env::var_os(
            "TOPOLOGY_RUST_ANALYZER",
        ));
        let version = std::process::Command::new(binary)
            .arg("--version")
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default();
        h.update(&version);
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
        cached
    })
}

/// The fixture workspace's syn-derived structure, read once per test binary.
pub fn fixture_structure() -> &'static rust_topology::structure::Structure {
    use std::sync::OnceLock;
    static S: OnceLock<rust_topology::structure::Structure> = OnceLock::new();
    S.get_or_init(|| {
        let ws = rust_topology::workspace::Workspace::load(&fixture_ws(), &["apptesting".into()])
            .unwrap();
        rust_topology::structure::read(&ws, &fixture_config()).unwrap()
    })
}

/// The fixture's structure, its placed occurrences and the aggregated graph.
/// Packages the config puts in a group count as external, as they do for the
/// real workspace's dependencies (`fakecord` is a path dependency here).
pub fn fixture_placed() -> &'static (
    rust_topology::structure::Structure,
    rust_topology::placement::Placed,
    rust_topology::graph::Graph,
) {
    use std::sync::OnceLock;
    static P: OnceLock<(
        rust_topology::structure::Structure,
        rust_topology::placement::Placed,
        rust_topology::graph::Graph,
    )> = OnceLock::new();
    P.get_or_init(|| {
        let ws = rust_topology::workspace::Workspace::load(&fixture_ws(), &["apptesting".into()])
            .unwrap();
        let cfg = fixture_config();
        let structure = fixture_structure().clone();
        let external = ws
            .packages
            .iter()
            .filter(|p| cfg.group_of(&p.name).is_some())
            .map(|p| p.name.clone())
            .collect();
        let placed =
            rust_topology::placement::place_with(&structure, fixture_index(), &ws, &external);
        let graph = rust_topology::graph::aggregate(&placed);
        (structure, placed, graph)
    })
}
