//! Build script: stamps build- and git-derived constants into the binary.
//!
//! `vergen-gitcl` 10 dropped the fallible `*Builder::all_*()` constructors in
//! favour of infallible `Build::all_build()` / `Gitcl::all_git()`.
use vergen_gitcl::{Build, Emitter, Gitcl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(tarpaulin_include)");

    // NOTE: See the specific builder documentation for configuration options.
    let build = Build::all_build();
    let git = Gitcl::all_git();

    // `emit` is idempotent by default: when the source tree has no `.git`
    // (release tarballs, `docker build` from a COPY'd context) the git
    // variables fall back to placeholders instead of failing the build.
    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&git)?
        .emit()?;
    Ok(())
}
