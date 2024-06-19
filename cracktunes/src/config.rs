use colored::Colorize;
#[cfg(feature = "crack-metrics")]
use crack_core::metrics::COMMAND_ERRORS;
use crack_core::{
    commands::CacheHttpExt,
    db,
    errors::CrackedError,
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::{handle_event, SerenityHandler},
    utils::{check_reply, count_command},
    BotConfig, Data, DataInner, Error, EventLogAsync, PhoneCodeData,
};
use crack_core::{http_utils::SendMessageParams, messaging::message::CrackedMessage};
use poise::serenity_prelude::Client;
use poise::serenity_prelude::{FullEvent, GatewayIntents, GuildId};
use songbird::serenity::SerenityInit;
use std::{collections::HashMap, process::exit, sync::Arc, time::Duration};
use tokio::sync::RwLock;

use crack_core::poise_ext::PoiseContextExt;

/// on_error is called when an error occurs in the framework.
async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    // This is our custom error handler
    // They are many errors that can occur, so we only handle the ones we want to customize
    // and forward the rest to the default handler
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
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
            let myerr = CrackedError::Poise(error);
            let params = SendMessageParams::new(CrackedMessage::CrackedError(myerr));
            check_reply(ctx.send_message(params).await.map_err(Into::into));
            #[cfg(feature = "crack-metrics")]
            COMMAND_ERRORS
                .with_label_values(&[&ctx.command().qualified_name])
                .inc();
        },
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("Error while handling error: {}", e)
            }
        },
    }
}

// /// Check if the user is authorized to use the osint commands.
// fn is_authorized_osint(member: Option<Cow<'_, Member>>, os_int_role: Option<RoleId>) -> bool {
//     let member = match member {
//         Some(m) => m,
//         None => {
//             // FIXME: Why would this happen?
//             tracing::warn!("Member not found");
//             return true;
//         },
//     };
//     let perms = member.permissions.unwrap_or_default();
//     let has_role = os_int_role
//         .map(|x| member.roles.contains(x.as_ref()))
//         .unwrap_or(true);
//     let is_admin = perms.contains(Permissions::ADMINISTRATOR);

//     is_admin || has_role
// }

// /// Check if the user is authorized to use mod commands.
// fn is_authorized_mod(member: Option<Cow<'_, Member>>, roles: HashSet<u64>) -> bool {
//     // implementation of the is_authorized_mod function
//     // ...
//     is_authorized_admin(member, roles) // placeholder return value
// }

// /// Check if the user is authorized to use admin commands.
// fn is_authorized_admin(member: Option<Cow<'_, Member>>, roles: HashSet<u64>) -> bool {
//     let member = match member {
//         Some(m) => m,
//         None => {
//             tracing::warn!("No member found");
//             return false;
//         },
//     };
//     // implementation of the is_authorized_admin function
//     // ...
//     let perms = member.permissions.unwrap_or_default();
//     let _has_role = roles
//         .intersection(
//             &member
//                 .roles
//                 .iter()
//                 .map(|x| x.get())
//                 .collect::<HashSet<u64>>(),
//         )
//         .count()
//         > 0;
//     perms.contains(Permissions::ADMINISTRATOR)
// }

/// Create the poise framework from the bot config.
pub async fn poise_framework(
    config: BotConfig,
    //TODO: can this be create in this function instead of passed in?
    //event_log: EventLog,
    event_log_async: EventLogAsync,
) -> Result<Client, Error> {
    // FrameworkOptions contains all of poise's configuration option in one struct
    // Every option can be omitted to use its default value

    tracing::warn!("Using prefix: {}", config.get_prefix());
    let up_prefix = config.get_prefix().to_ascii_uppercase();
    // FIXME: Is this the proper way to allocate this memory?
    let up_prefix_cloned = Box::leak(Box::new(up_prefix.clone()));

    let commands = crack_core::commands::all_commands();
    let commands_str = commands
        .iter()
        .map(|x| x.qualified_name.as_str())
        .collect::<Vec<&str>>()
        .join(", ");

    tracing::warn!("Commands: {}", commands_str);

    let options = poise::FrameworkOptions::<_, Error> {
        // #[cfg(feature = "set_owners_from_config")]
        // owners: config
        //     .owners
        //     .as_ref()
        //     .unwrap_or(&vec![])
        //     .iter()
        //     .map(|id| UserId::new(*id))
        //     .collect(),
        commands,
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some(config.get_prefix()),
            edit_tracker: Some(poise::EditTracker::for_timespan(Duration::from_secs(3600)).into()),
            additional_prefixes: vec![poise::Prefix::Literal(up_prefix_cloned)],
            stripped_dynamic_prefix: Some(|_ctx, msg, data| {
                Box::pin(async move {
                    let guild_id = match msg.guild_id {
                        Some(id) => id,
                        None => {
                            tracing::warn!("No guild id found");
                            GuildId::new(1)
                        },
                    };
                    let guild_settings_map = data.guild_settings_map.read().await;

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
                count_command(ctx.command().qualified_name.as_ref(), ctx.is_prefix());
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
                let guild_id = ctx.guild_id();
                let name = match guild_id {
                    None => return Ok(true),
                    Some(guild_id) => ctx.guild_name_from_guild_id(guild_id).await,
                }
                .unwrap_or("Unknown".to_string());
                tracing::warn!("Guild: {}", name);
                Ok(true)
            })
        }),
        event_handler: |ctx, event, framework, data_global| {
            Box::pin(async move { handle_event(ctx, event, framework, data_global).await })
        },
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        initialize_owners: true,
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
    let database_pool = match sqlx::postgres::PgPoolOptions::new().connect(&db_url).await {
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
    // let rt = tokio::runtime::Builder::new_multi_thread()
    //     .enable_all()
    //     .build()
    //     .unwrap();
    // let handle = rt.handle();
    let cloned_map = guild_settings_map.clone();
    let data = Data(Arc::new(DataInner {
        phone_data: PhoneCodeData::load().unwrap(),
        bot_settings: config.clone(),
        guild_settings_map: Arc::new(RwLock::new(cloned_map)),
        event_log_async,
        database_pool,
        db_channel,
        ..Default::default()
    }));

    //let save_data = data.clone();

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

    let token = config
        .credentials
        .expect("Error getting discord token")
        .discord_token;
    let data2 = data.clone();
    // FIXME: Why can't we use framework.user_data() later in this function? (it hangs)
    let framework = poise::Framework::new(options, |ctx, ready, framework| {
        Box::pin(async move {
            tracing::info!("Logged in as {}", ready.user.name);
            poise::builtins::register_globally(&ctx, &framework.options().commands).await?;
            ctx.data
                .write()
                .await
                .insert::<GuildSettingsMap>(guild_settings_map.clone());
            Ok(data.clone())
        })
    });
    let serenity_handler = SerenityHandler {
        is_loop_running: false.into(),
        data: data2.clone(),
    };
    let client = Client::builder(token, intents)
        .framework(framework)
        .register_songbird()
        .event_handler(serenity_handler)
        .await
        .unwrap();
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

        if pool.is_some() {
            let p = pool.unwrap();
            for (k, v) in guilds {
                tracing::warn!("Saving Guild: {}", k);
                match v.save(&p).await {
                    Ok(_) => {},
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    },
                }
            }
        }

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
