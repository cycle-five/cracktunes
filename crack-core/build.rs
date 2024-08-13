use vergen_git2 as vergen;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // make sure tarpaulin is included in the build
    println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");
    // NOTE: This will output everything, and requires all features enabled.
    // NOTE: See the specific builder documentation for configuration options.
    let build = vergen::BuildBuilder::all_build()?;
    let cargo = vergen::CargoBuilder::all_cargo()?;
    let git2 = vergen::Git2Builder::all_git()?;
    let rustc = vergen::RustcBuilder::all_rustc()?;
    let si = vergen::SysinfoBuilder::all_sysinfo()?;

    vergen::Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&cargo)?
        .add_instructions(&git2)?
        .add_instructions(&rustc)?
        .add_instructions(&si)?
        .emit()?;
    Ok(())
}
