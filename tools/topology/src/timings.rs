//! `timings`: per-crate compile time from `cargo build --timings`, plus the
//! wall-clock of a rebuild after touching one package.

use crate::config::Config;
use crate::model::TargetKind;
use crate::workspace::Workspace;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// One unit of cargo's `UNIT_DATA`; unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitTime {
    pub name: String,
    pub version: String,
    pub mode: String,
    pub target: String,
    pub start: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrateTiming {
    pub package: String,
    pub target: String,
    pub median_secs: f64,
    pub samples: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RebuildTiming {
    pub package: String,
    pub through: String,
    pub median_secs: f64,
    pub samples: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timings {
    pub rustc: String,
    pub profile: String,
    pub jobs: u32,
    pub host: String,
    pub repeats: u32,
    pub crates: Vec<CrateTiming>,
    pub rebuild: Option<RebuildTiming>,
}

const MARKER: &str = "const UNIT_DATA = ";

/// The JSON array after `const UNIT_DATA = ` up to its terminating `];`.
pub fn parse_unit_data(html: &str) -> Result<Vec<UnitTime>> {
    let start = html
        .find(MARKER)
        .ok_or_else(|| anyhow!("no `{MARKER}` in the timing report"))?
        + MARKER.len();
    let rest = &html[start..];
    let end = rest
        .find("];")
        .ok_or_else(|| anyhow!("UNIT_DATA has no terminating `];`"))?;
    serde_json::from_str::<Vec<UnitTime>>(&rest[..=end]).context("parsing UNIT_DATA")
}

pub fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Summed duration of the units that compile one workspace target. Build-script
/// runs are excluded; a lib's unit has an empty `target`, any other target's
/// reads ` <name> "<kind>"`.
fn unit_secs(units: &[UnitTime], package: &str, target_name: &str, lib: bool) -> Option<f64> {
    let marker = format!("{target_name} \"");
    let mut found = false;
    let mut sum = 0.0;
    for u in units {
        if u.name != package || u.mode == "run-custom-build" {
            continue;
        }
        let hit = if lib {
            u.target.is_empty()
        } else {
            u.target.trim_start().starts_with(&marker)
        };
        if hit {
            found = true;
            sum += u.duration;
        }
    }
    found.then_some(sum)
}

fn default_target_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/crack-topology/timings-target"))
}

fn touch(path: &Path) -> Result<()> {
    let f = std::fs::File::options()
        .append(true)
        .open(path)
        .with_context(|| format!("touching {}", path.display()))?;
    f.set_modified(std::time::SystemTime::now())
        .with_context(|| format!("touching {}", path.display()))
}

fn cargo(ws: &Workspace, target_dir: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(&ws.root)
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_INCREMENTAL", "0")
        .status()
        .context("running cargo")?;
    if !status.success() {
        bail!("`cargo {}` failed: {status}", args.join(" "));
    }
    Ok(())
}

fn rustc_vv() -> Result<String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .context("running rustc -vV")?;
    if !out.status.success() {
        bail!(
            "`rustc -vV` failed: {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Measures every workspace target (median of `repeats` touched rebuilds) and,
/// when configured, a core rebuild. Both a `--workspace` build and a
/// `-p <through_package>` build run untimed first, because feature unification
/// can differ between them and the first timed sample must not compile deps.
pub fn run(
    ws: &Workspace,
    cfg: &Config,
    out: &Path,
    repeats: u32,
    target_dir: Option<&Path>,
) -> Result<Timings> {
    let t = &cfg.timings;
    let target_dir = match (target_dir, &t.target_dir) {
        (Some(d), _) => d.to_path_buf(),
        (None, Some(d)) => d.clone(),
        (None, None) => default_target_dir()?,
    };
    let profile = t.profile.as_str();
    let verbose = rustc_vv()?;
    let rustc = verbose
        .lines()
        .next()
        .ok_or_else(|| anyhow!("`rustc -vV` printed nothing"))?
        .to_string();
    let host = verbose
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or_else(|| anyhow!("`rustc -vV` has no host line"))?
        .to_string();
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);

    // Warm build, so the repeats measure recompiling workspace code only.
    cargo(
        ws,
        &target_dir,
        &[
            "build",
            "--workspace",
            "--all-features",
            "--profile",
            profile,
        ],
    )?;

    let report = target_dir.join("cargo-timings/cargo-timing.html");
    let mut samples: std::collections::BTreeMap<(String, String), Vec<f64>> = Default::default();
    for _ in 0..repeats {
        for p in &ws.packages {
            for tg in &p.targets {
                touch(&ws.root.join(&tg.root_file))?;
            }
        }
        cargo(
            ws,
            &target_dir,
            &[
                "build",
                "--workspace",
                "--all-features",
                "--profile",
                profile,
                "--timings",
            ],
        )?;
        let html = std::fs::read_to_string(&report)
            .with_context(|| format!("reading {}", report.display()))?;
        let units = parse_unit_data(&html)?;
        for p in &ws.packages {
            for tg in &p.targets {
                let lib = matches!(tg.kind, TargetKind::Lib | TargetKind::ProcMacro);
                if let Some(secs) = unit_secs(&units, &p.name, &tg.name, lib) {
                    samples
                        .entry((p.name.clone(), tg.crate_id.clone()))
                        .or_default()
                        .push(secs);
                }
            }
        }
    }
    let crates = samples
        .into_iter()
        .map(|((package, target), samples)| CrateTiming {
            package,
            target,
            median_secs: median(&samples),
            samples,
        })
        .collect();

    let rebuild = match (&t.rebuild_package, &t.through_package) {
        (Some(pkg), Some(through)) => {
            let root = ws
                .packages
                .iter()
                .find(|p| p.name == *pkg)
                .and_then(|p| {
                    p.targets
                        .iter()
                        .find(|t| matches!(t.kind, TargetKind::Lib | TargetKind::ProcMacro))
                })
                .map(|t| ws.root.join(&t.root_file))
                .ok_or_else(|| anyhow!("rebuild_package `{pkg}` has no lib target"))?;
            // Untimed warm-up: `-p` may unify features differently from `--workspace`.
            cargo(
                ws,
                &target_dir,
                &[
                    "build",
                    "-p",
                    through,
                    "--all-features",
                    "--profile",
                    profile,
                ],
            )?;
            let mut samples = Vec::new();
            for _ in 0..repeats {
                touch(&root)?;
                let started = Instant::now();
                cargo(
                    ws,
                    &target_dir,
                    &[
                        "build",
                        "-p",
                        through,
                        "--all-features",
                        "--profile",
                        profile,
                    ],
                )?;
                samples.push(started.elapsed().as_secs_f64());
            }
            Some(RebuildTiming {
                package: pkg.clone(),
                through: through.clone(),
                median_secs: median(&samples),
                samples,
            })
        },
        _ => None,
    };

    let timings = Timings {
        rustc,
        profile: profile.to_string(),
        jobs,
        host,
        repeats,
        crates,
        rebuild,
    };
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    std::fs::write(
        out.join("timings.json"),
        serde_json::to_string_pretty(&timings)?,
    )?;
    Ok(timings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/cargo-timing-sample.html");

    #[test]
    fn parses_sample() {
        let units = parse_unit_data(SAMPLE).unwrap();
        assert!(units.len() >= 2);
        let core = units.iter().find(|u| u.name == "appcore").unwrap();
        assert!(core.duration > 0.0);
        assert_eq!(core.version, "0.1.0");
    }

    #[test]
    fn median_odd_even() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[7.0]), 7.0);
    }

    #[test]
    fn missing_unit_data_is_an_error() {
        assert!(parse_unit_data("<html></html>").is_err());
        assert!(parse_unit_data("const UNIT_DATA = [ {").is_err());
    }

    #[test]
    fn unit_secs_sums_matching_units_only() {
        let u = |name: &str, mode: &str, target: &str, d: f64| UnitTime {
            name: name.into(),
            version: "0.1.0".into(),
            mode: mode.into(),
            target: target.into(),
            start: 0.0,
            duration: d,
        };
        let units = vec![
            u("a", "todo", "", 1.0),
            u("a", "todo", "", 2.0),
            u("a", "run-custom-build", "", 40.0),
            u("a", "todo", " a \"bin\"", 5.0),
            u("b", "todo", "", 100.0),
        ];
        assert_eq!(unit_secs(&units, "a", "a", true), Some(3.0));
        assert_eq!(unit_secs(&units, "a", "a", false), Some(5.0));
        assert_eq!(unit_secs(&units, "a", "zzz", false), None);
        assert_eq!(unit_secs(&units, "c", "c", true), None);
    }
}
