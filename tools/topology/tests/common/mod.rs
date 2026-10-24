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
