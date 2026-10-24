//! Maps a Cargo workspace into facts from rust-analyzer's SCIP export plus `syn`.

pub mod analysis;
pub mod config;
pub mod diff;
pub mod digest;
pub mod extract;
pub mod graph;
pub mod model;
pub mod page;
pub mod placement;
pub mod rust_analyzer;
pub mod scip;
pub mod structure;
pub mod summary;
pub mod symbol;
pub mod taint;
pub mod timings;
pub mod workspace;

/// Hash of this tool's own sources, set by `build.rs`.
pub const TOOL_HASH: &str = env!("TOPOLOGY_TOOL_HASH");
