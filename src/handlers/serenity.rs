use self::serenity::{
    async_trait,
    model::{
        gateway::Ready,
        id::GuildId,
        prelude::{Activity, ChannelId, VoiceState},
    },
    {Context as SerenityContext, EventHandler},
};
use crate::{
    guild::settings::{GuildSettings, GuildSettingsMap},
    handlers::track_end::update_queue_messages,
    sources::spotify::{Spotify, SPOTIFY},
    BotConfig, Data,
};
use chrono::offset::Utc;
use poise::serenity_prelude::{self as serenity, Channel, UserId};
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc},
};
use tokio::time::{Duration, Instant};

pub struct SerenityHandler {
    pub data: Data,
    pub is_loop_running: std::sync::atomic::AtomicBool,
    // pub config: Arc<BotConfig>,
}

#[async_trait]
impl EventHandler for SerenityHandler {
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        // attempts to authenticate to spotify
        *SPOTIFY.lock().await = Spotify::auth().await;

        // creates the global application commands
        //self.create_commands(&ctx).await;

        // loads serialized guild settings
        self.load_guilds_settings(&ctx, &ready).await;
    }

    // async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
    //     let Interaction::ApplicationCommand(mut command) = interaction else {
    //         return;
    //     };

    //     if let Err(err) = self.run_command(&ctx, &mut command).await {
    //         self.handle_error(&ctx, &mut command, CrackedError::Poise(err))
    //             .await
    //     }
    // }

    async fn voice_state_update(
        &self,
        ctx: SerenityContext,
        _old: Option<VoiceState>,
        new: VoiceState,
    ) {
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
    async fn cache_ready(&self, ctx: SerenityContext, guilds: Vec<GuildId>) {
        tracing::info!("Cache built successfully!");

        // it's safe to clone Context, but Arc is cheaper for this use case.
        // Untested claim, just theoretically. :P
        let ctx = Arc::new(ctx);
        // let config = Arc::clone(&self.config);
        let config = self.data.bot_settings.clone();

        // let shard_manager = (*self.shard_manager.lock().unwrap()).clone().unwrap();
        // let framework_data = poise::FrameworkContext {
        //     bot_id: serenity::UserId(846453852164587620),
        //     options: &self.options,
        //     user_data: &(),
        //     shard_manager: &shard_manager,
        // };

        // We need to check that the loop is not already running when this event triggers,
        // as this event triggers every time the bot enters or leaves a guild, along every time the
        // ready shard event triggers.
        //
        // An AtomicBool is used because it doesn't require a mutable reference to be changed, as
        // we don't have one due to self being an immutable reference.
        if !self.is_loop_running.load(Ordering::Relaxed)
            && config.video_status_poll_interval.is_some()
        {
            // We have to clone the Arc, as it gets moved into the new thread.
            let ctx1 = Arc::clone(&ctx);
            // tokio::spawn creates a new green thread that can run in parallel with the rest of
            // the application.
            tokio::spawn(async move {
                loop {
                    // We clone Context again here, because Arc is owned, so it moves to the
                    // new function.
                    log_system_load(ctx1.clone(), config.clone()).await;
                    tokio::time::sleep(config.video_status_poll_interval.unwrap()).await;
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
            let config = self.data.bot_settings.clone();
            tokio::spawn(async move {
                let conf_guilds = config
                    .cam_kick
                    .iter()
                    .map(|x| x.guild_id)
                    .collect::<HashSet<_>>();
                let mut cam_status = HashMap::<UserId, (String, Instant)>::new();
                loop {
                    // We clone Context again here, because Arc is owned, so it moves to the
                    // new function.
                    // let new_cam_status = Arc::new(HashMap::<UserId, String>::new());
                    let mut cams = vec![];
                    for guild_id in &guilds {
                        if conf_guilds.contains(guild_id) {
                            cams.extend(check_camera_status(Arc::clone(&ctx3), *guild_id).await);
                        }
                    }

                    for cam in cams.iter() {
                        if let Some(status) = cam_status.get(&(*cam).0) {
                            if status.0 != *&(*cam).1 {
                                tracing::info!(
                                    "Camera status changed for user {} to {}",
                                    status.0,
                                    cam.1
                                );
                                cam_status.insert((*cam).0, ((*cam).1.clone(), (*cam).2.clone()));
                            } else if status.0 == "off"
                                && status.1.elapsed() > Duration::from_secs(20)
                            {
                                let user = (*cam).0.to_user(&ctx3.http).await.unwrap();
                                tracing::warn!(
                                    "User {} has been cammed down for {} seconds",
                                    user.name,
                                    status.1.elapsed().as_secs()
                                );
                                cam_status.insert((*cam).0, ((*cam).1.clone(), status.1.clone()));
                            }
                        }
                    }
                    cam_status = cams
                        .iter()
                        .map(|x| (x.0, (x.1.clone(), x.2)))
                        .collect::<HashMap<_, (_, _)>>();
                    tokio::time::sleep(Duration::from_secs(1 * config.cam_kick[0].poll_secs)).await;
                }
            });

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

impl SerenityHandler {
    // async fn _create_commands(&self, ctx: &Context) -> Vec<Command> {
    //     Command::set_global_application_commands(&ctx.http, |commands| {
    //         commands
    //             .create_application_command(|command| {
    //                 command
    //                     .name("grab")
    //                     .description("Grabs the current track, and has the bot DM it to you")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("volume")
    //                     .description("Get or set the volume")
    //                     .create_option(|option| {
    //                             option
    //                                 .name("percent")
    //                                 .description("The volume to set")
    //                                 .kind(CommandOptionType::Integer)
    //                                 .required(false)
    //                     })
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("autopause")
    //                     .description("Toggles whether to pause after a song ends")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("clear")
    //                     .description("Clears the queue")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("leave")
    //                     .description("Leave the voice channel the bot is connected to")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("managesources")
    //                     .description("Manage streaming from different sources")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("np")
    //                     .description("Displays information about the current track")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("pause")
    //                     .description("Pauses the current track")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("play")
    //                     .description("Add a track to the queue")
    //                     .create_option(|option| {
    //                             option
    //                                 .name("query")
    //                                 .description("The media to play")
    //                                 .kind(CommandOptionType::String)
    //                                 .required(true)
    //                     })
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("superplay")
    //                     .description("Add a track to the queue in a special way")
    //                     .create_option(|option| {
    //                         option
    //                             .name("next")
    //                             .description("Add a track to be played up next")
    //                             .kind(CommandOptionType::SubCommand)
    //                             .create_sub_option(|option| {
    //                                 option
    //                                     .name("query")
    //                                     .description("The media to play")
    //                                     .kind(CommandOptionType::String)
    //                                     .required(true)
    //                             })
    //                     })
    //                     .create_option(|option| {
    //                         option
    //                             .name("jump")
    //                             .description("Instantly plays a track, skipping the current one")
    //                             .kind(CommandOptionType::SubCommand)
    //                             .create_sub_option(|option| {
    //                                 option.name("query")
    //                                 .description("The media to play")
    //                                 .kind(CommandOptionType::String)
    //                                 .required(true)
    //                             })
    //                     })
    //                     .create_option(|option| {
    //                         option
    //                             .name("all")
    //                             .description("Add all tracks if the URL refers to a video and a playlist")
    //                             .kind(CommandOptionType::SubCommand)
    //                             .create_sub_option(|option| {
    //                                 option
    //                                     .name("query")
    //                                     .description("The media to play")
    //                                     .kind(CommandOptionType::String)
    //                                     .required(true)
    //                             })
    //                     })
    //                     .create_option(|option| {
    //                         option
    //                             .name("reverse")
    //                             .description("Add a playlist to the queue in reverse order")
    //                             .kind(CommandOptionType::SubCommand)
    //                             .create_sub_option(|option| {
    //                                 option
    //                                     .name("query")
    //                                     .description("The media to play")
    //                                     .kind(CommandOptionType::String)
    //                                     .required(true)
    //                             })
    //                     })
    //                     .create_option(|option| {
    //                         option
    //                             .name("shuffle")
    //                             .description("Add a playlist to the queue in random order")
    //                             .kind(CommandOptionType::SubCommand)
    //                             .create_sub_option(|option| {
    //                                 option
    //                                     .name("query")
    //                                     .description("The media to play")
    //                                     .kind(CommandOptionType::String)
    //                                     .required(true)
    //                             })
    //                     })
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("queue")
    //                     .description("Shows the queue")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("remove")
    //                     .description("Removes a track from the queue")
    //                     .create_option(|option| {
    //                         option
    //                             .name("index")
    //                             .description("Position of the track in the queue (1 is the next track to be played)")
    //                             .kind(CommandOptionType::Integer)
    //                             .required(true)
    //                             .min_int_value(1)
    //                     })
    //                     .create_option(|option| {
    //                         option
    //                             .name("until")
    //                             .description("Upper range track position to remove a range of tracks")
    //                             .kind(CommandOptionType::Integer)
    //                             .required(false)
    //                             .min_int_value(1)
    //                     })
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("repeat")
    //                     .description("Toggles looping for the current track")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("resume")
    //                     .description("Resumes the current track")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("seek")
    //                     .description("Seeks current track to the given position")
    //                     .create_option(|option| {
    //                         option
    //                             .name("timestamp")
    //                             .description("Timestamp in the format HH:MM:SS")
    //                             .kind(CommandOptionType::String)
    //                             .required(true)
    //                     })
    //             })
    //             .create_application_command(|command| {
    //                 command.name("shuffle").description("Shuffles the queue")
    //             })
    //             .create_application_command(|command| {
    //                 command.name("skip").description("Skips the current track")
    //                 .create_option(|option| {
    //                     option
    //                         .name("to")
    //                         .description("Track index to skip to")
    //                         .kind(CommandOptionType::Integer)
    //                         .required(false)
    //                         .min_int_value(1)
    //                 })
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("stop")
    //                     .description("Stops the bot and clears the queue")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("summon")
    //                     .description("Summons the bot in your voice channel")
    //             })
    //             .create_application_command(|command| {
    //                 command
    //                     .name("version")
    //                     .description("Displays the current version")
    //             })
    //             .create_application_command(|command| {
    //                 command.name("voteskip").description("Starts a vote to skip the current track")
    //             })
    //     })
    //     .await
    //     .expect("failed to create command")
    // }

    async fn load_guilds_settings(&self, ctx: &SerenityContext, ready: &Ready) {
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

    async fn self_deafen(&self, ctx: &SerenityContext, guild: Option<GuildId>, new: VoiceState) {
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

    // async fn handle_error(
    //     &self,
    //     ctx: &Context,
    //     interaction: &mut ApplicationCommandInteraction,
    //     err: CrackedError,
    // ) {
    //     create_response_text(&ctx.http, interaction, &format!("{err}"))
    //         .await
    //         .expect("failed to create response");
    // }
}

async fn log_system_load(ctx: Arc<SerenityContext>, config: BotConfig) {
    let cpu_load = sys_info::loadavg().unwrap();
    let mem_use = sys_info::mem_info().unwrap();

    // We can use ChannelId directly to send a message to a specific channel; in this case, the
    // message would be sent to the #testing channel on the discord server.
    if let Some(chan_id) = config.sys_log_channel_id {
        let message = ChannelId(chan_id)
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
    } else {
        tracing::error!("No system log channel set");
    }
}

async fn set_status_to_current_time(ctx: Arc<SerenityContext>) {
    let current_time = Utc::now();
    let formatted_time = current_time.to_rfc2822();

    ctx.set_activity(Activity::playing(&formatted_time)).await;
}

async fn check_camera_status(
    ctx: Arc<SerenityContext>,
    guild_id: GuildId,
) -> Vec<(UserId, String, Instant)> {
    let guild = match guild_id.to_guild_cached(&ctx.cache) {
        Some(guild) => guild,
        None => {
            tracing::error!("Guild not found in cache");
            return vec![];
        }
    };

    let voice_states = guild.voice_states;
    let mut cams = vec![];

    for (user_id, voice_state) in voice_states {
        if let Some(channel_id) = voice_state.channel_id {
            let camera_status = if voice_state.self_video { "on" } else { "off" };
            let user = user_id.to_user(&ctx.http).await.unwrap();
            let channel = channel_id.to_channel(&ctx.http).await.unwrap();
            cams.push((user.id, camera_status.to_string(), Instant::now()));
            tracing::warn!(
                "User {} / {} is connected to voice channel {} / {} with camera {}",
                user.name,
                user.id,
                match channel {
                    Channel::Guild(channel) => channel.name,
                    Channel::Private(channel) => channel.name(),
                    Channel::Category(channel) => channel.name,
                    _ => String::from("unknown"),
                },
                channel_id,
                camera_status,
            );
        }
    }
    return cams;
}
