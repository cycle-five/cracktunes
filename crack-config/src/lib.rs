use colored::Colorize;
use crack_core::{
    commands,
    errors::CrackedError,
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::handle_event,
    handlers::SerenityHandler,
    is_prefix,
    metrics::COMMAND_ERRORS,
    utils::{
        check_interaction, check_reply, count_command, create_response_text, get_interaction_new,
    },
    BotConfig, Data, DataInner, Error, EventLog, PhoneCodeData,
};
use poise::serenity_prelude::{model::permissions::Permissions, Client, UserId};
use poise::{
    serenity_prelude::{FullEvent, GatewayIntents, GuildId},
    CreateReply,
};
use songbird::serenity::SerenityInit;
use std::sync::RwLock;
use std::{collections::HashMap, process::exit, sync::Arc, time::Duration};

/// on_error is called when an error occurs in the framework.
async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
        poise::FrameworkError::EventHandler { error, event, .. } => match event {
            FullEvent::PresenceUpdate { .. } => { /* Ignore PresenceUpdate in terminal logging, too spammy */
            }
            _ => {
                tracing::warn!(
                    "{} {} {} {}",
                    "In event handler for ".yellow(),
                    event.snake_case_name().yellow().italic(),
                    " event: ".yellow(),
                    error.to_string().yellow().bold(),
                );
            }
        },
        poise::FrameworkError::Command { error, ctx, .. } => {
            COMMAND_ERRORS
                .with_label_values(&[&ctx.command().qualified_name])
                .inc();
            match get_interaction_new(ctx) {
                Some(interaction) => {
                    check_interaction(
                        create_response_text(ctx, &interaction, &format!("{error}"))
                            .await
                            .map(|_| ())
                            .map_err(Into::into),
                    );
                }
                None => {
                    check_reply(
                        ctx.send(CreateReply::default().content(&format!("{error}")))
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
pub async fn poise_framework(
    config: BotConfig,
    //TODO: can this be create in this function instead of passed in?
    event_log: EventLog,
) -> Result<Client, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value

    tracing::warn!("Using prefix: {}", config.get_prefix());
    let up_prefix = config.get_prefix().to_ascii_uppercase();
    let up_prefix_cloned = Box::leak(Box::new(up_prefix.clone()));

    let options = poise::FrameworkOptions::<_, Error> {
        #[cfg(feature = "set_owners_from_config")]
        owners: config
            .owners
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .map(|id| UserId::new(*id))
            .collect(),
        commands: vec![
            commands::autopause(),
            commands::toggle_autoplay(),
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
            commands::resume(),
            // commands::repeat(),
            commands::servers(),
            commands::seek(),
            commands::skip(),
            commands::stop(),
            commands::shuffle(),
            commands::summon(),
            commands::version(),
            commands::volume(),
            // commands::voteskip(),
            commands::queue(),
            #[cfg(feature = "osint")]
            crack_osint::osint(),
            // all playlist commands
            commands::playlist(),
            // all admin commands
            commands::admin(),
            // all settings commands
            commands::settings(),
            // all gambling commands
            // commands::coinflip(),
            // commands::rolldice(),
            // commands::boop(),
            // all ai commands
            // commands::chatgpt(),
        ],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(config.get_prefix()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600)).into()),
            additional_prefixes: vec![poise::Prefix::Literal(up_prefix_cloned)],
            stripped_dynamic_prefix: Some(|_ctx, msg, data| {
                Box::pin(async move {
                    let guild_id = msg.guild_id.unwrap();
                    // let data_read = data.read().await;
                    // let guild_settings_map = data_read.get::<GuildSettingsMap>().unwrap();
                    let guild_settings_map = data.guild_settings_map.read().unwrap();

                    if let Some(guild_settings) = guild_settings_map.get(&guild_id) {
                        let prefixes = &guild_settings.additional_prefixes;
                        if prefixes.is_empty() {
                            tracing::warn!(
                                "Prefix is empty for guild {}",
                                guild_settings.guild_name
                            );
                            return Ok(None);
                        }

                        if let Some(prefix_len) = check_prefixes(prefixes, &msg.content) {
                            Ok(Some(msg.content.split_at(prefix_len)))
                        } else {
                            tracing::warn!("Prefix not found");
                            Ok(None)
                        }
                    } else {
                        tracing::warn!("Guild not found in guild settings map");
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
                tracing::info!(">>> {}...", ctx.command().qualified_name);
                count_command(ctx.command().qualified_name.as_ref(), is_prefix(ctx));
            })
        },
        // This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                tracing::info!("<<< {}!", ctx.command().qualified_name);
            })
        },
        // Every command invocation must pass this check to continue execution
        command_check: Some(|ctx| {
            Box::pin(async move {
                let command = ctx.command().qualified_name.clone();
                let lit_command = command.trim().to_string();
                let music_commands = vec![
                    "play",
                    "pause",
                    "resume",
                    "stop",
                    "skip",
                    "seek",
                    "summon",
                    "leave",
                    "lyrics",
                    "volume",
                    "now_playing",
                    "queue",
                    "repeat",
                    "shuffle",
                    "clear",
                    "remove",
                    "grab",
                    "playlist",
                    "voteskip",
                    "version",
                    "help",
                    "autopause",
                    "autoplay",
                ];
                // let mod_commands = vec!["settings", "admin"];
                tracing::info!("Checking command {}...", command);
                let user_id = ctx.author().id.get();
                let mod_command = command.eq("admin") && lit_command.contains("timeout");
                // If the physically running bot's owner is running the command, allow it
                if ctx
                    .data()
                    .bot_settings
                    .owners
                    .as_ref()
                    .unwrap_or(&vec![])
                    .contains(&user_id)
                {
                    return Ok(true);
                }
                // If the user is an admin on the server, allow the mod commands
                let res = ctx.author_member().await.map_or_else(
                    || {
                        tracing::info!("Author not found in guild");
                        Err(CrackedError::Other("Author not found in guild"))
                    },
                    |member| {
                        tracing::info!("Author found in guild");
                        let allow = member
                            .permissions(ctx)?
                            .contains(Permissions::ADMINISTRATOR)
                            && mod_command;
                        Ok(allow)
                    },
                );

                match res {
                    Ok(true) => {
                        tracing::info!("Author is admin");
                        return Ok(true);
                    }
                    Ok(false) => {
                        tracing::info!("Author is not admin");
                    }
                    Err(e) => {
                        tracing::error!("Error checking permissions: {}", e);
                        return Ok(false);
                    }
                };

                if music_commands.contains(&command.as_str()) {
                    return Ok(true);
                }

                //let user_id = ctx.author().id.as_u64();
                let guild_id = ctx.guild_id().unwrap_or_default();

                ctx.data()
                    .guild_settings_map
                    .read()
                    .unwrap()
                    .get(&guild_id)
                    .map_or_else(
                        || {
                            tracing::info!("Guild not found in guild settings map");
                            Ok(false)
                        },
                        |guild_settings| {
                            tracing::info!("Guild found in guild settings map");
                            Ok(guild_settings.authorized_users.is_empty()
                                || guild_settings.authorized_users.contains_key(&user_id))
                        },
                    )
            })
        }),
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: true,
        event_handler: |ctx, event, framework, data_global| {
            Box::pin(async move { handle_event(ctx, event, framework, data_global).await })
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
    let pool_opts = sqlx::postgres::PgPoolOptions::new().connect(&db_url).await;
    let cloned_map = guild_settings_map.clone();
    let data = Data(Arc::new(DataInner {
        phone_data: PhoneCodeData::load().unwrap(),
        bot_settings: config.clone(),
        guild_settings_map: Arc::new(RwLock::new(cloned_map)),
        event_log,
        database_pool: pool_opts.unwrap().into(),
        ..Default::default()
    }));

    let save_data = data.clone();

    let intents = GatewayIntents::non_privileged()
        | GatewayIntents::privileged()
        | GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_MODERATION
        | GatewayIntents::GUILD_EMOJIS_AND_STICKERS
        | GatewayIntents::GUILD_INTEGRATIONS
        | GatewayIntents::GUILD_WEBHOOKS
        | GatewayIntents::GUILD_INVITES
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_PRESENCES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MESSAGE_TYPING
        | GatewayIntents::GUILD_MESSAGE_REACTIONS
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::DIRECT_MESSAGE_TYPING
        | GatewayIntents::DIRECT_MESSAGE_REACTIONS
        | GatewayIntents::GUILD_SCHEDULED_EVENTS
        | GatewayIntents::AUTO_MODERATION_CONFIGURATION
        | GatewayIntents::AUTO_MODERATION_EXECUTION
        | GatewayIntents::MESSAGE_CONTENT;

    let handler_data = data.clone();
    let setup_data = data;
    let token = config
        .credentials
        .expect("Error getting discord token")
        .discord_token;
    let framework = poise::Framework::new(options, |ctx, ready, framework| {
        Box::pin(async move {
            tracing::info!("Logged in as {}", ready.user.name);
            poise::builtins::register_globally(ctx, &framework.options().commands).await?;
            ctx.data
                .write()
                .await
                .insert::<GuildSettingsMap>(guild_settings_map.clone());
            Ok(setup_data)
        })
    });
    let client = Client::builder(token, intents)
        .framework(framework)
        .register_songbird()
        .event_handler(SerenityHandler {
            is_loop_running: false.into(),
            data: handler_data,
        })
        .await
        .unwrap();
    let shard_manager = client.shard_manager.clone();

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
        {
            let guilds = save_data.guild_settings_map.read().unwrap().clone();
            for (k, v) in guilds {
                tracing::warn!("Saving Guild: {}", k);
                match v.save().await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    }
                }
            }
        }
        // .for_each(|(k, v)| {
        //     tracing::warn!("Saving Guild: {}", k);
        //     v.save().expect("Error saving guild settings");
        // });
        shard_manager.clone().shutdown_all().await;

        exit(0);
    });

    let shard_manager_2 = client.shard_manager.clone();
    tokio::spawn(async move {
        loop {
            let count = shard_manager_2.shards_instantiated().await.len();
            println!("Shard count instantiated: {}", count);

            tokio::time::sleep(Duration::from_millis(5000)).await;
        }
    });

    Ok(client)
}

fn check_prefixes(prefixes: &[String], content: &str) -> Option<usize> {
    for prefix in prefixes {
        if content.starts_with(prefix) {
            return Some(prefix.len());
        }
    }
    None
}

mod test {
    #[test]
    fn test_prefix() {
        let prefixes = vec!["crack ", "crack", "crack!"]
            .iter()
            .map(|&s| s.trim().to_string())
            .collect::<Vec<_>>();
        let content = "crack test";
        let prefix_len = super::check_prefixes(&prefixes, content).unwrap();
        assert_eq!(prefix_len, 5);
    }

    #[test]
    fn test_prefix_no_match() {
        let prefixes = vec!["crack ", "crack", "crack!"]
            .iter()
            .map(|&s| s.trim().to_string())
            .collect::<Vec<_>>();
        let content = "crac test";
        let prefix_len = super::check_prefixes(&prefixes, content);
        assert!(prefix_len.is_none());
    }
}
