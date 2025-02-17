use crate::guild::operations::GuildSettingsOperations;
use crate::guild::settings::DEFAULT_PREFIX;
use crack_types::to_fixed;
// #[cfg(feature = "crack-metrics")]
// use crate::metrics::COMMAND_ERRORS;
use crate::http_utils;
use crate::poise_ext::PoiseContextExt;
use crate::{
    db,
    guild::settings::GuildSettings,
    handlers::{handle_event_noop, SerenityHandler},
    http_utils::CacheHttpExt,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    utils::{check_reply, count_command},
    BotConfig, Context, Data, DataInner, Error, EventLogAsync, PhoneCodeData,
};
use ::serenity::secrets::Token;
use colored::Colorize;
use crack_types::messaging::messages::FAIL_RUSTLS_PROVIDER_LOAD;
use crack_types::CrackedError;
use poise::serenity_prelude::{Client, FullEvent, GatewayIntents, GuildId, UserId};
use songbird::driver::DecodeMode;
use songbird::Songbird;
use std::borrow::Cow;
use std::{collections::HashMap, process::exit, sync::Arc, time::Duration};
use tokio::sync::RwLock;

/// `on_error` is called when an error occurs in the framework.
async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        //poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::EventHandler { error, event, .. } => match event {
            FullEvent::PresenceUpdate { .. } => { /* Ignore PresenceUpdate in terminal logging, too spammy */
            },
            _ => {
                tracing::warn!(
                    "{} {} {} {}",
                    "In event handler for ".yellow(),
                    event.snake_case_name().yellow().italic(),
                    " event: ".yellow(),
                    error.to_string().yellow().bold(),
                );
            },
        },
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::warn!(
                "<<< {} Error in command: {:?}",
                ctx.command().qualified_name,
                error.to_string(),
            );
            let myerr = CrackedError::Poise(error);
            let params = SendMessageParams::new(CrackedMessage::CrackedError(myerr));

            check_reply(ctx.send_message(params).await.map_err(Into::into));
            // #[cfg(feature = "crack-metrics")]
            // COMMAND_ERRORS
            //     .with_label_values(&[&ctx.command().qualified_name])
            //     .inc();
        },
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e);
            }
        },
    }
}

/// Installs the AWS LC provider for rustls.
pub fn install_crypto_provider() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect(FAIL_RUSTLS_PROVIDER_LOAD);
}

/// Create the poise framework from the bot config.
pub async fn poise_framework(
    config: BotConfig,
    event_log_async: EventLogAsync,
) -> Result<Client, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value

    // tracing::warn!("Using prefix: {}", config.get_prefix());
    // let up_prefix = config.get_prefix().to_ascii_uppercase();
    // // FIXME: Is this the proper way to allocate this memory?
    // let up_prefix_cloned = Box::leak(Box::new(up_prefix.clone()));

    let commands = crate::commands::all_commands();
    let _commands_map = crate::commands::all_commands_map();
    let commands_str = crate::commands::all_command_names();

    tracing::warn!("Commands: {:#?}", commands_str);

    let options = poise::FrameworkOptions::<_, Error> {
        commands,
        owners: config
            .owners
            .as_ref()
            .unwrap_or(&vec![285_219_649_921_220_608])
            .iter()
            .map(|id| UserId::new(*id))
            .collect(),
        prefix_options: poise::PrefixFrameworkOptions {
            case_insensitive_commands: true,
            prefix: Some(Cow::Owned(config.get_prefix())),
            ignore_bots: false, // This is for automated smoke tests
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600)).into()),
            additional_prefixes: vec![],
            stripped_dynamic_prefix: Some(|ctx, msg, data| {
                Box::pin(async move {
                    // allow specific bots with specific prefixes to use bot commands for testing.
                    if msg.author.bot() {
                        if !crate::poise_ext::check_bot_message(ctx, msg) {
                            return Ok(None);
                        }

                        // FIXME: Make this less hacky
                        if !msg.content.starts_with("{test}!")
                            || ctx.cache.current_user().name.starts_with("Crack")
                        {
                            return Ok(None);
                        } else {
                            return Ok(Some(msg.content.split_at(7)));
                        }
                    }
                    let guild_id = match msg.guild_id {
                        Some(id) => id,
                        None => {
                            return Ok(None);
                        },
                    };
                    // ----------------
                    // Here we re-check the original prefix because without message content intents
                    // we can't check the prefix in the message content and only get this is we
                    // are mentioned in the message.
                    let original_prefix = data
                        .get_prefix(guild_id)
                        .await
                        .unwrap_or(DEFAULT_PREFIX.to_string());
                    if msg.content.starts_with(&original_prefix) {
                        return Ok(Some(msg.content.split_at(original_prefix.len())));
                    }
                    // ----------------

                    let guild_settings_map = data.guild_settings_map.read().await.clone();

                    if let Some(guild_settings) = guild_settings_map.get(&guild_id) {
                        let prefixes = &guild_settings.additional_prefixes;
                        if prefixes.is_empty() {
                            tracing::trace!(
                                "Prefix is empty for guild {}",
                                guild_settings.guild_name
                            );
                            return Ok(None);
                        }

                        if let Some(prefix_len) = check_prefixes(prefixes, &msg.content) {
                            Ok(Some(msg.content.split_at(prefix_len)))
                        } else {
                            tracing::trace!("Prefix not found");
                            Ok(None)
                        }
                    } else {
                        tracing::warn!("Guild not found in guild settings map");
                        // Insert a default guild settings object
                        let guild_name = ctx
                            .guild_name_from_guild_id(guild_id)
                            .await
                            .unwrap_or_default();
                        let guild_settings = GuildSettings::new(guild_id, None, Some(guild_name));
                        let res = data.insert_guild(guild_id, guild_settings.clone()).await;
                        if res.is_err() {
                            tracing::warn!("Error inserting guild settings");
                        } else {
                            tracing::warn!("Inserted guild settings: {guild_settings:?}");
                        }
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
                tracing::trace!(">>> {}!", ctx.command().qualified_name);

                count_command(ctx.command().qualified_name.as_ref(), ctx.is_prefix());
            })
        },
        // This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                tracing::trace!("<<< {}!", ctx.command().qualified_name);
            })
        },
        // Every command invocation must pass this check to continue execution
        command_check: Some(|ctx| {
            Box::pin(async move {
                let guild_id = ctx.guild_id();
                let name = match guild_id {
                    None => return Ok(true),
                    Some(guild_id) => ctx.guild_name_from_guild_id(guild_id).await,
                }
                .unwrap_or_else(|_| to_fixed("Unknown"));
                tracing::trace!("Guild: {}", name);
                Ok(true)
            })
        }),
        //event_handler: |ctx, event, framework, data_global| {
        event_handler: |framework, event| {
            Box::pin(async move {
                let ctx = framework.serenity_context;
                handle_event_noop(ctx, event, framework, framework.user_data()).await
            })
        },
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        initialize_owners: true,
        ..Default::default()
    };
    let config_ref = &config;
    let guild_settings_map = config_ref
        .guild_settings_map
        .as_ref()
        .map(|x| {
            x.iter()
                .map(|gs| (gs.guild_id, gs.clone()))
                .collect::<HashMap<GuildId, GuildSettings>>()
        })
        .unwrap_or_default();

    let db_url: &str = &config_ref.get_database_url();
    let database_pool = match sqlx::postgres::PgPoolOptions::new().connect(db_url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            tracing::error!("Error getting database pool: {}, db_url: {}", e, db_url);
            None
        },
    };
    let db_channel = match database_pool.clone().map(db::worker_pool::setup_workers) {
        Some(c) => Some(c.await),
        None => None,
    };

    let songbird_config = songbird::Config::default().decode_mode(DecodeMode::Decode);
    let manager: Arc<Songbird> = songbird::Songbird::serenity_from_config(songbird_config);

    let cloned_map = guild_settings_map.clone();
    let data = Data(Arc::new(DataInner {
        phone_data: PhoneCodeData::default(),
        bot_settings: config.clone(),
        guild_settings_map: Arc::new(RwLock::new(cloned_map)),
        songbird: manager.clone(),
        event_log_async,
        database_pool,
        db_channel,
        http_client: http_utils::get_client().clone(),
        ..Default::default()
    }));

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MESSAGE_TYPING
        | GatewayIntents::GUILD_MESSAGE_REACTIONS;

    let token = config
        .credentials
        .expect("Error getting discord token")
        .discord_token
        .parse::<Token>()?;
    let data2 = data.clone();
    // FIXME: Why can't we use framework.user_data() later in this function? (it hangs)
    let framework = poise::Framework::new(
        options,
        // Box::pin(async move {
        //     tracing::info!("Logged in as {}", ready.user.name);
        //     crate::commands::register::register_globally_cracked(
        //         &ctx,
        //         &crate::commands::commands_to_register(),
        //     )
        //     .await?;
        //     ctx.data
        //         .write()
        //         .await
        //         .insert::<GuildSettingsMap>(guild_settings_map.clone());
        //     Ok(data.clone())
        // }),
    );

    // songbird::register_serenity!(framework, songbird_config);
    // let bot_test_handler = Arc::new(ForwardBotTestCommandsHandler {

    //     options: Default::default(),
    //     cmd_lookup: commands_map,
    //     shard_manager: std::sync::Mutex::new(None),
    // });
    let serenity_handler = SerenityHandler {
        is_loop_running: false.into(),
        data: data2.clone(),
    };

    let client = Client::builder(token, intents)
        .voice_manager::<Songbird>(manager.clone())
        .event_handler(serenity_handler)
        .data(data2.clone().into())
        .framework(framework)
        //.event_handler_arc(bot_test_handler.clone())
        .await
        .unwrap();
    //*bot_test_handler.shard_manager.lock().unwrap() = Some(client.shard_manager.clone());
    let shard_manager = client.shard_manager.clone();

    // let data2 = client.data.clone();
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
        let guilds = data2.guild_settings_map.read().await.clone();
        let pool = data2.clone().database_pool.clone();
        let mut saved_guilds = Vec::with_capacity(guilds.len());

        println!("Saving guilds...");
        if pool.is_some() {
            let p = pool.unwrap();
            for (k, v) in guilds {
                //tracing::warn!("Saving Guild: {}", k);
                match v.save(&p).await {
                    Ok(()) => {
                        saved_guilds.push(k);
                    },
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    },
                }
            }
            p.close().await;
        }
        println!("Saved guilds: {saved_guilds:?}");
        tracing::trace!("Saved guilds: {:?}", saved_guilds);

        shard_manager.clone().shutdown_all().await;

        exit(0);
    });

    // let shard_manager_2 = client.shard_manager.clone();
    // tokio::spawn(async move {
    //     loop {
    //         let count = shard_manager_2.shards_instantiated().await.len();
    //         let intents = shard_manager_2.intents();

    //         tracing::warn!("Shards: {}, Intents: {:?}", count, intents);

    //         tokio::time::sleep(Duration::from_secs(10)).await;
    //     }
    // });

    Ok(client)
}

/// Checks if the message starts with any of the given prefixes.
fn check_prefixes(prefixes: &[String], content: &str) -> Option<usize> {
    for prefix in prefixes {
        if content.starts_with(prefix) {
            return Some(prefix.len());
        }
    }
    None
}

#[cfg(not(tarpaulin_include))]
#[tracing::instrument(skip(ctx))]
#[poise::command(slash_command, prefix_command, owners_only)]
async fn register_commands_new(ctx: Context<'_>) -> Result<(), Error> {
    let commands = &ctx.framework().options().commands;
    poise::builtins::register_globally(ctx.http(), commands).await?;

    ctx.say("Successfully registered slash commands!").await?;
    Ok(())
}
