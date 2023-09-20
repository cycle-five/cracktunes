use self::serenity::GatewayIntents;
use config_file::FromConfigFile;
use cracktunes::commands::PhoneCodeData;
use cracktunes::handlers::handle_event;
use cracktunes::metrics::{COMMAND_ERRORS, REGISTRY};
use cracktunes::utils::count_command;
use cracktunes::{
    commands,
    guild::settings::GuildSettings,
    handlers::SerenityHandler,
    utils::{check_interaction, check_reply, create_response_text, get_interaction},
    BotConfig, Data,
};
use cracktunes::{is_prefix, BotCredentials, DataInner, EventLog};
use poise::serenity_prelude::GuildId;
use poise::{serenity_prelude as serenity, Framework};
use prometheus::{Encoder, TextEncoder};
use songbird::serenity::SerenityInit;
use std::env;
use std::{
    collections::HashMap,
    process::exit,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::instrument;
use warp::Filter;

use {cracktunes::guild::cache::GuildCacheMap, cracktunes::guild::settings::GuildSettingsMap};

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
    cracktunes::metrics::register_custom_metrics();
}

#[allow(dead_code)]
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

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            COMMAND_ERRORS
                .with_label_values(&[&ctx.command().qualified_name])
                .inc();
            match get_interaction(ctx) {
                Some(interaction) => {
                    check_interaction(
                        create_response_text(
                            &ctx.serenity_context().http,
                            &interaction,
                            &format!("{error}"),
                        )
                        .await,
                    );
                }
                None => {
                    check_reply(
                        ctx.send(|builder| builder.content(&format!("{error}")))
                            .await,
                    );
                }
            }
            tracing::error!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e)
            }
        }
    }
}

/// Create the poise framework from the bot config.
async fn poise_framework(
    config: BotConfig,
    //TODO: can this be create in this function instead of passed in?
    event_log: EventLog,
) -> Result<Arc<Framework<Data, Error>>, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value
    tracing::warn!("Using prefix: {}", config.get_prefix());
    let up_prefix = config.get_prefix().to_ascii_uppercase();
    let up_prefix_cloned = Box::leak(Box::new(up_prefix.clone()));
    let options = poise::FrameworkOptions::<_, Error> {
        // owners: [1124878856491389012, 285219649921220608]
        //     .iter()
        //     .clone()
        //     .map(|id| UserId(*id))
        //     .collect(),
        commands: vec![
            commands::admin(),
            commands::autopause(),
            commands::boop(),
            commands::coinflip(),
            commands::create_playlist(),
            commands::delete_playlist(),
            commands::chatgpt(),
            commands::clear(),
            commands::help(),
            commands::leave(),
            commands::lyrics(),
            commands::grab(),
            commands::now_playing(),
            commands::pause(),
            commands::play(),
            commands::ping(),
            commands::remove(),
            commands::repeat(),
            commands::resume(),
            commands::servers(),
            commands::seek(),
            commands::skip(),
            commands::stop(),
            commands::shuffle(),
            commands::summon(),
            commands::version(),
            commands::volume(),
            commands::queue(),
            commands::osint(),
        ],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(config.get_prefix()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600))),
            additional_prefixes: vec![
                poise::Prefix::Literal(up_prefix_cloned),
                poise::Prefix::Literal("hey bot,"),
                poise::Prefix::Literal("hey bot"),
                poise::Prefix::Literal("bot,"),
                poise::Prefix::Literal("bot"),
            ],
            stripped_dynamic_prefix: Some(|ctx, msg, _| {
                Box::pin(async move {
                    let guild_id = msg.guild_id.unwrap();
                    let data_read = ctx.data.read().await;
                    let guild_settings_map = data_read.get::<GuildSettingsMap>().unwrap();
                    // tracing::warn!("guild_id: {}", guild_id);
                    // for (k, v) in guild_settings_map.iter() {
                    //     tracing::warn!("Guild: {} - {:?}", k, v);
                    // }

                    if let Some(guild_settings) = guild_settings_map.get(&guild_id) {
                        let prefix = &guild_settings.prefix;
                        let prefix_up = &guild_settings.prefix_up;

                        tracing::warn!("Checking for prefix: {}", prefix);

                        if msg.content.starts_with(prefix) {
                            Ok(Some(msg.content.split_at(prefix.len())))
                        } else if msg.content.starts_with(prefix_up) {
                            Ok(Some(msg.content.split_at(prefix_up.len())))
                        } else {
                            Ok(None)
                        }
                    } else {
                        Ok(None)
                    }
                })
            }),
            ..Default::default()
        },
        // The global error handler for all error cases that may occur
        on_error: |error| Box::pin(on_error(error)),
        // This code is run before every command
        pre_command: |ctx| {
            Box::pin(async move {
                tracing::info!("Executing command {}...", ctx.command().qualified_name);
                count_command(ctx.command().qualified_name.as_ref(), is_prefix(ctx));
            })
        },
        // This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                tracing::info!("Executed command {}!", ctx.command().qualified_name);
            })
        },
        // Every command invocation must pass this check to continue execution
        command_check: Some(|ctx| {
            Box::pin(async move {
                let command = ctx.command().qualified_name.clone();
                tracing::info!("Checking command {}...", command);
                let user_id = *ctx.author().id.as_u64();
                // ctx.author_member().await.map_or_else(
                //     || {
                //         tracing::info!("Author not found in guild");
                //         Ok(false)
                //     },
                //     |member| {
                //         tracing::info!("Author found in guild");
                //         Ok(member.permissions().contains(serenity::model::permissions::ADMINISTRATOR))
                //     },
                // )?;
                //let asdf = vec![user_id];
                let music_commands = vec![
                    "play",
                    "pause",
                    "resume",
                    "stop",
                    "skip",
                    "seek",
                    "volume",
                    "now_playing",
                    "queue",
                    "repeat",
                    "shuffle",
                    "clear",
                    "remove",
                    "grab",
                    "create_playlist",
                    "delete_playlist",
                ];
                if music_commands.contains(&command.as_str()) {
                    return Ok(true);
                }
                // if ctx
                //     .data()
                //     .bot_settings
                //     .authorized_users
                //     .as_ref()
                //     .unwrap_or(asdf.as_ref())
                //     .contains(&user_id)
                // {
                //     return Ok(true);
                // }

                //let user_id = ctx.author().id.as_u64();
                let guild_id = ctx.guild_id().unwrap_or_default();

                ctx.data()
                    .guild_settings_map
                    .lock()
                    .unwrap()
                    .get(&guild_id)
                    .map_or_else(
                        || {
                            tracing::info!("Guild not found in guild settings map");
                            Ok(true)
                        },
                        |guild_settings| {
                            tracing::info!("Guild found in guild settings map");
                            Ok(guild_settings.authorized_users.is_empty()
                                || guild_settings.authorized_users.contains(&user_id))
                        },
                    )
            })
        }),
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |ctx, event, _framework, data_global| {
            Box::pin(async move { handle_event(ctx, event, data_global).await })
        },
        ..Default::default()
    };
    let guild_settings_map = config
        .clone()
        .guild_settings_map
        .unwrap_or_default()
        .iter()
        .map(|gs| (gs.guild_id, gs.clone()))
        .collect::<HashMap<GuildId, GuildSettings>>();

    let db_url = config.get_database_url();
    let pool_opts = sqlx::sqlite::SqlitePoolOptions::new()
        .connect(&db_url)
        .await;
    let cloned_map = guild_settings_map.clone();
    let data = Data(Arc::new(DataInner {
        phone_data: PhoneCodeData::load().unwrap(),
        bot_settings: config.clone(),
        guild_settings_map: Arc::new(Mutex::new(cloned_map)),
        event_log,
        database_pool: pool_opts.unwrap().into(),
        ..Default::default()
    }));

    let save_data = data.clone();
    // ctrlc::set_handler(move || {
    //     tracing::warn!("Received Ctrl-C, shutting down...");
    //     save_data
    //         .guild_settings_map
    //         .lock()
    //         .unwrap()
    //         .iter()
    //         .for_each(|(k, v)| {
    //             tracing::warn!("Saving Guild: {}", k);
    //             v.save().expect("Error saving guild settings");
    //         });

    //     exit(0);
    // })
    // .expect("Error setting Ctrl-C handler");

    let handler_data = data.clone();
    let setup_data = data;
    let token = config
        .credentials
        .expect("Error getting discord token")
        .discord_token;
    let framework = poise::Framework::builder()
        .client_settings(|builder| {
            builder
                .event_handler(SerenityHandler {
                    is_loop_running: false.into(),
                    data: handler_data,
                })
                .register_songbird()
        })
        .token(token)
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!("Logged in as {}", ready.user.name);
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                ctx.data
                    .write()
                    .await
                    .insert::<GuildSettingsMap>(guild_settings_map.clone());
                Ok(setup_data)
            })
        })
        .options(options)
        .intents(
            GatewayIntents::non_privileged()
                | GatewayIntents::GUILD_MEMBERS
                | GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::GUILD_MESSAGE_REACTIONS
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::DIRECT_MESSAGE_TYPING
                | GatewayIntents::DIRECT_MESSAGE_REACTIONS
                | GatewayIntents::GUILDS
                | GatewayIntents::GUILD_VOICE_STATES
                | GatewayIntents::GUILD_PRESENCES
                | GatewayIntents::MESSAGE_CONTENT,
        );

    let res = framework.build().await?;
    let shard_manager = res.client().shard_manager.clone();

    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix as signal;

            let [mut s1, mut s2, mut s3] = [
                signal::signal(signal::SignalKind::hangup()).unwrap(),
                signal::signal(signal::SignalKind::interrupt()).unwrap(),
                signal::signal(signal::SignalKind::terminate()).unwrap(),
            ];

            tokio::select!(
                v = s1.recv() => v.unwrap(),
                v = s2.recv() => v.unwrap(),
                v = s3.recv() => v.unwrap(),
            );
        }
        #[cfg(windows)]
        {
            let (mut s1, mut s2) = (
                tokio::signal::windows::ctrl_c().unwrap(),
                tokio::signal::windows::ctrl_break().unwrap(),
            );

            tokio::select!(
                v = s1.recv() => v.unwrap(),
                v = s2.recv() => v.unwrap(),
            );
        }

        tracing::warn!("Received Ctrl-C, shutting down...");
        save_data
            .guild_settings_map
            .lock()
            .unwrap()
            .iter()
            .for_each(|(k, v)| {
                tracing::warn!("Saving Guild: {}", k);
                v.save().expect("Error saving guild settings");
            });
        shard_manager.lock().await.shutdown_all().await;

        exit(0);
    });

    res.client().start_autosharded().await?;
    Ok(res)
}
