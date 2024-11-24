use vergen_gitcl as vergen;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make sure tarpaulin is included in the build.
    // println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");
    // NOTE: This will output everything, and requires all features enabled.
    // NOTE: See the specific builder documentation for configuration options.
    let build = vergen::BuildBuilder::all_build()?;
    let git = vergen::GitclBuilder::all_git()?;
    // let cargo = vergen::CargoBuilder::all_cargo()?;
    // let rustc = vergen::RustcBuilder::all_rustc()?;
    // let si = vergen::SysinfoBuilder::all_sysinfo()?;

    vergen::Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&git)?
        // .add_instructions(&rustc)?
        // .add_instructions(&si)?
        .emit()?;
    Ok(())
}
