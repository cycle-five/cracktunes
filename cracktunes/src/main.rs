use config_file::FromConfigFile;
use crack_core::guild::{cache::GuildCacheMap, settings::GuildSettingsMap};
use crack_core::metrics::REGISTRY;
use crack_core::BotConfig;
pub use crack_core::PhoneCodeData;
use crack_core::{BotCredentials, EventLog};
use cracktunes::poise_framework;
use poise::serenity_prelude as serenity;
use prometheus::{Encoder, TextEncoder};
use std::env;
use std::{collections::HashMap, sync::Arc};
use tracing_subscriber::{filter, prelude::*, EnvFilter, Registry};
use warp::Filter;
#[cfg(feature = "crack-telemetry")]
use {
    opentelemetry::global::set_text_map_propagator,
    // opentelemetry_otlp::WithExportConfig,
    opentelemetry_sdk::propagation::TraceContextPropagator,
    tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer},
};

#[cfg(feature = "crack-telemetry")]
const SERVICE_NAME: &str = "cracktunes";

type Error = Box<dyn std::error::Error + Send + Sync>;

/// Main function, get everything kicked off.
#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() -> Result<(), Error> {
    let event_log = EventLog::default();
    // let rt = tokio::runtime::Builder::new_multi_thread()
    //     //.worker_threads(16)
    //     .enable_all()
    //     .build()
    //     .unwrap();

    dotenv::dotenv().ok();
    // rt.block_on(async {
    //     init_telemetry("").await;
    //     main_async(event_log).await
    // })

    let url = "https://otlp-gateway-prod-us-east-0.grafana.net/otlp";

    init_telemetry(url).await;
    main_async(event_log).await?;
    Ok(())
}

/// Main async function, needed so we can  initialize everything.
#[cfg(not(tarpaulin_include))]
async fn main_async(event_log: EventLog) -> Result<(), Error> {
    init_metrics();
    let config = load_bot_config().await.unwrap();
    tracing::warn!("Using config: {:?}", config);

    let mut client = poise_framework(config, event_log).await?;

    // let client = framework.client();
    let data_ro = client.data.clone();
    let mut data_global = data_ro.write().await;
    match data_global.get::<GuildSettingsMap>() {
        Some(guild_settings_map) => {
            for (guild_id, guild_settings) in guild_settings_map.iter() {
                tracing::info!("Guild: {:?} Settings: {:?}", guild_id, guild_settings);
            }
        }
        None => {
            tracing::info!("No guild settings found");
            data_global.insert::<GuildSettingsMap>(HashMap::default());
        }
    }
    data_global.insert::<GuildCacheMap>(HashMap::default());

    drop(data_global);

    let metrics_route = warp::path!("metrics").and_then(metrics_handler);

    let server = async {
        warp::serve(metrics_route).run(([127, 0, 0, 1], 8000)).await;
        Ok::<(), serenity::Error>(())
    };

    let bot = client.start();

    tokio::try_join!(bot, server)?;

    Ok(())
}

/// Prometheus handler
#[cfg(not(tarpaulin_include))]
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

/// Load an environment variable
fn load_key(k: String) -> Result<String, Error> {
    match env::var(&k) {
        Ok(token) => Ok(token),
        Err(_) => {
            tracing::warn!("{} not found in environment", &k);
            Err(format!("{} not found in environment or Secrets.toml", k).into())
        }
    }
}

/// Load the bot's config
async fn load_bot_config() -> Result<BotConfig, Error> {
    let discord_token = load_key("DISCORD_TOKEN".to_string())?;
    let discord_app_id = load_key("DISCORD_APP_ID".to_string())?;
    let spotify_client_id = load_key("SPOTIFY_CLIENT_ID".to_string()).ok();
    let spotify_client_secret = load_key("SPOTIFY_CLIENT_SECRET".to_string()).ok();
    let openai_api_key = load_key("OPENAI_API_KEY".to_string()).ok();
    let virustotal_api_key = load_key("VIRUSTOTAL_API_KEY".to_string()).ok();

    let config_res = BotConfig::from_config_file("./cracktunes.toml");
    let mut config = match config_res {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("Using default config: {:?}", error);
            BotConfig::default()
        }
    };
    let config_with_creds = config.set_credentials(BotCredentials {
        discord_token,
        discord_app_id,
        spotify_client_id,
        spotify_client_secret,
        openai_api_key,
        virustotal_api_key,
    });

    Ok(config_with_creds.clone())
}

fn combine_log_layers(
    stdout_log: impl tracing_subscriber::Layer<Registry>,
    debug_log: impl tracing_subscriber::Layer<Registry>,
) -> impl tracing_subscriber::Layer<Registry> {
    // A layer that logs events to a file
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
        }))
}

fn get_debug_log() -> impl tracing_subscriber::Layer<Registry> {
    let debug_file = std::fs::File::create("/data/debug.log");
    let debug_file = match debug_file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };
    tracing_subscriber::fmt::layer().with_writer(Arc::new(debug_file))
}

#[allow(dead_code)]
fn get_bunyan_writer() -> Arc<std::fs::File> {
    let debug_file = std::fs::File::create("/data/bunyan.log");
    let debug_file = match debug_file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };
    Arc::new(debug_file)
}

fn get_current_log_layer() -> impl tracing_subscriber::Layer<Registry> {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();

    // A layer that logs up to debug events.
    let debug_log = get_debug_log();

    // Get the debug layer.
    combine_log_layers(stdout_log, debug_log)
}

#[tracing::instrument]
/// Initialize metrics.
fn init_metrics() {
    tracing::info!("Initializing metrics");
    crack_core::metrics::register_custom_metrics();
}

#[tracing::instrument]
/// Initialize logging and tracing.
fn init_logging() {
    let final_log = get_current_log_layer();
    tracing_subscriber::registry().with(final_log).init();

    // init_telemetry("");

    tracing::warn!("Hello, world!");
}

// const SERVICE_NAME: &str = "crack-tunes";

// #[tracing::instrument]
/// Initialize logging and tracing.
pub async fn init_telemetry(_exporter_endpoint: &str) {
    // Create a gRPC exporter
    // let exporter = opentelemetry_otlp::new_exporter()
    //     .tonic()
    //     .with_endpoint(exporter_endpoint);

    // Define a tracer
    // let tracer = opentelemetry_otlp::new_pipeline()
    //     .tracing()
    //     .with_exporter(exporter)
    //     .with_trace_config(
    //         trace::config().with_resource(Resource::new(vec![KeyValue::new(
    //             opentelemetry_semantic_conventions::resource::SERVICE_NAME,
    //             SERVICE_NAME.to_string(),
    //         )])),
    //     )
    //     .install_batch(opentelemetry_sdk::runtime::Tokio)
    //     .expect("Error: Failed to initialize the tracer.");

    // Define a subscriber.
    let subscriber = Registry::default();
    // Level filter layer to filter traces based on level (trace, debug, info, warn, error).
    let level_filter_layer = EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("INFO"));
    // Layer for adding our configured tracer.
    // let tracing_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    // Layer for printing spans to a file.
    #[cfg(feature = "crack-telemetry")]
    let formatting_layer =
        BunyanFormattingLayer::new(SERVICE_NAME.to_string(), get_bunyan_writer());

    // Layer for printing to stdout.
    let stdout_formatting_layer = get_current_log_layer();

    // global::set_text_map_propagator(TraceContextPropagator::new());
    #[cfg(feature = "crack-telemetry")]
    set_text_map_propagator(TraceContextPropagator::new());

    let x = subscriber
        .with(stdout_formatting_layer)
        .with(level_filter_layer);
    // .with(tracing_layer)
    #[cfg(feature = "crack-telemetry")]
    let x = x.with(JsonStorageLayer).with(formatting_layer);

    x.init()
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(feature = "crack-telemetry")]
    use {
        opentelemetry::global::set_text_map_propagator,
        opentelemetry_sdk::propagation::TraceContextPropagator,
        tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer},
    };

    #[tokio::test]
    async fn test_init_telemetry() {
        init_telemetry("").await;
    }

    #[test]
    fn test_get_current_log_layer() {
        let _layer = get_current_log_layer();
        //assert!(layer.h(&tracing::Level::INFO));
    }

    #[test]
    fn test_get_debug_log() {
        let _layer = get_debug_log();
    }

    #[test]
    fn test_combine_log_layers() {
        let stdout_log = tracing_subscriber::fmt::layer().pretty();
        let debug_log = get_debug_log();
        let _layer = combine_log_layers(stdout_log, debug_log);
    }

    #[test]
    fn test_init_metrics() {
        init_metrics();
    }

    #[test]
    fn test_load_key() {
        let key = "DISCORD_TOKEN".to_string();
        let result = load_key(key);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_load_bot_config() {
        let result = load_bot_config().await;
        assert!(result.is_ok());
    }
}
