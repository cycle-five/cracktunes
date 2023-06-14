use self::serenity::{
    async_trait,
    model::{
        application::command::{Command, CommandOptionType},
        application::interaction::{
            application_command::ApplicationCommandInteraction, Interaction,
        },
        gateway::Ready,
        id::GuildId,
        prelude::{Activity, ChannelId, VoiceState},
    },
    Mentionable, {Context, EventHandler},
};
use crate::{
    commands::{
        autopause::*, clear::*, grab::*, leave::*, manage_sources::*, now_playing::*, pause::*,
        play::*, queue::*, remove::*, repeat::*, resume::*, seek::*, shuffle::*, skip::*, stop::*,
        version::*, voteskip::*,
    },
    connection::{check_voice_connections, Connection},
    errors::ParrotError,
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::track_end::update_queue_messages,
    //poise_commands::volume::volume,
    sources::spotify::{Spotify, SPOTIFY},
    utils::create_response_text,
    Error,
};
use chrono::offset::Utc;
use poise::serenity_prelude as serenity;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

pub struct SerenityHandler {
    pub is_loop_running: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl EventHandler for SerenityHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        // attempts to authenticate to spotify
        *SPOTIFY.lock().await = Spotify::auth().await;

        // creates the global application commands
        //self.create_commands(&ctx).await;

        // loads serialized guild settings
        self.load_guilds_settings(&ctx, &ready).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::ApplicationCommand(mut command) = interaction else {
            return;
        };

        if let Err(err) = self.run_command(&ctx, &mut command).await {
            self.handle_error(&ctx, &mut command, ParrotError::Poise(err))
                .await
        }
    }

    async fn voice_state_update(&self, ctx: Context, _old: Option<VoiceState>, new: VoiceState) {
        // do nothing if this is a voice update event for a user, not a bot
        if new.user_id != ctx.cache.current_user_id() {
            return;
        }

        if new.channel_id.is_some() {
            return self.self_deafen(&ctx, new.guild_id, new).await;
        }

        let manager = songbird::get(&ctx).await.unwrap();
        let guild_id = new.guild_id.unwrap();

        if manager.get(guild_id).is_some() {
            manager.remove(guild_id).await.ok();
        }

        update_queue_messages(&ctx.http, &ctx.data, &[], guild_id).await;
    }

    // We use the cache_ready event just in case some cache operation is required in whatever use
    // case you have for this.
    async fn cache_ready(&self, ctx: Context, guilds: Vec<GuildId>) {
        tracing::info!("Cache built successfully!");

        // it's safe to clone Context, but Arc is cheaper for this use case.
        // Untested claim, just theoretically. :P
        let ctx = Arc::new(ctx);

        // We need to check that the loop is not already running when this event triggers,
        // as this event triggers every time the bot enters or leaves a guild, along every time the
        // ready shard event triggers.
        //
        // An AtomicBool is used because it doesn't require a mutable reference to be changed, as
        // we don't have one due to self being an immutable reference.
        if !self.is_loop_running.load(Ordering::Relaxed) {
            // We have to clone the Arc, as it gets moved into the new thread.
            let ctx1 = Arc::clone(&ctx);
            // tokio::spawn creates a new green thread that can run in parallel with the rest of
            // the application.
            tokio::spawn(async move {
                loop {
                    // We clone Context again here, because Arc is owned, so it moves to the
                    // new function.
                    log_system_load(Arc::clone(&ctx1)).await;
                    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
                }
            });

            // And of course, we can run more than one thread at different timings.
            let ctx2 = Arc::clone(&ctx);
            tokio::spawn(async move {
                loop {
                    set_status_to_current_time(Arc::clone(&ctx2)).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });

            let ctx3 = Arc::clone(&ctx);
            tokio::spawn(async move {
                loop {
                    // We clone Context again here, because Arc is owned, so it moves to the
                    // new function.
                    for guild_id in &guilds {
                        check_camera_status(Arc::clone(&ctx3), *guild_id).await;
                    }
                    tokio::time::sleep(Duration::from_secs(5 * 60)).await;
                }
            });

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

impl SerenityHandler {
    async fn _create_commands(&self, ctx: &Context) -> Vec<Command> {
        Command::set_global_application_commands(&ctx.http, |commands| {
            commands
                .create_application_command(|command| {
                    command
                        .name("grab")
                        .description("Grabs the current track, and has the bot DM it to you")
                })
                .create_application_command(|command| {
                    command
                        .name("volume")
                        .description("Get or set the volume")
                        .create_option(|option| {
                                option
                                    .name("percent")
                                    .description("The volume to set")
                                    .kind(CommandOptionType::Integer)
                                    .required(false)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("autopause")
                        .description("Toggles whether to pause after a song ends")
                })
                .create_application_command(|command| {
                    command
                        .name("clear")
                        .description("Clears the queue")
                })
                .create_application_command(|command| {
                    command
                        .name("leave")
                        .description("Leave the voice channel the bot is connected to")
                })
                .create_application_command(|command| {
                    command
                        .name("managesources")
                        .description("Manage streaming from different sources")
                })
                .create_application_command(|command| {
                    command
                        .name("np")
                        .description("Displays information about the current track")
                })
                .create_application_command(|command| {
                    command
                        .name("pause")
                        .description("Pauses the current track")
                })
                .create_application_command(|command| {
                    command
                        .name("play")
                        .description("Add a track to the queue")
                        .create_option(|option| {
                                option
                                    .name("query")
                                    .description("The media to play")
                                    .kind(CommandOptionType::String)
                                    .required(true)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("superplay")
                        .description("Add a track to the queue in a special way")
                        .create_option(|option| {
                            option
                                .name("next")
                                .description("Add a track to be played up next")
                                .kind(CommandOptionType::SubCommand)
                                .create_sub_option(|option| {
                                    option
                                        .name("query")
                                        .description("The media to play")
                                        .kind(CommandOptionType::String)
                                        .required(true)
                                })
                        })
                        .create_option(|option| {
                            option
                                .name("jump")
                                .description("Instantly plays a track, skipping the current one")
                                .kind(CommandOptionType::SubCommand)
                                .create_sub_option(|option| {
                                    option.name("query")
                                    .description("The media to play")
                                    .kind(CommandOptionType::String)
                                    .required(true)
                                })
                        })
                        .create_option(|option| {
                            option
                                .name("all")
                                .description("Add all tracks if the URL refers to a video and a playlist")
                                .kind(CommandOptionType::SubCommand)
                                .create_sub_option(|option| {
                                    option
                                        .name("query")
                                        .description("The media to play")
                                        .kind(CommandOptionType::String)
                                        .required(true)
                                })
                        })
                        .create_option(|option| {
                            option
                                .name("reverse")
                                .description("Add a playlist to the queue in reverse order")
                                .kind(CommandOptionType::SubCommand)
                                .create_sub_option(|option| {
                                    option
                                        .name("query")
                                        .description("The media to play")
                                        .kind(CommandOptionType::String)
                                        .required(true)
                                })
                        })
                        .create_option(|option| {
                            option
                                .name("shuffle")
                                .description("Add a playlist to the queue in random order")
                                .kind(CommandOptionType::SubCommand)
                                .create_sub_option(|option| {
                                    option
                                        .name("query")
                                        .description("The media to play")
                                        .kind(CommandOptionType::String)
                                        .required(true)
                                })
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("queue")
                        .description("Shows the queue")
                })
                .create_application_command(|command| {
                    command
                        .name("remove")
                        .description("Removes a track from the queue")
                        .create_option(|option| {
                            option
                                .name("index")
                                .description("Position of the track in the queue (1 is the next track to be played)")
                                .kind(CommandOptionType::Integer)
                                .required(true)
                                .min_int_value(1)
                        })
                        .create_option(|option| {
                            option
                                .name("until")
                                .description("Upper range track position to remove a range of tracks")
                                .kind(CommandOptionType::Integer)
                                .required(false)
                                .min_int_value(1)
                        })
                })
                .create_application_command(|command| {
                    command
                        .name("repeat")
                        .description("Toggles looping for the current track")
                })
                .create_application_command(|command| {
                    command
                        .name("resume")
                        .description("Resumes the current track")
                })
                .create_application_command(|command| {
                    command
                        .name("seek")
                        .description("Seeks current track to the given position")
                        .create_option(|option| {
                            option
                                .name("timestamp")
                                .description("Timestamp in the format HH:MM:SS")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
                .create_application_command(|command| {
                    command.name("shuffle").description("Shuffles the queue")
                })
                .create_application_command(|command| {
                    command.name("skip").description("Skips the current track")
                    .create_option(|option| {
                        option
                            .name("to")
                            .description("Track index to skip to")
                            .kind(CommandOptionType::Integer)
                            .required(false)
                            .min_int_value(1)
                    })
                })
                .create_application_command(|command| {
                    command
                        .name("stop")
                        .description("Stops the bot and clears the queue")
                })
                .create_application_command(|command| {
                    command
                        .name("summon")
                        .description("Summons the bot in your voice channel")
                })
                .create_application_command(|command| {
                    command
                        .name("version")
                        .description("Displays the current version")
                })
                .create_application_command(|command| {
                    command.name("voteskip").description("Starts a vote to skip the current track")
                })
        })
        .await
        .expect("failed to create command")
    }

    async fn load_guilds_settings(&self, ctx: &Context, ready: &Ready) {
        tracing::info!("Loading guilds' settings");
        let mut data = ctx.data.write().await;
        for guild in &ready.guilds {
            tracing::debug!("[DEBUG] Loading guild settings for {:?}", guild);
            let settings = data.get_mut::<GuildSettingsMap>().unwrap();

            let guild_settings = settings
                .entry(guild.id)
                .or_insert_with(|| GuildSettings::new(guild.id));

            if let Err(err) = guild_settings.load_if_exists() {
                tracing::error!(
                    "[ERROR] Failed to load guild {} settings due to {}",
                    guild.id,
                    err
                );
            }
        }
    }

    async fn run_command(
        &self,
        ctx: &Context,
        command: &mut ApplicationCommandInteraction,
    ) -> Result<(), Error> {
        let command_name = command.data.name.as_str();

        let guild_id = command.guild_id.unwrap();
        let guild = ctx.cache.guild(guild_id).unwrap();

        // get songbird voice client
        let manager = songbird::get(ctx).await.unwrap();

        // parrot might have been disconnected manually
        if let Some(call) = manager.get(guild.id) {
            let mut handler = call.lock().await;
            if handler.current_connection().is_none() {
                handler.leave().await.unwrap();
            }
        }

        // fetch the user and the bot's user IDs
        let user_id = command.user.id;
        let bot_id = ctx.cache.current_user_id();

        tracing::info!(
            "Running command {} for user {} in guild {}",
            command_name,
            user_id,
            guild_id
        );

        match command_name {
            "autopause" | "clear" | "leave" | "pause" | "remove" | "repeat" | "resume" | "seek"
            | "shuffle" | "skip" | "stop" | "voteskip" | "volume" | "grab" => {
                match check_voice_connections(&guild, &user_id, &bot_id) {
                    Connection::User(_) | Connection::Neither => Err(ParrotError::NotConnected),
                    Connection::Bot(bot_channel_id) => {
                        Err(ParrotError::AuthorDisconnected(bot_channel_id.mention()))
                    }
                    Connection::Separate(_, _) => Err(ParrotError::WrongVoiceChannel),
                    _ => Ok(()),
                }
            }
            "play" | "superplay" | "summon" => {
                match check_voice_connections(&guild, &user_id, &bot_id) {
                    Connection::User(_) => Ok(()),
                    Connection::Bot(_) if command_name == "summon" => {
                        Err(ParrotError::AuthorNotFound)
                    }
                    Connection::Bot(_) if command_name != "summon" => {
                        Err(ParrotError::WrongVoiceChannel)
                    }
                    Connection::Separate(bot_channel_id, _) => {
                        Err(ParrotError::AlreadyConnected(bot_channel_id.mention()))
                    }
                    Connection::Neither => Err(ParrotError::AuthorNotFound),
                    _ => Ok(()),
                }
            }
            "np" | "queue" => match check_voice_connections(&guild, &user_id, &bot_id) {
                Connection::User(_) | Connection::Neither => Err(ParrotError::NotConnected),
                _ => Ok(()),
            },
            _ => Ok(()),
        }?;

        match command_name {
            "grab" => grab(ctx, command).await,
            "volume" => {
                tracing::error!("volume not implemented here");
                Ok(())
            }
            "autopause" => autopause(ctx, command).await,
            "clear" => clear(ctx, command).await,
            "leave" => leave(ctx, command).await,
            "managesources" => allow(ctx, command).await,
            "np" => now_playing(ctx, command).await,
            "pause" => pause(ctx, command).await,
            "play" | "superplay" => play(ctx, command).await,
            "queue" => queue(ctx, command).await,
            "remove" => remove(ctx, command).await,
            "repeat" => repeat(ctx, command).await,
            "resume" => resume(ctx, command).await,
            "seek" => seek(ctx, command).await,
            "shuffle" => shuffle(ctx, command).await,
            "skip" => skip(ctx, command).await,
            "stop" => stop(ctx, command).await,
            "summon" => {
                tracing::error!("summon not implemented here");
                Ok(())
            }
            "version" => version(ctx, command).await,
            "voteskip" => voteskip(ctx, command).await,
            _ => unreachable!(),
        }
    }

    async fn self_deafen(&self, ctx: &Context, guild: Option<GuildId>, new: VoiceState) {
        let Ok(user) = ctx.http.get_current_user().await else {
            return;
        };

        if user.id == new.user_id && !new.deaf {
            guild
                .unwrap()
                .edit_member(&ctx.http, new.user_id, |n| n.deafen(true))
                .await
                .unwrap();
        }
    }

    async fn handle_error(
        &self,
        ctx: &Context,
        interaction: &mut ApplicationCommandInteraction,
        err: ParrotError,
    ) {
        create_response_text(&ctx.http, interaction, &format!("{err}"))
            .await
            .expect("failed to create response");
    }
}

async fn log_system_load(ctx: Arc<Context>) {
    let cpu_load = sys_info::loadavg().unwrap();
    let mem_use = sys_info::mem_info().unwrap();

    // We can use ChannelId directly to send a message to a specific channel; in this case, the
    // message would be sent to the #testing channel on the discord server.
    let message = ChannelId(1112603333182640158)
        .send_message(&ctx, |m| {
            m.embed(|e| {
                e.title("System Resource Load")
                    .field(
                        "CPU Load Average",
                        format!("{:.2}%", cpu_load.one * 10.0),
                        false,
                    )
                    .field(
                        "Memory Usage",
                        format!(
                            "{:.2} MB Free out of {:.2} MB",
                            mem_use.free as f32 / 1000.0,
                            mem_use.total as f32 / 1000.0
                        ),
                        false,
                    )
            })
        })
        .await;
    if let Err(why) = message {
        tracing::error!("Error sending message: {:?}", why);
    };
}

async fn set_status_to_current_time(ctx: Arc<Context>) {
    let current_time = Utc::now();
    let formatted_time = current_time.to_rfc2822();

    ctx.set_activity(Activity::playing(&formatted_time)).await;
}

async fn check_camera_status(ctx: Arc<Context>, guild_id: GuildId) {
    let guild = match guild_id.to_guild_cached(&ctx.cache) {
        Some(guild) => guild,
        None => {
            tracing::error!("Guild not found in cache");
            return;
        }
    };

    let voice_states = guild.voice_states;

    for (user_id, voice_state) in voice_states {
        if let Some(channel_id) = voice_state.channel_id {
            let camera_status = if voice_state.self_video { "on" } else { "off" };
            tracing::warn!(
                "User {} is connected to voice channel {} with camera {}",
                user_id,
                channel_id,
                camera_status
            );
        }
    }
}
