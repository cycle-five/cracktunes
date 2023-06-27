use cracktunes::{
    commands,
    guild::{
        cache::GuildCacheMap,
        settings::{GuildSettings, GuildSettingsMap},
    },
    handlers::SerenityHandler,
    utils::{create_response_text, get_interaction},
    BotConfig, Data,
};
use poise::{serenity_prelude as serenity, FrameworkBuilder};
use songbird::serenity::SerenityInit;
use std::{
    collections::HashMap,
    env::var,
    process::exit,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing_subscriber::{filter, prelude::*};
type Error = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
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
    let trace_file = std::fs::File::create("trace.log");
    let trace_file = match trace_file {
        Ok(file) => file,
        Err(error) => panic!("Error: {:?}", error),
    };
    let trace_log = tracing_subscriber::fmt::layer().with_writer(Arc::new(trace_file));
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
                .and_then(trace_log)
                // Add a filter to *both* layers that rejects spans and
                // events whose targets start with `metrics`.
                .with_filter(filter::filter_fn(|metadata| {
                    !metadata.target().starts_with("metrics")
                })),
        )
        .init();

    tracing::warn!("Hello, world!");

    use config_file::FromConfigFile;

    let config = match BotConfig::from_config_file("./cracktunes.json") {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("Using default config: {:?}", error);
            BotConfig::default()
        }
    };
    tracing::warn!("Using config: {:?}", config);
    let framework = poise_framework(config);
    let framework = framework.build().await?;

    let client = framework.client();
    let mut data = client.data.write().await;
    //let mut data = ctx.serenity_context().data.write().await;
    data.insert::<GuildCacheMap>(HashMap::default());
    data.insert::<GuildSettingsMap>(HashMap::default());
    drop(data);
    drop(client);

    framework.start().await?;

    Ok(())
}

async fn on_error(error: poise::FrameworkError<'_, Arc<Data>, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            let mut interaction = get_interaction(ctx).unwrap();
            let _ = create_response_text(
                &ctx.serenity_context().http,
                &mut interaction,
                &format!("{error}"),
            )
            .await;
            tracing::error!("Error in command `{}`: {:?}", ctx.command().name, error,);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e)
            }
        }
    }
}

fn poise_framework(config: BotConfig) -> FrameworkBuilder<Arc<Data>, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value
    let options = poise::FrameworkOptions::<_, Error> {
        commands: vec![
            commands::admin(),
            commands::authorize(),
            commands::deauthorize(),
            commands::autopause(),
            commands::boop(),
            commands::chatgpt(),
            commands::clear(),
            commands::help(),
            commands::leave(),
            commands::now_playing(),
            commands::pause(),
            commands::play(),
            commands::ping(),
            commands::remove(),
            commands::repeat(),
            commands::resume(),
            commands::seek(),
            commands::skip(),
            commands::stop(),
            commands::shuffle(),
            commands::summon(),
            commands::version(),
            commands::volume(),
            commands::queue(),
        ],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("~".into()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600))),
            additional_prefixes: vec![
                poise::Prefix::Literal("hey bot"),
                poise::Prefix::Literal("hey bot,"),
            ],
            ..Default::default()
        },
        /// The global error handler for all error cases that may occur
        on_error: |error| Box::pin(on_error(error)),
        /// This code is run before every command
        pre_command: |ctx| {
            Box::pin(async move {
                tracing::info!("Executing command {}...", ctx.command().qualified_name);
            })
        },
        /// This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                tracing::info!("Executed command {}!", ctx.command().qualified_name);
            })
        },
        /// Every command invocation must pass this check to continue execution
        command_check: Some(|ctx| {
            //Box::pin(async move {Ok(true)})
            Box::pin(async move {
                tracing::info!("Checking command {}...", ctx.command().qualified_name);
                if ctx.data().bot_settings.authorized_users.is_empty()
                    || ctx
                        .data()
                        .bot_settings
                        .authorized_users
                        .contains(&ctx.author().id.0)
                {
                    return Ok(true);
                }

                let user_id = ctx.author_member().await.unwrap().user.id.0;
                let guild_id = ctx.guild_id().unwrap_or_default();

                ctx.data()
                    .guild_settings_map
                    .lock()
                    .unwrap()
                    .get(guild_id.as_u64())
                    .map_or_else(
                        || {
                            tracing::info!("Guild not found in guild settings map");
                            Ok(false)
                        },
                        |guild_settings| {
                            tracing::info!("Guild found in guild settings map");
                            Ok(guild_settings.authorized_users.contains(&user_id))
                        },
                    )
            })
        }),
        /// Enforce command checks even for owners (enforced by default)
        /// Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |_ctx, event, _framework, _data| {
            Box::pin(async move {
                tracing::info!("Got an event in event handler: {:?}", event.name());
                Ok(())
            })
        },
        ..Default::default()
    };
    let guild_settings_map = config
        .guild_settings_map
        .iter()
        .map(|gs| (*gs.guild_id.as_u64(), gs.clone()))
        .collect::<HashMap<u64, GuildSettings>>();
    let data = Arc::new(cracktunes::Data {
        bot_settings: config,
        guild_settings_map: Arc::new(Mutex::new(guild_settings_map)),
        ..Default::default()
    });

    let save_data = data.clone();
    ctrlc::set_handler(move || {
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

        exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let handler_data = data.clone();
    let setup_data = data;
    poise::Framework::builder()
        .client_settings(|builder| {
            builder
                .event_handler(SerenityHandler {
                    is_loop_running: false.into(),
                    data: handler_data,
                })
                .register_songbird()
        })
        .token(
            var("DISCORD_TOKEN")
                .expect("Missing `DISCORD_TOKEN` env var, see README for more information."),
        )
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                tracing::info!("Logged in as {}", _ready.user.name);
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(setup_data)
            })
        })
        .options(options)
        .intents(
            serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT,
        )
}
