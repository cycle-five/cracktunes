use config_file::FromConfigFile;
use crack_core::{config, sources::ytdl::HANDLE, BotConfig, BotCredentials, EventLogAsync};
use std::env;
use tokio::runtime::Handle;

#[cfg(feature = "crack-tracing")]
use crack_core::guild::settings::get_log_prefix;
#[cfg(feature = "crack-telemetry")]
use std::sync::Arc;
#[cfg(feature = "crack-tracing")]
use tracing_subscriber::{filter, prelude::*, EnvFilter, Registry};
// #[cfg(feature = "crack-metrics")]
// use {
//     crack_core::metrics::REGISTRY,
//     opentelemetry::global::set_text_map_propagator,
//     opentelemetry_sdk::propagation::TraceContextPropagator,
//     poise::serenity_prelude as serenity,
//     prometheus::{Encoder, TextEncoder},
//     warp::Filter,
// };

// #[cfg(feature = "crack-telemetry")]
// const SERVICE_NAME: &str = "cracktunes";
#[cfg(feature = "crack-metrics")]
const WARP_PORT: u16 = 8833;

type Error = Box<dyn std::error::Error + Send + Sync>;

use std::time::Duration;
/// Main function, get everything kicked off.
#[cfg(not(tarpaulin_include))]
//#[tokio::main]
fn main() -> Result<(), Error> {
    config::install_crypto_provider();
    // let event_log = EventLog::default();
    let event_log_async = EventLogAsync::default();

    dotenvy::dotenv().ok();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_keep_alive(Duration::from_millis(100))
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        *HANDLE.lock().unwrap() = Some(Handle::current());
        #[cfg(feature = "crack-tracing")]
        init_telemetry("").await;
        match main_async(event_log_async).await {
            Ok(_) => (),
            Err(error) => {
                tracing::error!("Error: {:?}", error);
            },
        }
    });

    // let url = "https://otlp-gateway-prod-us-east-0.grafana.net/otlp";

    // init_telemetry(url).await;
    // main_async(event_log_async).await?;

    Ok(())
}

/// Main async function, needed so we can  initialize everything.
#[cfg(not(tarpaulin_include))]
async fn main_async(event_log_async: EventLogAsync) -> Result<(), Error> {
    use crack_core::http_utils;

    // init_metrics();
    let config = load_bot_config().expect("Error: Failed to load bot config");
    tracing::warn!("Using config: {:?}", config);

    let mut client = config::poise_framework(config, event_log_async).await?;

    // Force the client to init.
    http_utils::init_http_client().await?;

    // let client = framework.client();
    let data_arc = client.data::<crack_core::Data>().clone();
    let guild_settings_map = data_arc.guild_settings_map.read().await.clone();

    for (guild_id, guild_settings) in guild_settings_map.iter() {
        tracing::info!("Guild: {:?} Settings: {:?}", guild_id, guild_settings);
    }

    // let bot = client.start_shards(2);
    let bot = client.start_autosharded();

    // #[cfg(feature = "crack-metrics")]
    // {
    //     let metrics_route = warp::path!("metrics").and_then(metrics_handler);

    //     let server = async {
    //         warp::serve(metrics_route)
    //             .run(([127, 0, 0, 1], WARP_PORT))
    //             .await;
    //         Ok::<(), serenity::Error>(())
    //     };
    //     tokio::try_join!(bot, server)?;
    // };
    // #[cfg(not(feature = "crack-metrics"))]
    bot.await?;

    Ok(())
}

// /// Prometheus handler
// #[cfg(feature = "crack-metrics")]
// #[cfg(not(tarpaulin_include))]
// async fn metrics_handler() -> Result<impl warp::Reply, warp::Rejection> {
//     let encoder = TextEncoder::new();
//     let mut metric_families = prometheus::gather();
//     metric_families.extend(REGISTRY.gather());
//     // tracing::info!("Metrics: {:?}", metric_families);
//     let mut buffer = vec![];
//     encoder.encode(&metric_families, &mut buffer).unwrap();

//     Ok(warp::reply::with_header(
//         buffer,
//         "content-type",
//         encoder.format_type(),
//     ))
// }

/// Load an environment variable
fn load_key(k: String) -> Result<String, Error> {
    match env::var(&k) {
        Ok(token) => Ok(token),
        Err(_) => {
            tracing::warn!("{} not found in environment.", &k);
            Err(format!("{} not found in environment.", &k).into())
        },
    }
}

/// Load the bot's config
fn load_bot_config() -> Result<BotConfig, Error> {
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
        },
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

/// Combine the stdout and debug log layers
#[cfg(feature = "crack-tracing")]
fn combine_log_layers(
    stdout_log: impl tracing_subscriber::Layer<Registry>,
    debug_log: impl tracing_subscriber::Layer<Registry>,
) -> impl tracing_subscriber::Layer<Registry> {
    // A layer that logs events to a file
    //let log = stdout_log
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

/// Get the debug log layer
#[cfg(feature = "crack-tracing")]
fn get_debug_log() -> impl tracing_subscriber::Layer<Registry> {
    let log_path = &format!("{}/debug.log", get_log_prefix());
    let debug_file = std::fs::File::create(log_path);

    // let log_file = std::fs::File::create("my_cool_trace.log")?;
    // let subscriber = tracing_subscriber::fmt::layer().with_writer(Mutex::new(log_file));

    match debug_file {
        Ok(file) => {
            //let xyz: tracing_subscriber::fmt::Layer<Registry> =
            //.with_writer(Box::make_writer(&Box::new(Mutex::new(file))));
            tracing_subscriber::fmt::layer().with_writer(file)
        },
        Err(error) => {
            println!("warning: no log file available for output! {:?}", error);
            // let sink: std::io::Sink = std::io::sink();
            // let writer = Arc::new(sink);
            let sink = std::fs::File::open("/dev/null").unwrap();
            tracing_subscriber::fmt::layer().with_writer(sink)
        },
    }
}

/// Get the current log layer
#[cfg(feature = "crack-tracing")]
fn get_current_log_layer() -> impl tracing_subscriber::Layer<Registry> {
    let stdout_log = tracing_subscriber::fmt::layer().pretty();

    // A layer that logs up to debug events.
    let debug_log = get_debug_log();

    // Get the debug layer.
    combine_log_layers(stdout_log, debug_log)
}

// #[tracing::instrument]
// /// Initialize metrics.
// fn init_metrics() {
//     #[cfg(feature = "crack-metrics")]
//     {
//         tracing::info!("Initializing metrics");
//         crack_core::metrics::register_custom_metrics();
//     }
//     #[cfg(not(feature = "crack-metrics"))]
//     {
//         tracing::info!("Metrics not enabled");
//     }
// }

#[tracing::instrument]
/// Initialize logging and tracing.
fn init_logging() {
    #[cfg(feature = "crack-tracing")]
    {
        let final_log = get_current_log_layer();
        tracing_subscriber::registry().with(final_log).init();
    }

    // init_telemetry("");

    tracing::warn!("Hello, world!");
}

#[cfg(feature = "crack-telemetry")]
const SERVICE_NAME: &str = "crack-tunes";

// #[tracing::instrument]
/// Initialize logging and tracing.
#[cfg(feature = "crack-tracing")]
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
    #[cfg(feature = "crack-tracing")]
    let subscriber = Registry::default();
    // Level filter layer to filter traces based on level (trace, debug, info, warn, error).
    #[cfg(feature = "crack-tracing")]
    let level_filter_layer = EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("INFO"));
    // Layer for adding our configured tracer.
    // let tracing_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    // Layer for printing spans to a file.
    #[cfg(feature = "crack-telemetry")]
    let stdout_formatting_layer = get_current_log_layer();

    // Layer for printing to stdout.
    #[cfg(feature = "crack-tracing")]
    let stdout_formatting_layer = get_current_log_layer();

    // global::set_text_map_propagator(TraceContextPropagator::new());
    #[cfg(feature = "crack-metrics")]
    set_text_map_propagator(TraceContextPropagator::new());

    #[cfg(feature = "crack-tracing")]
    let x = subscriber
        .with(stdout_formatting_layer)
        .with(level_filter_layer);
    // .with(tracing_layer)
    #[cfg(feature = "crack-telemetry")]
    let x = x.with(JsonStorageLayer).with(formatting_layer);

    #[cfg(any(feature = "crack-tracing", feature = "crack-telemetry"))]
    x.init()
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_init_telemetry() {
        init_telemetry("").await;
    }

    #[test]
    fn test_get_current_log_layer() {
        let _layer = get_current_log_layer();
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
    fn test_load_key() {
        let key = "DISCORD_TOKEN".to_string();
        let result = load_key(key);
        match result {
            Ok(token) => assert!(!token.is_empty()),
            Err(_error) => assert!(true),
        }
    }

    #[test]
    fn test_load_bot_config() {
        let result = load_bot_config();
        assert!(result.is_ok() || result.is_err());
    }
}
