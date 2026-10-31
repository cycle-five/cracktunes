//! Runs `rust-analyzer scip` over a workspace.

use crate::config::Config;
use crate::workspace::Workspace;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct RaRun {
    pub index: PathBuf,
    pub version: String,
    pub error_lines: u32,
}

#[derive(Serialize)]
struct CargoSection<'a> {
    features: &'a str,
}

#[derive(Serialize)]
struct RaConfigFile<'a> {
    cargo: CargoSection<'a>,
}

pub fn resolve_binary(env: Option<OsString>) -> OsString {
    match env {
        Some(v) if !v.is_empty() => v,
        _ => OsString::from("rust-analyzer"),
    }
}

pub fn run(ws: &Workspace, cfg: &Config, out: &Path) -> Result<RaRun> {
    run_with(
        &resolve_binary(std::env::var_os("TOPOLOGY_RUST_ANALYZER")),
        ws,
        cfg,
        out,
    )
}

pub fn run_with(binary: &OsStr, ws: &Workspace, cfg: &Config, out: &Path) -> Result<RaRun> {
    const HINT: &str =
        "install it with `rustup component add rust-analyzer` or set TOPOLOGY_RUST_ANALYZER";
    // Every command runs from the workspace root, so every path it gets is absolute.
    let root = ws
        .root
        .canonicalize()
        .with_context(|| format!("workspace {}", ws.root.display()))?;
    let version_out = Command::new(binary)
        .arg("--version")
        .current_dir(&root)
        .output()
        .with_context(|| {
            format!(
                "cannot run rust-analyzer ({}): {HINT}",
                binary.to_string_lossy()
            )
        })?;
    if !version_out.status.success() {
        bail!(
            "`{} --version` failed ({}): {HINT}",
            binary.to_string_lossy(),
            version_out.status
        );
    }
    let version = String::from_utf8_lossy(&version_out.stdout)
        .trim()
        .to_string();

    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let out = std::path::absolute(out)?;

    let config_path = out.join("ra-config.json");
    let ra_config = RaConfigFile {
        cargo: CargoSection {
            features: &cfg.rust_analyzer.features,
        },
    };
    std::fs::write(&config_path, serde_json::to_string_pretty(&ra_config)?)?;

    let index = out.join("index.scip");
    match std::fs::remove_file(&index) {
        Ok(()) => {},
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => return Err(e).with_context(|| format!("removing stale {}", index.display())),
    }
    let log_path = out.join("rust-analyzer.log");
    let log = std::fs::File::create(&log_path)?;

    let status = Command::new(binary)
        .arg("scip")
        .arg(&root)
        .arg("--config-path")
        .arg(&config_path)
        .arg("--output")
        .arg(&index)
        .current_dir(&root)
        .envs(&cfg.rust_analyzer.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .status()
        .with_context(|| format!("running {}", binary.to_string_lossy()))?;
    if !status.success() {
        bail!(
            "rust-analyzer scip failed ({status}); see {}",
            log_path.display()
        );
    }
    if !index.is_file() {
        bail!(
            "rust-analyzer scip wrote no index at {}; see {}",
            index.display(),
            log_path.display()
        );
    }
    let error_lines = count_error_lines(&String::from_utf8_lossy(&std::fs::read(&log_path)?));
    Ok(RaRun {
        index,
        version,
        error_lines,
    })
}

fn count_error_lines(log: &str) -> u32 {
    let n = log.lines().filter(|l| l.contains(" ERROR ")).count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_defaults_to_path_lookup() {
        assert_eq!(resolve_binary(None), OsString::from("rust-analyzer"));
        assert_eq!(
            resolve_binary(Some("/x/ra".into())),
            OsString::from("/x/ra")
        );
        assert_eq!(
            resolve_binary(Some("".into())),
            OsString::from("rust-analyzer")
        );
    }

    #[test]
    fn counts_only_error_level_lines() {
        let log = "2026 INFO foo\n2026-10-24T00:00:00Z ERROR bar\nERROR at start has no leading space\n x ERROR y\n";
        assert_eq!(count_error_lines(log), 2);
    }
}
