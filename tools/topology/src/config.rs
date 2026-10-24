//! `topology.toml`: every field optional.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RaConfig {
    pub features: String,
    pub env: BTreeMap<String, String>,
}

impl Default for RaConfig {
    fn default() -> Self {
        RaConfig {
            features: "all".into(),
            env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TimingsConfig {
    pub rebuild_package: Option<String>,
    pub through_package: Option<String>,
    pub repeats: u32,
    pub profile: String,
    pub target_dir: Option<PathBuf>,
}

impl Default for TimingsConfig {
    fn default() -> Self {
        TimingsConfig {
            rebuild_package: None,
            through_package: None,
            repeats: 3,
            profile: "dev".into(),
            target_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Empty means "use the workspace directory name".
    pub name: String,
    /// Group name -> package names; a trailing `*` is a prefix glob.
    pub groups: BTreeMap<String, Vec<String>>,
    pub headline_group: String,
    pub test_crates: Vec<String>,
    pub wide_struct_min_fields: usize,
    pub near_clean_max_cuts: u32,
    pub rust_analyzer: RaConfig,
    pub timings: TimingsConfig,
    #[serde(skip, default = "default_hash")]
    hash: String,
}

fn default_hash() -> String {
    "default".into()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            name: String::new(),
            groups: BTreeMap::new(),
            headline_group: "discord".into(),
            test_crates: Vec::new(),
            wide_struct_min_fields: 10,
            near_clean_max_cuts: 2,
            rust_analyzer: RaConfig::default(),
            timings: TimingsConfig::default(),
            hash: default_hash(),
        }
    }
}

fn matches(pattern: &str, package: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => package.starts_with(prefix),
        None => pattern == package,
    }
}

/// Can some package name match both patterns?
fn overlaps(a: &str, b: &str) -> bool {
    match (a.strip_suffix('*'), b.strip_suffix('*')) {
        (None, None) => a == b,
        (Some(_), None) => matches(a, b),
        (None, Some(_)) => matches(b, a),
        (Some(pa), Some(pb)) => pa.starts_with(pb) || pb.starts_with(pa),
    }
}

impl Config {
    /// Defaults when `path` is `None`; otherwise the TOML file at `path`.
    pub fn load(path: Option<&Path>) -> Result<Config> {
        match path {
            None => Ok(Config::default()),
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("reading config {}", p.display()))?;
                Config::from_toml_str(&text).with_context(|| format!("in config {}", p.display()))
            },
        }
    }

    pub fn from_toml_str(s: &str) -> Result<Config> {
        let mut c: Config = toml::from_str(s).context("parsing config")?;
        c.hash = format!("{:x}", Sha256::digest(s.as_bytes()));
        c.check_groups()?;
        Ok(c)
    }

    fn check_groups(&self) -> Result<()> {
        let pats: Vec<(&str, &str)> = self
            .groups
            .iter()
            .flat_map(|(g, ps)| ps.iter().map(move |p| (g.as_str(), p.as_str())))
            .collect();
        for (i, (ga, pa)) in pats.iter().enumerate() {
            for (gb, pb) in &pats[i + 1..] {
                if ga != gb && overlaps(pa, pb) {
                    bail!("group patterns overlap: `{pa}` in `{ga}` and `{pb}` in `{gb}`");
                }
            }
        }
        Ok(())
    }

    pub fn group_of(&self, package: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|(_, ps)| ps.iter().any(|p| matches(p, package)))
            .map(|(g, _)| g.as_str())
    }

    /// sha256 hex of the config text, `"default"` when there was no file.
    pub fn hash(&self) -> String {
        self.hash.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_without_file() {
        let c = Config::load(None).unwrap();
        assert_eq!(c.headline_group, "discord");
        assert_eq!(c.wide_struct_min_fields, 10);
        assert_eq!(c.near_clean_max_cuts, 2);
        assert_eq!(c.rust_analyzer.features, "all");
        assert_eq!(c.timings.repeats, 3);
        assert_eq!(c.timings.profile, "dev");
        assert_eq!(c.hash(), "default");
    }

    #[test]
    fn glob_and_exact_group_match() {
        let c = Config::from_toml_str("[groups]\ndatabase = [\"sqlx\", \"sqlx-*\"]\n").unwrap();
        assert_eq!(c.group_of("sqlx-core"), Some("database"));
        assert_eq!(c.group_of("sqlx"), Some("database"));
        assert_eq!(c.group_of("tokio"), None);
        assert_ne!(c.hash(), "default");
    }

    #[test]
    fn package_in_two_groups_is_an_error() {
        let err = Config::from_toml_str("[groups]\na = [\"x\"]\nb = [\"x*\"]\n").unwrap_err();
        assert!(err.to_string().contains('x'), "{err}");
    }

    #[test]
    fn overlap_cases() {
        assert!(overlaps("x", "x"));
        assert!(!overlaps("x", "y"));
        assert!(overlaps("x*", "xy"));
        assert!(overlaps("xy", "x*"));
        assert!(!overlaps("x*", "y"));
        assert!(overlaps("ab*", "a*"));
        assert!(overlaps("a*", "ab*"));
        assert!(!overlaps("a*", "b*"));
    }

    #[test]
    fn same_group_may_repeat_a_package() {
        assert!(Config::from_toml_str("[groups]\na = [\"x\", \"x*\"]\n").is_ok());
    }

    #[test]
    fn unknown_field_is_rejected() {
        assert!(Config::from_toml_str("bogus = 1\n").is_err());
    }

    #[test]
    fn cracktunes_config_parses() {
        let c = Config::load(Some(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/topology.toml"
        ))))
        .unwrap();
        assert_eq!(c.group_of("poise_macros"), Some("discord"));
        assert_eq!(c.group_of("sqlx-postgres"), Some("database"));
        assert_eq!(c.rust_analyzer.env["SQLX_OFFLINE"], "true");
        assert_eq!(c.timings.through_package.as_deref(), Some("cracktunes"));
    }
}
