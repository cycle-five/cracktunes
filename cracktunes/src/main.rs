use crack_config::poise_framework;
use crack_core::metrics::REGISTRY;
use crack_core::BotConfig;
pub use crack_core::PhoneCodeData;
use crack_core::{BotCredentials, EventLog};
use poise::serenity_prelude as serenity;
use prometheus::{Encoder, TextEncoder};
use std::env;
use std::{collections::HashMap, sync::Arc};
use tracing::instrument;
use warp::Filter;

use crack_core::guild::{cache::GuildCacheMap, settings::GuildSettingsMap};

use tracing_subscriber::{filter, prelude::*};
type Error = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), Error> {
    let event_log = EventLog::default();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .build()
        .unwrap();

    dotenv::dotenv().ok();
    rt.block_on(async { main_async(event_log).await })
}

async fn main_async(event_log: EventLog) -> Result<(), Error> {
    init_logging();
    init_metrics();
    let config = load_bot_config().await.unwrap();
    tracing::warn!("Using config: {:?}", config);

    let framework = poise_framework(config, event_log).await?;

    let client = framework.client();
    let mut data_global = client.data.write().await;
    data_global.insert::<GuildCacheMap>(HashMap::default());
    data_global.insert::<GuildSettingsMap>(HashMap::default());
    drop(data_global);
    drop(client);

    let metrics_route = warp::path!("metrics").and_then(metrics_handler);

    let server = async {
        warp::serve(metrics_route).run(([127, 0, 0, 1], 8000)).await;
        Ok::<(), serenity::Error>(())
    };

    let bot = framework.start(); //.await?;

    tokio::try_join!(bot, server)?;

    Ok(())
}

async fn metrics_handler() -> Result<impl warp::Reply, warp::Rejection> {
    let encoder = TextEncoder::new();
    let mut metric_families = prometheus::gather();
    metric_families.extend(REGISTRY.gather());
    // tracing::info!("Metrics: {:?}", metric_families);
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();

    Ok(warp::reply::with_header(
        buffer,
        "content-type",
        encoder.format_type(),
    ))
}

fn load_key(k: String) -> Result<String, Error> {
    match env::var(&k) {
        Ok(token) => Ok(token),
        Err(_) => {
            tracing::warn!("{} not found in environment", &k);
            Err(format!("{} not found in environment or Secrets.toml", k).into())
        }
    }
}

async fn load_bot_config() -> Result<BotConfig, Error> {
    let discord_token = load_key("DISCORD_TOKEN".to_string())?;
    let discord_app_id = load_key("DISCORD_APP_ID".to_string())?;
    let spotify_client_id = load_key("SPOTIFY_CLIENT_ID".to_string()).ok();
    let spotify_client_secret = load_key("SPOTIFY_CLIENT_SECRET".to_string()).ok();
    let openai_key = load_key("OPENAI_KEY".to_string()).ok();

    let config = match BotConfig::from_config_file("./cracktunes.toml") {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("Using default config: {:?}", error);
            BotConfig::default()
        }
    }
    .set_credentials(BotCredentials {
        discord_token,
        discord_app_id,
        spotify_client_id,
        spotify_client_secret,
        openai_key,
    });

    Ok(config)
}

#[instrument]
fn init_metrics() {
    tracing::info!("Initializing metrics");
    crack_core::metrics::register_custom_metrics();
}

#[instrument]
fn init_logging() {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();

    // A layer that logs up to debug events.
    let debug_file = std::fs::File::create("debug.log");
    let debug_file = match debug_file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };
    let debug_log = tracing_subscriber::fmt::layer().with_writer(Arc::new(debug_file));

    //oauth2_callback=debug,tower_http=debug
    // A layer that logs events to a file.
    // let trace_file = std::fs::File::create("trace.log");
    // let trace_file = match trace_file {
    //     Ok(file) => file,
    //     Err(error) => panic!("Error: {:?}", error),
    // };
    //let trace_log = tracing_subscriber::fmt::layer().with_writer(Arc::new(trace_file));
    tracing_subscriber::registry()
        .with(
            stdout_log
                // Add an `INFO` filter to the stdout logging layer
                .with_filter(filter::LevelFilter::INFO)
                // Combine the filtered `stdout_log` layer with the
                // `debug_log` layer, producing a new `Layered` layer.
                .and_then(debug_log)
                .with_filter(filter::LevelFilter::DEBUG)
                // Combine with trace layer
                // .and_then(trace_log)
                // Add a filter to *both* layers that rejects spans and
                // events whose targets start with `metrics`.
                .with_filter(filter::filter_fn(|metadata_global| {
                    !metadata_global.target().starts_with("metrics")
                })),
        )
        .init();

    tracing::warn!("Hello, world!");
}
