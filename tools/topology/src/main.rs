use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use rust_topology::{diff, extract, timings};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "topology", version, about = "Map a Cargo workspace into facts")]
struct Cli {
    /// Workspace directory.
    #[arg(long, global = true, default_value = ".")]
    workspace: PathBuf,
    /// Config file (default: `<workspace>/topology.toml` if present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Output directory (default: `<workspace>/topology-out`).
    #[arg(long, global = true)]
    out: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Index the workspace and write topology.json and digest.json.
    Extract {
        /// Use an existing SCIP index instead of running rust-analyzer.
        #[arg(long)]
        scip: Option<PathBuf>,
        /// Print a one-screen summary.
        #[arg(long)]
        summary: bool,
    },
    /// Compare two digests.
    Diff {
        a: PathBuf,
        b: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Render the HTML page.
    Page {
        #[arg(long = "in")]
        input: Option<PathBuf>,
        #[arg(long)]
        timings: Option<PathBuf>,
    },
    /// Measure rebuild timings.
    Timings {
        #[arg(long)]
        repeats: Option<u32>,
        #[arg(long)]
        target_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let out = cli
        .out
        .clone()
        .unwrap_or_else(|| cli.workspace.join("topology-out"));
    match cli.command {
        Command::Extract { scip, summary } => {
            let t = extract::run(&cli.workspace, cli.config.as_deref(), &out, scip.as_deref())?;
            if summary {
                let cfg = extract::load_config(&cli.workspace, cli.config.as_deref())?;
                print!("{}", rust_topology::summary::render(&t, &cfg));
            }
            Ok(())
        },
        Command::Diff { a, b, json } => {
            let report = diff::compare(&diff::load(&a)?, &diff::load(&b)?);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", diff::render(&report));
            }
            Ok(())
        },
        Command::Timings {
            repeats,
            target_dir,
        } => {
            let cfg = extract::load_config(&cli.workspace, cli.config.as_deref())?;
            let ws = rust_topology::workspace::Workspace::load(&cli.workspace, &cfg.test_crates)?;
            let repeats = repeats.unwrap_or(cfg.timings.repeats);
            let t = timings::run(&ws, &cfg, &out, repeats, target_dir.as_deref())?;
            println!(
                "timings: {} crates, wrote {}",
                t.crates.len(),
                out.join("timings.json").display()
            );
            Ok(())
        },
        Command::Page { .. } => {
            bail!("not implemented yet")
        },
    }
}
