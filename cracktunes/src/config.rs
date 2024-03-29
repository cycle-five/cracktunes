use colored::Colorize;
#[cfg(feature = "crack-metrics")]
use crack_core::metrics::COMMAND_ERRORS;
use crack_core::{
    commands,
    errors::CrackedError,
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::handle_event,
    handlers::SerenityHandler,
    is_prefix,
    utils::{
        check_interaction, check_reply, count_command, create_response_text, get_interaction_new,
    },
    BotConfig, Data, DataInner, Error, EventLog, PhoneCodeData,
};
use poise::serenity_prelude::{model::permissions::Permissions, Client};
use poise::{
    serenity_prelude::{FullEvent, GatewayIntents, GuildId},
    CreateReply,
};
use songbird::serenity::SerenityInit;
use std::sync::RwLock;
use std::{
    collections::{HashMap, HashSet},
    process::exit,
    sync::Arc,
    time::Duration,
};

use crate::{
    get_admin_commands_hashset, get_mod_commands, get_music_commands, get_osint_commands,
    get_playlist_commands,
};

#[derive(Debug, Clone)]
pub struct CommandCategories {
    mod_command: bool,
    admin_command: bool,
    music_command: bool,
    osint_command: bool,
    playlist_command: bool,
}

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
            #[cfg(feature = "crack-metrics")]
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

/// Check if the user is authorized to use the osint commands.
fn is_authorized_osint() -> bool {
    // implementation of the is_authorized_osint function
    // ...
    true // placeholder return value
}

/// Check if the user is authorized to use the music commands.
fn is_authorized_music() -> bool {
    // implementation of the is_authorized_music function
    // ...
    true // placeholder return value
}

/// Check if the user is authorized to use mod commands.
fn is_authorized_mod() -> bool {
    // implementation of the is_authorized_mod function
    // ...
    false // placeholder return value
}

/// Check if the user is authorized to use admin commands.
fn is_authorized_admin() -> bool {
    // implementation of the is_authorized_admin function
    // ...
    false // placeholder return value
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
    // FIXME: Is this the proper way to allocate this memory?
    let up_prefix_cloned = Box::leak(Box::new(up_prefix.clone()));

    let options = poise::FrameworkOptions::<_, Error> {
        // #[cfg(feature = "set_owners_from_config")]
        // owners: config
        //     .owners
        //     .as_ref()
        //     .unwrap_or(&vec![])
        //     .iter()
        //     .map(|id| UserId::new(*id))
        //     .collect(),
        commands: vec![
            commands::autopause(),
            commands::autoplay(),
            commands::clear(),
            commands::clean(),
            commands::downvote(),
            commands::help(),
            commands::leave(),
            commands::lyrics(),
            commands::grab(),
            commands::nowplaying(),
            commands::pause(),
            commands::optplay(),
            commands::play(),
            commands::playnext(),
            commands::playlog(),
            commands::ping(),
            commands::remove(),
            commands::resume(),
            commands::repeat(),
            commands::search(),
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
            #[cfg(feature = "crack-osint")]
            commands::osint(),
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
                    let guild_id = match msg.guild_id {
                        Some(id) => id,
                        None => {
                            tracing::warn!("No guild id found");
                            GuildId::new(1)
                        }
                    };
                    let guild_settings_map = data.guild_settings_map.read().unwrap();

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
                let command = &ctx.command().qualified_name;
                let user_id = ctx.author().id.get();
                let _ = is_authorized_mod();
                let _ = is_authorized_admin();

                let cmd_cats = check_command_categories(command.clone());
                tracing::info!("Command: {:?}", command);
                tracing::info!("Command Categories: {:?}", cmd_cats);
                let CommandCategories {
                    mod_command,
                    admin_command,
                    music_command,
                    osint_command,
                    playlist_command,
                } = cmd_cats;

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
                        let perms = member.permissions(ctx)?;
                        tracing::warn!("perm {}", perms);
                        let is_admin = perms.contains(Permissions::ADMINISTRATOR);
                        tracing::warn!("is_admin: {}", is_admin);
                        Ok(is_admin && (mod_command || admin_command))
                    },
                );

                // FIXME: This is a bit of a hack
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

                // These need to be processed in order of most to least restrictive

                if osint_command {
                    return Ok(is_authorized_osint());
                }

                if music_command || playlist_command {
                    return Ok(is_authorized_music());
                }

                // Default case fail to be on the safe side.
                Ok(false)

                // //let user_id = ctx.author().id.as_u64();
                // // let guild_id = ctx.guild_id().unwrap_or_default();

                // ctx.data()
                //     .guild_settings_map
                //     .read()
                //     .unwrap()
                //     .get(&guild_id)
                //     .map_or_else(
                //         || {
                //             tracing::info!("Guild not found in guild settings map");
                //             Ok(false)
                //         },
                //         |guild_settings| {
                //             tracing::info!("Guild found in guild settings map");
                //             Ok(guild_settings.authorized_users.is_empty()
                //                 || guild_settings.authorized_users.contains_key(&user_id))
                //         },
                //     )
            })
        }),
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
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
        database_pool: pool_opts.ok(),
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
            let pool = save_data
                .database_pool
                .clone()
                .ok_or({
                    tracing::error!("No database pool");
                    CrackedError::Other("No database pool")
                })
                .unwrap();
            for (k, v) in guilds {
                tracing::warn!("Saving Guild: {}", k);
                match v.save(&pool).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Error saving guild settings: {}", e);
                    }
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
/// Checks what categories the given command belongs to.
// TODO: Use the build it categories?!?
// TODO: Create a command struct with the categories info
//     to parse this info.
fn check_command_categories(user_cmd: String) -> CommandCategories {
    // FIXME: Make these constants
    let music_commands = get_music_commands();
    let playlist_commands = get_playlist_commands();
    let osint_commands = get_osint_commands();
    let mod_commands: HashMap<&str, Vec<&str>> = get_mod_commands().into_iter().collect();
    let admin_commands: HashSet<&'static str> = get_admin_commands_hashset();

    let clean_cmd = user_cmd.clone().trim().to_string();
    let first = clean_cmd.split_whitespace().next().unwrap_or_default();
    let second = clean_cmd.split_whitespace().nth(1);

    let mut mod_command = false;
    for cmd in mod_commands.keys() {
        if cmd.eq(&first)
            && second.is_some()
            && mod_commands.get(cmd).unwrap().contains(&second.unwrap())
        {
            mod_command = true;
            break;
        }
    }

    let admin_command = "admin".eq(first) && admin_commands.contains(&second.unwrap_or_default());

    let music_command = music_commands.contains(&first);

    let osint_command = "osint".eq(first)
        && (second.is_none() || osint_commands.contains(&second.unwrap_or_default()));

    let playlist_command = ("playlist".eq(first) || "pl".eq(first))
        && (second.is_none() || playlist_commands.contains(&second.unwrap_or_default()));

    CommandCategories {
        mod_command,
        admin_command,
        music_command,
        osint_command,
        playlist_command,
    }
}

#[cfg(test)]
mod test {
    use crack_core::{BotConfig, EventLog};

    use crate::config::CommandCategories;

    #[test]
    fn test_command_categories() {
        let cmd = CommandCategories {
            mod_command: true,
            admin_command: false,
            music_command: false,
            osint_command: false,
            playlist_command: false,
        };
        assert_eq!(cmd.mod_command, true);
        assert_eq!(cmd.admin_command, false);
        assert_eq!(cmd.music_command, false);
        assert_eq!(cmd.osint_command, false);
        assert_eq!(cmd.playlist_command, false);
    }

    #[tokio::test]
    async fn test_build_framework() {
        let config = BotConfig::default();
        let event_log = EventLog::new();
        let client = super::poise_framework(config, event_log).await;
        assert!(client.is_ok());
    }

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

    #[test]
    fn test_check_command_categories_bad_command() {
        let CommandCategories {
            mod_command,
            admin_command,
            music_command,
            osint_command,
            playlist_command,
        } = super::check_command_categories("admin settings".to_owned());
        assert_eq!(mod_command, false);
        assert_eq!(admin_command, false);
        assert_eq!(music_command, false);
        assert_eq!(osint_command, false);
        assert_eq!(playlist_command, false);
    }

    #[test]
    fn test_check_command_categories_music_command() {
        let CommandCategories {
            mod_command,
            admin_command,
            music_command,
            osint_command,
            ..
        } = super::check_command_categories("play".to_owned());
        assert_eq!(mod_command, false);
        assert_eq!(admin_command, false);
        assert_eq!(music_command, true);
        assert_eq!(osint_command, false);
    }

    #[test]
    fn test_check_command_categories_settings_command() {
        let CommandCategories {
            mod_command,
            admin_command,
            music_command,
            osint_command,
            ..
        } = super::check_command_categories("settings get all".to_owned());
        assert_eq!(mod_command, true);
        assert_eq!(admin_command, false);
        assert_eq!(music_command, false);
        assert_eq!(osint_command, false);
    }

    #[test]
    fn test_check_command_categories_admin_command() {
        let CommandCategories {
            mod_command,
            admin_command,
            music_command,
            osint_command,
            ..
        } = super::check_command_categories("admin ban".to_owned());
        assert_eq!(mod_command, true);
        assert_eq!(admin_command, true);
        assert_eq!(music_command, false);
        assert_eq!(osint_command, false);
    }
}

// impl CommandCategories {
//     pub fn is_mod_command(&self) -> bool {
//         self.mod_command
//     }

//     pub fn is_admin_command(&self) -> bool {
//         self.admin_command
//     }

//     pub fn is_music_command(&self) -> bool {
//         self.music_command
//     }

//     pub fn is_osint_command(&self) -> bool {
//         self.osint_command
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_authorized_defaults() {
        // Just check the default return values of the authorization functions.
        assert_eq!(is_authorized_osint(), true);
        assert_eq!(is_authorized_music(), true);
        assert_eq!(is_authorized_mod(), false);
        assert_eq!(is_authorized_admin(), false);
    }
}
