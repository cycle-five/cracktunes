fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make sure tarpaulin is included in the build
    println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");
    vergen::EmitBuilder::builder()
        .all_build()
        .all_git()
        .emit()?;
    Ok(())
}
