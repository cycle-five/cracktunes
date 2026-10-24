use sha2::{Digest, Sha256};
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src");
    let mut files: Vec<_> = ["src", "src/page/assets"]
        .iter()
        .filter(|d| Path::new(d).is_dir())
        .flat_map(|d| walkdir::WalkDir::new(d).sort_by_file_name())
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files.dedup();
    let mut h = Sha256::new();
    for f in files {
        h.update(f.to_string_lossy().replace('\\', "/").as_bytes());
        h.update(std::fs::read(&f).expect("read source file"));
    }
    println!("cargo:rustc-env=TOPOLOGY_TOOL_HASH={:x}", h.finalize());
}
