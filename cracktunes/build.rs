use std::process::Command;
fn main() {
    // make sure tarpaulin is included in the build.
    println!("cargo::rustc-check-cfg=cfg(tarpaulin_include)");
    // git hash for the build version.
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
