use vergen_gitcl as vergen;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make sure tarpaulin is included in the build.
    println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");
    // NOTE: This will output everything, and requires all features enabled.
    // NOTE: See the specific builder documentation for configuration options.
    let build = vergen::BuildBuilder::all_build()?;
    let git = vergen::GitclBuilder::all_git()?;

    vergen::Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&git)?
        .emit()?;
    Ok(())
}
