use self::serenity::GatewayIntents;
use colored::Colorize;
use config_file::FromConfigFile;
use cracktunes::{
    commands,
    guild::{
        cache::GuildCacheMap,
        settings::{GuildSettings, GuildSettingsMap},
    },
    handlers::{serenity::voice_state_diff_str, SerenityHandler},
    utils::{check_interaction, check_reply, create_response_text, get_interaction},
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

#[cfg(feature = "shuttle")]
#[shuttle_runtime::main]
async fn serenity(
    #[shuttle_secrets::Secrets] secret_store: SecretStore,
) -> shuttle_serenity::ShuttleSerenity {
    // Get the discord token set in `Secrets.toml`

    let token = secret_store
        .get("DISCORD_TOKEN")
        .context("'DISCORD_TOKEN' was not found")?;
    let app_id = secret_store
        .get("DISCORD_APP_ID")
        .context("'DISCORD_APP_ID' was not found")?;
    let spotify_client = secret_store
        .get("SPOTIFY_CLIENT_ID")
        .context("'SPOTIFY_CLIENT_ID' was not found")?;
    let spotify_secret = secret_store
        .get("SPOTIFY_CLIENT_SECRET")
        .context("'SPOTIFY_CLIENT_SECRET' was not found")?;
    let openai_key = secret_store
        .get("OPENAI_KEY")
        .context("'OPENAI_KEY' was not found")?;

    init_logging().await;

    let config = match BotConfig::from_config_file("./cracktunes.toml") {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!("Using default config: {:?}", error);
            BotConfig::default()
        }
    };
    tracing::warn!("Using config: {:?}", config);
    let framework = poise_framework(config);
    let framework = framework
        .build()
        .await
        .map_err(|e| CrackedError::Serenity(e))?;

    // use ::serenity::Client;
    // use shuttle_serenity::{NewTrait, NewTraitClone, NewTraitCloneBox};

    // let client: &dyn shuttle_serenity::NewTraitClone =
    //    <&dyn NewTrait<'_, Target = Client>>::into(framework.client());
    let client = framework.client();
    let mut data = client.data.write().await;
    data.insert::<GuildCacheMap>(HashMap::default());
    data.insert::<GuildSettingsMap>(HashMap::default());
    drop(data);
    drop(client);

    match framework.clone().start().await {
        Ok(_) => {}
        Err(e) => {
            tracing::error!("Error starting framework: {}", e);
            return Err(<shuttle_serenity::Error as From<CrackedError>>::from(
                e.into(),
            ));
        }
    }

    // if let client = (<std::boxed::Box<
    //     impl std::ops::DerefMut + std::ops::Deref<Target = poise::serenity_prelude::Client> + '_,
    // > as std::convert::Into<T>>::into(Box::new(client))
    //     as Box<serenity::Client>)
    use shuttle_serenity::SerenityService;
    use shuttle_serenity::{NewTrait, NewTraitClone, NewTraitCloneBox};
    let client_ref = std::pin::Pin::new(Box::new(framework.client())) as Box<dyn NewTraitClone>;
    let asdf = <dyn NewTraitClone>::into(Box::new(client_ref) as Box<dyn NewTrait>);
    //let client: Box<poise::serenity_prelude::Client> = Box::new(client.into());
    //let client: NewTraitCloneBox = Box::new();
    let ss = SerenityService((*client).clone());
    Ok(ss)

    //let client: shuttle_serenity::NewTrait = &framework.clone().client().into();
}

#[cfg(not(feature = "shuttle"))]
#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    init_logging().await;

    let config = match BotConfig::from_config_file("./cracktunes.toml") {
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
    data.insert::<GuildCacheMap>(HashMap::default());
    data.insert::<GuildSettingsMap>(HashMap::default());
    drop(data);
    drop(client);

    framework.start().await?;

    Ok(())
}

async fn init_logging() {
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
}

async fn on_error(error: poise::FrameworkError<'_, Arc<Data>, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::Command { error, ctx } => {
            match get_interaction(ctx) {
                Some(mut interaction) => {
                    check_interaction(
                        create_response_text(
                            &ctx.serenity_context().http,
                            &mut interaction,
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

fn poise_framework(config: BotConfig) -> FrameworkBuilder<Arc<Data>, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value
    let options = poise::FrameworkOptions::<_, Error> {
        commands: vec![
            commands::admin(),
            commands::autopause(),
            commands::boop(),
            commands::coinflip(),
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
            prefix: Some(config.get_prefix()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600))),
            additional_prefixes: vec![poise::Prefix::Literal("rs!")],
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
            // Box::pin(async move {
            //     // let guild_id = ctx.guild_id().unwrap_or_default();
            //     Ok(true)
            // })
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
                            Ok(guild_settings.authorized_users.is_empty()
                                || guild_settings.authorized_users.contains(&user_id))
                        },
                    )
            })
        }),
        /// Enforce command checks even for owners (enforced by default)
        /// Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |ctx, event, _framework, _data| {
            Box::pin(async move {
                match event {
                    poise::Event::PresenceUpdate { new_data } => {
                        let _ = new_data;
                        tracing::trace!("Got a presence update: {:?}", new_data);
                        Ok(())
                    }
                    poise::Event::PresenceReplace { new_presences } => {
                        let _ = new_presences;
                        tracing::trace!("Got a presence replace");
                        Ok(())
                    }
                    poise::Event::GuildMemberAddition { new_member } => {
                        tracing::info!("Got a new member: {:?}", new_member);
                        Ok(())
                    }
                    poise::Event::VoiceStateUpdate { old, new } => {
                        tracing::debug!(
                            "VoiceStateUpdate: {}",
                            voice_state_diff_str(old.clone(), new).bright_yellow()
                        );
                        Ok(())
                    }
                    poise::Event::Message { new_message } => {
                        let serde_msg = serde_json::to_string(new_message).unwrap();
                        tracing::trace!(target = "events_serde", "serde_msg: {}", serde_msg);
                        Ok(())
                    }
                    poise::Event::TypingStart { event } => {
                        //let serde_msg = serde_json::to_string(event).unwrap();
                        let cache_http = ctx.http.clone();
                        let channel = event
                            .channel_id
                            .to_channel_cached(ctx.cache.clone())
                            .unwrap();
                        let user = event.user_id.to_user(cache_http.clone()).await.unwrap();
                        let channel_name = channel
                            .guild()
                            .map(|guild| guild.name)
                            .unwrap_or("DM".to_string());
                        let guild = event
                            .guild_id
                            .unwrap_or_default()
                            .to_guild_cached(ctx.cache.clone())
                            .map(|guild| guild.name)
                            .unwrap_or("DM".to_string());

                        tracing::info!(
                            "{}{} / {} / {} / {}",
                            "TypingStart: ".bright_green(),
                            user.name.bright_yellow(),
                            user.id.to_string().bright_yellow(),
                            channel_name.bright_yellow(),
                            guild.bright_yellow(),
                        );
                        Ok(())
                    }
                    _ => {
                        tracing::info!("{}", event.name().bright_green());
                        Ok(())
                    }
                }
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
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!("Logged in as {}", ready.user.name);
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
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
        )
}
