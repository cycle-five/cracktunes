use crate::{CrackTrackClient, Error};
use clap::{Parser, Subcommand};
use serenity::model::id::GuildId;
/// Args struct for the CLI.
#[derive(Parser, Debug)]
#[command(
    version = "1.0",
    author = "Cycle Five <cycle.five@proton.me>",
    about = "Implementation of the event handler for CrackTunes as a separate binary."
)]
struct Cli {
    /// The command to run
    #[command(subcommand)]
    command: Commands,
}

/// The command to run.
#[derive(Subcommand, Debug)]
enum Commands {
    Run {
        /// The query to get suggestions for.
        query: String,
    },
    Test {
        /// The query to get suggestions for, second method.
        query: String,
    },
    Help,
}

/// Match the CLI command and run the appropriate function.
#[tracing::instrument]
async fn match_cli(cli: Cli) -> Result<(), Error> {
    let guild = GuildId::new(1);
    let client = Box::leak(Box::new(CrackTrackClient::new()));
    match cli.command {
        Commands::Run { query } => {
            tracing::info!("run");
        },
        Commands::Test { query } => {
            tracing::info!("test");
        },
        Commands::Help => {
            println!("{}", cli.help_message());
            tracing::info!()
        },
    }
    Ok(())
}
