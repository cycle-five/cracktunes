// use std::process::Command;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make sure tarpaulin is included in the build.
    println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");
    // Git hash of the build.
    vergen::EmitBuilder::builder()
        .all_build()
        .all_git()
        .emit()?;
    Ok(())
}
