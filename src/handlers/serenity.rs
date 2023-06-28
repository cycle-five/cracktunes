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
    BotConfig, CamKickConfig, Data,
};
use chrono::offset::Utc;
use poise::serenity_prelude::{self as serenity, Channel, Guild, Member, SerenityError, UserId};
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc, Mutex},
};
use tokio::time::{Duration, Instant};

pub struct SerenityHandler {
    pub data: Arc<Data>,
    pub is_loop_running: std::sync::atomic::AtomicBool,
    // pub config: Arc<BotConfig>,
}

use poise::serenity_prelude::Mentionable;
#[derive(Copy, Clone, Debug)]
pub struct MyVoiceUserInfo {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    // true = on, false = off
    pub camera_status: bool,
    pub time_last_cam_change: Instant,
}

struct ChanCacheValue {
    #[allow(dead_code)]
    pub name: String,
}

#[async_trait]
impl EventHandler for SerenityHandler {
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        // attempts to authenticate to spotify
        *SPOTIFY.lock().await = Spotify::auth().await;

        // creates the global application commands
        //self.create_commands(&ctx).await;
        //self.data.bot_settings = BotConfig::from_config_file("cracktunes.json").unwrap();

        // loads serialized guild settings
        tracing::warn!("Loading guilds' settings");
        self.load_guilds_settings(&ctx, &ready).await;

        // These are the guild settings defined in the config file.
        // Should they always override the ones in the database?
        tracing::warn!("Merging guilds' settings");
        self.merge_guild_settings(&ctx, &ready, self.data.guild_settings_map.clone())
            .await;

        self.data
            .guild_settings_map
            .lock()
            .unwrap()
            .iter()
            .for_each(|(k, v)| {
                tracing::warn!("Saving Guild: {}", k);
                v.save().expect("Error saving guild settings");
            });
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

    async fn message(&self, ctx: SerenityContext, msg: serenity::Message) {
        // let user_id = msg.author.id;
        let guild_id = match msg.guild_id {
            Some(guild_id) => guild_id,
            None => {
                tracing::warn!("Non-gateway message received: {:?}", msg);
                GuildId(0)
                // return
            }
        };

        if guild_id.0 != 0 {
            let guild = guild_id.to_guild_cached(&ctx.cache).unwrap();
            let name = msg.author.name.clone();
            let guild_name = guild.name;
            let content = msg.content.clone();
            let channel_name = msg.channel_id.name(&ctx).await.unwrap();
            tracing::info!(
                "Message: {} / {} / {} / {}",
                name,
                guild_name,
                channel_name,
                content
            );
        }

        let serde_msg = serde_json::to_string(&msg).unwrap();
        tracing::trace!(target = "events_serde", "serde_msg: {}", serde_msg);
    }

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
        tracing::info!("Cache built successfully! {} guilds cached", guilds.len());

        let _channel_cache = HashMap::<u64, ChanCacheValue>::new();

        for guildid in guilds.iter() {
            tracing::info!("Guild: {:?}", guildid);
        }

        let config = self.data.bot_settings.clone();
        // it's safe to clone Context, but Arc is cheaper for this use case.
        // Untested claim, just theoretically. :P
        let ctx = Arc::new(ctx);
        let config = Arc::new(config);
        // let config = Arc::clone(&self.config);

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
        if !self.is_loop_running.load(Ordering::Relaxed) {
            // We have to clone the Arc, as it gets moved into the new thread.
            let ctx1 = Arc::clone(&ctx);
            let config1 = Arc::clone(&config);
            // tokio::spawn creates a new green thread that can run in parallel with the rest of
            // the application.
            if false {
                tokio::spawn(async move {
                    loop {
                        // We clone Context again here, because Arc is owned, so it moves to the
                        // new function.
                        log_system_load(ctx1.clone(), config1.clone()).await;
                        tokio::time::sleep(Duration::from_secs(config1.video_status_poll_interval))
                            .await;
                    }
                });
            }

            // And of course, we can run more than one thread at different timings.
            let ctx2 = Arc::clone(&ctx);
            tokio::spawn(async move {
                loop {
                    set_status_to_current_time(Arc::clone(&ctx2)).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });

            let ctx3 = Arc::clone(&ctx);

            if config.video_status_poll_interval > 0 {
                cam_status_loop(ctx3, config.clone(), guilds.clone()).await;
            }

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

impl SerenityHandler {
    async fn merge_guild_settings(
        &self,
        ctx: &SerenityContext,
        _ready: &Ready,
        new_settings: Arc<Mutex<HashMap<u64, GuildSettings>>>,
    ) {
        tracing::warn!("in merge_guild_settings");
        let mut data = ctx.data.write().await;

        let settings = data.get_mut::<GuildSettingsMap>().unwrap();
        let new_settings = new_settings.lock().unwrap();

        tracing::warn!("new_settings len: {:?}", new_settings.len());

        for (key, value) in new_settings.iter() {
            match settings.insert(GuildId(*key), value.clone()) {
                Some(_) => tracing::info!("Guild {} settings overwritten", key),
                None => tracing::info!("Guild {} settings did not exist", key),
            }
        }
    }

    async fn load_guilds_settings(&self, ctx: &SerenityContext, ready: &Ready) {
        tracing::info!("Loading guilds' settings");
        let mut data = ctx.data.write().await;
        for guild in &ready.guilds {
            tracing::info!("Loading guild settings for {:?}", guild);
            let settings = data.get_mut::<GuildSettingsMap>().unwrap();

            let guild_settings = settings
                .entry(guild.id)
                .or_insert_with(|| GuildSettings::new(guild.id));

            if let Err(err) = guild_settings.load_if_exists() {
                tracing::error!("Failed to load guild {} settings due to {}", guild.id, err);
            }
        }
    }

    async fn self_deafen(&self, ctx: &SerenityContext, guild: Option<GuildId>, new: VoiceState) {
        if self.data.bot_settings.self_deafen {
            return;
        }

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
}

async fn log_system_load(ctx: Arc<SerenityContext>, config: Arc<BotConfig>) {
    let cpu_load = sys_info::loadavg().unwrap();
    let mem_use = sys_info::mem_info().unwrap();

    // We can use ChannelId directly to send a message to a specific channel; in this case, the
    // message would be sent to the #testing channel on the discord server.
    if config.sys_log_channel_id > 0 {
        let chan_id = config.sys_log_channel_id;
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
    // channel_cache: &HashMap<u64, ChanCacheValue>,
    // user_cache: &HashMap<u64, String>,
) -> Vec<MyVoiceUserInfo> {
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
            let user = match user_id.to_user(&ctx.http).await {
                Ok(user) => user,
                Err(err) => {
                    tracing::error!("Error getting user: {}", err);
                    continue;
                }
            };
            let channel = channel_id.to_channel(&ctx.http).await.unwrap();

            let info = MyVoiceUserInfo {
                user_id,
                guild_id,
                channel_id,
                camera_status: voice_state.self_video,
                time_last_cam_change: Instant::now(),
            };

            cams.push(info);
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
                if info.camera_status { "on" } else { "off" },
            );
        }
    }
    cams
}

async fn cam_status_loop(ctx: Arc<SerenityContext>, config: Arc<BotConfig>, guilds: Vec<GuildId>) {
    tokio::spawn(async move {
        tracing::trace!(
            target = "cam_status_loop",
            "Starting camera status check loop"
        );
        let conf_guilds = config
            .cam_kick
            .iter()
            .map(|x| x.guild_id)
            .collect::<HashSet<_>>();
        let mut cam_status: HashMap<UserId, MyVoiceUserInfo> =
            HashMap::<UserId, MyVoiceUserInfo>::new();
        let channels: HashMap<u64, &CamKickConfig> = config
            .cam_kick
            .iter()
            .map(|x| (x.channel_id, x))
            .collect::<HashMap<_, _>>();
        conf_guilds
            .iter()
            .for_each(|x| tracing::error!("Guild: {}", x));
        loop {
            // We clone Context again here, because Arc is owned, so it moves to the
            // new function.
            // let new_cam_status = Arc::new(HashMap::<UserId, String>::new());
            tracing::error!("Checking camera status for {} guilds", guilds.len());
            // Go through all the guilds we have cached and check the camera status
            // for all the users we can see in voice channels.
            let mut cams = vec![];
            for guild_id in &guilds {
                cams.extend(check_camera_status(Arc::clone(&ctx), *guild_id).await);
            }
            tracing::trace!("num cams {}", cams.len());
            let mut new_cams = vec![];

            for cam in cams.iter() {
                if let Some(status) = cam_status.get(&cam.user_id) {
                    tracing::error!("Status checking {:?}", status);
                    if let Some(kick_conf) = channels.get(&status.channel_id.0) {
                        if status.camera_status != cam.camera_status {
                            tracing::info!(
                                "Camera status changed for user {} to {}",
                                status.user_id,
                                cam.camera_status
                            );
                            cam_status.insert(cam.user_id, *cam);
                        } else {
                            tracing::info!(
                                target = "Camera",
                                "cur: {}, prev: {}",
                                status.camera_status,
                                cam.camera_status
                            );
                            tracing::info!(
                                target = "Camera",
                                "elapsed: {:?}, timeout: {}",
                                status.time_last_cam_change.elapsed(),
                                kick_conf.cammed_down_timeout
                            );
                            if !status.camera_status
                                && status.time_last_cam_change.elapsed()
                                    > Duration::from_secs(kick_conf.cammed_down_timeout)
                            {
                                let user = cam.user_id.to_user(&ctx.http).await.unwrap();
                                tracing::warn!(
                                    "User {} has been cammed down for {} seconds",
                                    user.name,
                                    status.time_last_cam_change.elapsed().as_secs()
                                );

                                let guild = cam.guild_id.to_guild_cached(&ctx.cache).unwrap();
                                tracing::error!("about to disconnect {:?}", cam.user_id);

                                // WARN: Disconnect the user
                                // FIXME: Should this not be it's own function?
                                let dc_res = disconnect_member(ctx.clone(), *cam, guild).await;

                                match dc_res {
                                    Ok(_) => {
                                        tracing::error!("User {} has been disconnected", user.name);
                                        let channel = ChannelId(kick_conf.channel_id);
                                        let _ = channel
                                            .send_message(&ctx.http, |m| {
                                                m.content(format!(
                                                    "{} {}",
                                                    user.mention(),
                                                    kick_conf.dc_message
                                                ))
                                            })
                                            .await;
                                    }
                                    Err(err) => {
                                        tracing::error!("Error disconnecting user: {}", err);
                                    }
                                }
                                cam_status.remove(&cam.user_id);
                            }
                        }
                    }
                } else {
                    new_cams.push(cam);
                }
            }
            let res = new_cams
                .iter()
                .all(|x| cam_status.insert(x.user_id, **x).is_none());
            if !res {
                tracing::error!("Something failed to insert??!?!");
            }

            tracing::error!("Sleeping");
            tokio::time::sleep(Duration::from_secs(config.video_status_poll_interval)).await;
        }
    });
}

async fn disconnect_member(
    ctx: Arc<SerenityContext>,
    cam: MyVoiceUserInfo,
    guild: Guild,
) -> Result<Member, SerenityError> {
    guild
        .member(&ctx.http, cam.user_id)
        .await
        .expect("Member not found")
        .edit(&ctx.http, |m| m.disconnect_member())
        .await
}

#[allow(dead_code)]
fn voice_state_diff_str(old: Option<VoiceState>, new: &VoiceState) -> String {
    let old = match old {
        Some(old) => old,
        None => {
            return format!(
                "{} / {} / {}",
                new.member.as_ref().unwrap().user.name,
                new.guild_id.unwrap().0,
                new.channel_id.unwrap().0
            );
            // return format!(
            //     "channel_id: (none) -> {:?}
            //     deaf: (none) -> {:?}
            //     guild_id: (none) -> {:?}
            //     member: (none) -> {:?}
            //     mute: (none) -> {:?}
            //     self_deaf: (none) -> {:?}
            //     self_mute: (none) -> {:?}
            //     self_stream: (none) -> {:?}
            //     self_video: (none) -> {:?}
            //     session_id: (none) -> {:?}
            //     suppress: (none) -> {:?}
            //     token: (none) -> {:?}
            //     user_id: (none) -> {:?}
            //     request_to_speak_timestamp: (none) -> {:?}",
            //     new.channel_id, new.deaf, new.guild_id, new.mute, new.member, new.self_deaf, new.self_mute,
            //     new.self_stream, new.self_video, new.session_id, new.suppress, new.token, new.user_id, new.request_to_speak_timestamp
            // );
        }
    };
    let mut result = String::new();
    if old.channel_id != new.channel_id {
        result.push_str(&format!(
            "channel_id: {:?} -> {:?}\n",
            old.channel_id, new.channel_id
        ));
    }
    if old.deaf != new.deaf {
        result.push_str(&format!("deaf: {:?} -> {:?}\n", old.deaf, new.deaf));
    }
    if old.mute != new.mute {
        result.push_str(&format!("mute: {:?} -> {:?}\n", old.mute, new.mute));
    }
    if old.guild_id != new.guild_id {
        result.push_str(&format!(
            "guild_id: {:?} -> {:?}\n",
            old.guild_id, new.guild_id
        ));
    }

    if old.self_deaf != new.self_deaf {
        result.push_str(&format!(
            "self_deaf: {:?} -> {:?}\n",
            old.self_deaf, new.self_deaf
        ));
    }
    if old.self_mute != new.self_mute {
        result.push_str(&format!(
            "self_mute: {:?} -> {:?}\n",
            old.self_mute, new.self_mute
        ));
    }
    if old.self_stream != new.self_stream {
        result.push_str(&format!(
            "self_stream: {:?} -> {:?}\n",
            old.self_stream, new.self_stream
        ));
    }
    if old.self_video != new.self_video {
        result.push_str(&format!(
            "self_video: {:?} -> {:?}\n",
            old.self_video, new.self_video
        ));
    }
    if old.session_id != new.session_id {
        result.push_str(&format!(
            "session_id: {:?} -> {:?}\n",
            old.session_id, new.session_id
        ));
    }
    if old.suppress != new.suppress {
        result.push_str(&format!(
            "suppress: {:?} -> {:?}\n",
            old.suppress, new.suppress
        ));
    }
    if old.token != new.token {
        result.push_str(&format!("token: {:?} -> {:?}\n", old.token, new.token));
    }
    if old.user_id != new.user_id {
        result.push_str(&format!(
            "user_id : {:?} -> {:?}\n",
            old.user_id, new.user_id
        ));
    }
    if old.request_to_speak_timestamp != new.request_to_speak_timestamp {
        result.push_str(&format!(
            "request_to_speak: {:?} -> {:?}\n",
            old.request_to_speak_timestamp, new.request_to_speak_timestamp,
        ));
    }
    result
}
