// pub mod queue;
// pub use queue::*;
// pub mod resolve;
// pub use resolve::*;

//------------------------------------
// crack_types imports
//------------------------------------
use crack_types::CrackTrackClient;
use crack_types::SpotifyTrackTrait;
use crack_types::TrackResolveError;
use crack_types::{parse_url, video_info_to_aux_metadata};
use crack_types::{Error, QueryType, SearchResult};
use crack_types::{CREATING, NEW_FAILED, REQ_CLIENT_STR, YOUTUBE_CLIENT_STR};
//------------------------------------
// External library imports
//------------------------------------
use clap::{Parser, Subcommand};
use dashmap::DashMap;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use once_cell::sync::Lazy;
use rusty_ytdl::{search, search::YouTube};
use rusty_ytdl::{RequestOptions, VideoOptions};
use serenity::all::{AutocompleteChoice, GuildId};
use std::borrow::Cow;
//------------------------------------
// Standard library imports
//------------------------------------
use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crack_types::suggestion_yt;
//------------------------------------
// Module statics.
// I did this so that I could easily make sure only one instance of the client is created
// and that it's available to all functions in the module.
// I've read elsewhere that this is a bit of a bad practice, and that it's better to put
// the clients in a context struct and pass it around everywhere. Other than the potential
// problems from it getting out of hand if the module is too big, I don't see a problem with it.
//------------------------------------
static REQ_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    println!("{}: {}...", CREATING, REQ_CLIENT_STR);
    build_configured_reqwest_client()
});

static YOUTUBE_CLIENT: Lazy<rusty_ytdl::search::YouTube> = Lazy::new(|| {
    println!("{CREATING}: {YOUTUBE_CLIENT_STR}...");
    let req_client = REQ_CLIENT.clone();
    let opts = RequestOptions {
        client: Some(req_client.clone()),
        ..Default::default()
    };
    rusty_ytdl::search::YouTube::new_with_options(&opts)
        .unwrap_or_else(|_| panic!("{} {}", NEW_FAILED, YOUTUBE_CLIENT_STR))
});

static CRACK_TRACK_CLIENT: Lazy<CrackTrackClient<'static>> = Lazy::new(|| {
    println!("{CREATING}: CrackTrackClient...");
    CrackTrackClient::new_with_clients(REQ_CLIENT.clone(), YOUTUBE_CLIENT.clone())
});

/// Build a configured reqwest client for use in the CrackTrackClient.
pub fn build_configured_reqwest_client() -> reqwest::Client {
    reqwest::ClientBuilder::new()
        .use_rustls_tls()
        .cookie_store(true)
        .build()
        .unwrap_or_else(|_| panic!("{} {}", NEW_FAILED, REQ_CLIENT_STR))
}

/// Get a suggestion from a query. Use the global static client.
/// # Errors
/// Returns an error if the query fails.
pub async fn suggestion2(query: &str) -> Result<Vec<AutocompleteChoice<'_>>, Error> {
    let client = CRACK_TRACK_CLIENT.clone();
    client.resolve_suggestion_search(query).await
}

/// Args struct for the CLI.
#[derive(Parser, Debug)]
#[command(
    version = "1.0",
    author = "Cycle Five <cycle.five@proton.me>",
    about = "A simple CLI to get autocomplete suggestions from YouTube."
)]
struct Cli {
    /// The command to run
    #[command(subcommand)]
    command: Commands,
}

/// The command to run.
#[derive(Subcommand, Debug)]
enum Commands {
    Suggest {
        /// The query to get suggestions for.
        query: String,
    },
    SuggestNew {
        /// The query to get suggestions for, second method.
        query: String,
    },
    Resolve {
        /// URL of the video / playlist to resolve.
        #[arg(value_parser = parse_url)]
        url: url::Url,
    },
    Query {
        /// The query to resolve.
        query: String,
    },
}

/// Get the query type from a youtube URL. Video or playlist.
fn yt_url_type(url: &url::Url) -> QueryType {
    if url.path().contains("playlist")
        || url.query_pairs().any(|(k, _)| k == "list") && url.path().contains("watch")
    {
        QueryType::PlaylistLink(url.to_string())
    } else {
        QueryType::VideoLink(url.to_string())
    }
}

/// Match the CLI command and run the appropriate function.
#[tracing::instrument]
async fn match_cli(cli: Cli) -> Result<(), Error> {
    let guild = GuildId::new(1);
    let client = Box::leak(Box::new(CrackTrackClient::new()));
    match cli.command {
        Commands::Suggest { query } => {
            // let res = suggestion(&query).await?;
            tracing::warn!("Deprecated!");
        },
        Commands::SuggestNew { query } => {
            let res = suggestion2(&query).await?;
            tracing::info!("Suggestions: {res:?}");
        },
        Commands::Resolve { url } => {
            let tracks = match yt_url_type(&url) {
                QueryType::VideoLink(url) => {
                    vec![client.resolve_track(QueryType::VideoLink(url)).await?]
                },
                QueryType::PlaylistLink(url) => {
                    let url = url.clone();
                    client.resolve_playlist(url.as_str()).await?
                },
                _ => {
                    tracing::error!("Unknown URL type: {url}");
                    Vec::new()
                },
            };
            client.append_queue(guild, tracks).await?;
        },
        Commands::Query { query } => {
            let queries = query.split(',');
            for query in queries {
                let res = client.resolve_search_one(query).await?;
                println!("Resolved: {res}");
                let _ = client.enqueue_track(guild, res).await;
            }
        },
    }
    Ok(())
}

/// Run the CLI.
/// # Errors
/// Returns an error if the CLI fails.
#[cfg(not(tarpaulin_include))]
pub async fn run() -> Result<(), Error> {
    let cli: Cli = Cli::parse();
    match_cli(cli).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::{match_cli, Cli};
    use clap::Parser;

    #[tokio::test]
    async fn test_cli() {
        let cli = Cli::parse_from(vec!["crack_testing", "suggest", "molly nilsson"]);
        match match_cli(cli).await {
            Ok(_) => (),
            Err(e) => eprintln!("{}", e),
        }
    }

    #[tokio::test]
    async fn test_cli2() {
        let cli = Cli::parse_from(vec![
            "crack_testing",
            "resolve",
            "https://www.youtube.com/playlist?list=PLc1HPXyC5ookjUsyLkdfek0WUIGuGXRcP",
        ]);
        match_cli(cli).await.expect("asdf");
    }

    #[tokio::test]
    async fn test_cli3() {
        let cli = Cli::parse_from(vec!["crack_testing", "suggest-new", "molly nilsson"]);
        match match_cli(cli).await {
            Ok(_) => (),
            Err(e) => eprintln!("{}", e),
        }
    }

    #[tokio::test]
    async fn test_cli4() {
        let cli = Cli::parse_from(vec!["crack_testing", "query", "molly nilsson"]);
        match match_cli(cli).await {
            Ok(_) => (),
            Err(e) => eprintln!("{}", e),
        }
    }
}
