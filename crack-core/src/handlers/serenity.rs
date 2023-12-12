use crate::{
    db::GuildEntity,
    guild::settings::{GuildSettings, GuildSettingsMap},
    sources::spotify::{Spotify, SPOTIFY},
    BotConfig, CamKickConfig, Data,
};
use ::serenity::{
    all::Message,
    builder::{CreateEmbed, CreateMessage, EditMember},
    gateway::ActivityData,
};
use chrono::{DateTime, Utc};
use colored::Colorize;
use poise::serenity_prelude::{
    self as serenity, Channel, Error as SerenityError, Member, Mentionable, UserId,
};
use serenity::{
    async_trait,
    model::{gateway::Ready, id::GuildId, prelude::VoiceState},
    ChannelId, {Context as SerenityContext, EventHandler},
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{atomic::Ordering, Arc, Mutex},
    time::SystemTime,
};
use tokio::time::{Duration, Instant};

pub struct SerenityHandler {
    pub data: Data,
    pub is_loop_running: std::sync::atomic::AtomicBool,
}

#[derive(Copy, Clone, Debug)]
pub struct MyVoiceUserInfo {
    pub user_id: UserId,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    // true = on, false = off
    pub camera_status: bool,
    pub time_last_cam_change: Instant,
}

impl MyVoiceUserInfo {
    pub fn key(&self) -> (UserId, ChannelId) {
        (self.user_id, self.channel_id)
    }
}

#[async_trait]
impl EventHandler for SerenityHandler {
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        ctx.set_activity(Some(ActivityData::listening(format!(
            "{}play",
            __self.data.bot_settings.get_prefix()
        ))));

        // attempts to authenticate to spotify
        *SPOTIFY.lock().await = Spotify::auth(None).await;

        // These are the guild settings defined in the config file.
        // Should they always override the ones in the database?
        // tracing::warn!("Merging guilds' settings");
        // self.merge_guild_settings(&ctx, &ready, self.data.guild_settings_map.clone())
        //     .await;

        // *self.data.guild_settings_map.lock().unwrap() = guild_settings_map;
        // let mut guild_settings_map = self.data().guild_settings_map.lock().unwrap();
        // let num_saved = {
        //     let mut x = 0;
        //     self.data
        //         .guild_settings_map
        //         .lock()
        //         .unwrap()
        //         .iter()
        //         .for_each(|(k, v)| {
        //             tracing::warn!("Saving Guild: {}", k);
        //             x = x + 1;
        //             v.save().expect("Error saving guild settings");
        //         });
        //     x
        // };

        // tracing::warn!("num_saved: {}", num_saved);
    }

    async fn guild_member_addition(&self, ctx: SerenityContext, new_member: Member) {
        tracing::info!(
            "{}{}",
            "new member: ".white(),
            new_member.to_string().white()
        );
        let guild_id = new_member.guild_id;
        let guild_settings_map = self.data.guild_settings_map.read().unwrap().clone();
        let guild_settings = guild_settings_map.get(&guild_id);
        // let guild_settings = guild_settings_map.get_mut(&guild_id);
        // guild_settings.cloned()

        let (_guild_settings, welcome) = match guild_settings {
            Some(guild_settings) => match guild_settings.clone().welcome_settings {
                None => return,
                Some(welcome_settings) => (guild_settings, welcome_settings),
            },
            None => {
                tracing::error!("Guild settings not found for guild {}", guild_id);
                return;
            }
        };

        tracing::trace!("welcome: {:?}", welcome);

        match (welcome.message, welcome.channel_id) {
            (None, _) => {}
            (_, None) => {}
            (Some(message), Some(channel)) => {
                let channel = serenity::ChannelId::new(channel);
                let x = channel
                    .send_message(
                        &ctx.http,
                        CreateMessage::default().content({
                            if message.contains("{user}") {
                                message.replace(
                                    "{user}",
                                    new_member.user.mention().to_string().as_str(),
                                )
                            } else {
                                format_args!("{} {user}", message, user = new_member.user.mention())
                                    .to_string()
                            }
                        }),
                    )
                    .await;
                tracing::info!("x: {:?}", x.unwrap());
            }
        };

        if let Some(role_id) = welcome.auto_role {
            tracing::info!("{}{}", "role_id: ".white(), role_id.to_string().white());
            let role_id = serenity::RoleId::new(role_id);
            match new_member.add_role(&ctx.http, role_id).await {
                Ok(_) => {
                    tracing::info!("{}{}", "role added: ".white(), role_id.to_string().white());
                }
                Err(err) => {
                    tracing::error!("Error adding role: {}", err);
                }
            }
        }
    }

    async fn message(&self, ctx: SerenityContext, msg: serenity::Message) {
        struct MyMessage(serenity::Message);
        impl fmt::Display for MyMessage {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut result = String::new();
                let msg = &self.0;
                // let guild_id = match msg.guild_id {
                //     Some(guild_id) => guild_id,
                //     None => {
                //         tracing::warn!("Non-gateway message received: {:?}", msg);
                //         GuildId(0)
                //     }
                // };
                let name = msg.author.name.clone();
                let content = msg.content.clone();
                result.push_str(&format!("Message: {} {}", name.purple(), content.purple(),));
                msg.embeds.iter().for_each(|x| {
                    result.push_str(&format!(
                        "{}{}{}",
                        x.title.as_ref().unwrap_or(&String::new()).purple(),
                        x.description.as_ref().unwrap_or(&String::new()).purple(),
                        x.fields.iter().fold(String::new(), |acc, x| {
                            format!("{}{}{}", acc, x.name.purple(), x.value.purple())
                        })
                    ));
                });
                write!(f, "{}", result)
            }
        }

        let guild_id = match msg.guild_id {
            Some(guild_id) => guild_id,
            None => {
                tracing::warn!("Non-gateway message received: {:?}", msg);
                return;
            }
        };

        let guild_name = {
            let guild = guild_id.to_guild_cached(&ctx.cache).unwrap();
            guild.name.clone()
        };
        let name = msg.author.name.clone();
        // let guild_name = guild.name;
        let content = msg.content.clone();
        let channel_name = msg.channel_id.name(&ctx.clone()).await.unwrap_or_default();

        tracing::info!(
            "Message: {} {} {} {}",
            name.purple(),
            guild_name.purple(),
            channel_name.purple(),
            content.purple(),
        );
        let _mm = MyMessage(msg);
    }

    async fn voice_state_update(
        &self,
        ctx: SerenityContext,
        _old: Option<VoiceState>,
        new: VoiceState,
    ) {
        // do nothing if this is a voice update event for a user, not a bot
        if new.user_id != ctx.cache.current_user().id {
            return;
        }

        if new.channel_id.is_some() {
            // check the data struct with this guild to see self deafen settings
            // if self deafen is false, deafen the bot
            // if self deafen is true, do nothing
            // if self deafen is None, do nothing
            let do_i_deafen = self
                .data
                .guild_settings_map
                .read()
                .unwrap()
                .get(&new.guild_id.unwrap())
                .map(|x| x.self_deafen)
                .unwrap_or_else(|| true);
            if !do_i_deafen {
                return self.self_deafen(&ctx, new.guild_id, new).await;
            }
            return;
        }

        let manager = songbird::get(&ctx).await.unwrap();
        let guild_id = new.guild_id.unwrap();

        // This is a voice state update event for the bot
        // However there is no channel_id, so the bot has been disconnected
        // from the voice channel
        // This somehow clears the queue?
        // TODO: Figure out why this clears the queue
        // TODO: Figure out why there is a voice state update event when the bot is disconnected
        // from the voice channel
        // ANSWER: This is because the bot is deafened, so it's a voice state update event
        // Q: What if the bot is not deafened?
        // A: Then there is no voice state update event
        // Q: Then how do we know when the bot is disconnected from the voice channel?
        // A: We don't
        // Q: Fuck you :(
        // A:
        if manager.get(guild_id).is_some() {
            manager.remove(guild_id).await.ok();
        }

        // update_queue_messages(&ctx.http, &self.data, &[], guild_id).await;
    }

    // We use the cache_ready event just in case some cache operation is required in whatever use
    // case you have for this.
    async fn cache_ready(&self, ctx: SerenityContext, guilds: Vec<GuildId>) {
        tracing::info!("Cache built successfully! {} guilds cached", guilds.len());

        for guildid in guilds.iter() {
            tracing::info!("Guild: {:?}", guildid);
        }

        let config = self.data.bot_settings.clone();
        let video_status_poll_interval = config.get_video_status_poll_interval();
        // let config.get
        // it's safe to clone Context, but Arc is cheaper for this use case.
        // Untested claim, just theoretically. :P
        let arc_ctx = Arc::new(ctx.clone());
        let arc_config = Arc::new(config.clone());

        // loads serialized guild settings
        tracing::warn!("Loading guilds' settings");
        let _ = self
            .load_guilds_settings_cache_ready(&arc_ctx.clone(), &guilds)
            .await;

        let num_inserted = {
            let ctx1 = arc_ctx.clone();
            let lock = ctx1.data.read().await;
            let guild_settings_map = lock.get::<GuildSettingsMap>().unwrap();
            let mut data_write = self.data.guild_settings_map.write().unwrap();

            let mut x = 0;
            for (key, value) in guild_settings_map.clone().iter() {
                tracing::info!("Guild {} settings: {:?}", key, value);

                data_write.insert(*key, value.clone());
                x += 1;
            }
            x
        };

        tracing::warn!("num_inserted: {}", num_inserted);

        // We need to check that the loop is not already running when this event triggers,
        // as this event triggers every time the bot enters or leaves a guild, along every time the
        // ready shard event triggers.
        //
        // An AtomicBool is used because it doesn't require a mutable reference to be changed, as
        // we don't have one due to self being an immutable reference.
        if !self.is_loop_running.load(Ordering::Relaxed) {
            // We have to clone the Arc, as it gets moved into the new thread.
            // tokio::spawn creates a new green thread that can run in parallel with the rest of
            // the application.
            if false {
                let ctx2 = arc_ctx.clone();
                let config2 = arc_config.clone();
                let _res = tokio::spawn(async move {
                    loop {
                        // We clone Context again here, because Arc is owned, so it moves to the
                        // new function.
                        log_system_load(ctx2.clone(), config2.clone()).await;
                        tokio::time::sleep(Duration::from_secs(video_status_poll_interval)).await;
                    }
                })
                .await;
            }

            // let _data = self.data.clone();
            // let _res: JoinHandle<()> = tokio::spawn(async move {
            //     let ctx3 = arc_ctx.clone();
            //     loop {
            //         let ctx4 = ctx3.clone();
            //         // We clone Context again here, because Arc is owned, so it moves to the
            //         tokio::time::sleep(Duration::from_secs(60)).await;
            //         let _guilds = get_guilds(ctx4.clone()).await;
            //         tracing::warn!("*Not* checking for old messages");
            //         // let _ = check_delete_old_messages(
            //         //     ctx2.clone(),
            //         //     &data,
            //         //     guilds,
            //         //     chrono::Duration::from_std(Duration::from_secs(10 * 60)).unwrap(),
            //         // )
            //         // .await;
            //     }
            // });

            let ctx3 = arc_ctx.clone();
            let config3 = arc_config.clone();
            if video_status_poll_interval > 0 {
                cam_status_loop(ctx3.clone(), config3.clone(), guilds.clone()).await;
            };

            // Now that the loop is running, we set the bool to true
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }
    }
}

impl SerenityHandler {
    async fn _merge_guild_settings(
        &self,
        ctx: &SerenityContext,
        _ready: &Ready,
        new_settings: Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
    ) {
        tracing::warn!("in merge_guild_settings");
        let mut data = ctx.data.write().await;

        let settings = data.get_mut::<GuildSettingsMap>().unwrap();
        let mut new_settings = new_settings.lock().unwrap();

        tracing::warn!("new_settings len: {:?}", new_settings.len());

        for (key, value) in new_settings.iter() {
            match settings.insert(*key, value.clone()) {
                Some(_) => tracing::info!("Guild {} settings overwritten", key),
                None => tracing::info!("Guild {} settings did not exist", key),
            }
        }

        for (key, value) in settings.iter_mut() {
            new_settings.insert(*key, value.clone());
        }
        tracing::warn!(
            "settings len: {:?}, new_settings len: {:?}",
            settings.len(),
            new_settings.len()
        );
    }

    async fn _load_guilds_settings(&self, ctx: &SerenityContext, ready: &Ready) {
        let prefix = self.data.bot_settings.get_prefix();
        tracing::info!("Loading guilds' settings");

        for guild in &ready.guilds {
            let guild_id = guild.id;
            let guild_name = match guild_id.to_guild_cached(&ctx.cache) {
                Some(guild_match) => guild_match.name.clone(),
                None => {
                    tracing::error!("Guild not found in cache");
                    continue;
                }
            };
            tracing::info!(
                "Loading guild settings for {}, {}",
                guild_id,
                guild_name.clone()
            );

            let mut default = GuildSettings::new(guild_id, Some(&prefix), Some(guild_name.clone()));

            let pool = self.data.database_pool.clone().unwrap();
            let _ = default.load_if_exists(&pool).await.map_err(|err| {
                tracing::error!("Failed to load guild {} settings due to {}", guild_id, err);
            });

            tracing::warn!("GuildSettings: {:?}", default);

            self.data
                .guild_settings_map
                .write()
                .unwrap()
                .insert(guild_id, default.clone());

            // match guild_settings {
            //     Some(guild_settings) => {
            default.save().await.expect("Error saving guild settings");
            tracing::info!("saving guild {}...", default);
            //     }
            //     None => {
            //         tracing::error!("Guild not found in settings map");
            //     }
            // }
        }
    }

    async fn load_guilds_settings_cache_ready(
        &self,
        ctx: &SerenityContext,
        guilds: &Vec<GuildId>,
    ) -> Result<(), SerenityError> {
        let prefix = self.data.bot_settings.get_prefix();
        tracing::info!("Loading guilds' settings");

        let mut guild_settings_list: Vec<GuildSettings> = Vec::new();
        for guild_id in guilds {
            let guild_name = match guild_id.to_guild_cached(&ctx.cache) {
                Some(guild_match) => guild_match.name.clone(),
                None => {
                    tracing::error!("Guild not found in cache");
                    continue;
                }
            };
            tracing::info!(
                "Loading guild settings for {}, {}",
                guild_id,
                guild_name.clone()
            );

            let default = GuildSettings::new(
                *guild_id,
                Some(&prefix),
                Some(guild_name.to_ascii_lowercase().clone()),
            );

            // let guild_entity = GuildEntity::new_guild(guild_id.get() as i64, guild_name.to_ascii_lowercase().clone());
            let guild_id_int = guild_id.get() as i64;
            let guild_name = guild_name.to_ascii_lowercase().clone();
            let prefix = prefix.clone();
            let pool = self.data.database_pool.clone().unwrap();
            let (_guild, settings) =
                GuildEntity::get_or_create(&pool, guild_id_int, guild_name, prefix)
                    .await
                    .unwrap();
            // .map_err(Into::into)?;
            let mut guild_settings_map = self.data.guild_settings_map.write().unwrap();

            // let _ = default..map_err(|err| {
            //     tracing::error!("Failed to load guild {} settings due to {}", guild_id, err);
            // });

            tracing::warn!("GuildSettings: {:?}", settings);

            let _ = guild_settings_map.insert(*guild_id, default.clone());
            tracing::warn!("guild_settings_map: {:?}", guild_settings_map);

            let guild_settings_opt = guild_settings_map.get_mut(guild_id);

            match guild_settings_opt {
                Some(&mut ref guild_settings) => {
                    tracing::info!("saving guild {}...", guild_settings);
                    guild_settings_list.push(guild_settings.clone());
                    // guild_settings
                    //     .save()
                    //     .await
                    //     .expect("Error saving guild settings");
                }
                None => {
                    tracing::error!("Guild not found in settings map");
                }
            }
        }

        for guild in guild_settings_list.clone() {
            guild.save().await.expect("Error saving guild settings");
        }

        Ok(())
    }

    async fn self_deafen(&self, ctx: &SerenityContext, guild: Option<GuildId>, new: VoiceState) {
        if self.data.bot_settings.self_deafen.is_some() {
            return;
        }

        let Ok(user) = ctx.http.get_current_user().await else {
            return;
        };

        if user.id == new.user_id && !new.deaf {
            guild
                .unwrap()
                .edit_member(&ctx.http, new.user_id, EditMember::default().deafen(true))
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
    if let Some(chan_id) = config.sys_log_channel_id {
        let message = ChannelId::new(chan_id)
            .send_message(
                &ctx,
                CreateMessage::new().embed({
                    CreateEmbed::new()
                        .title("System Resource Load")
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
                }),
            )
            .await;
        if let Err(why) = message {
            tracing::error!("Error sending message: {:?}", why);
        };
    } else {
        tracing::error!("No system log channel set");
    }
}

async fn check_camera_status(ctx: Arc<SerenityContext>, guild_id: GuildId) -> Vec<MyVoiceUserInfo> {
    let (voice_states, guild_name) = match guild_id.to_guild_cached(&ctx.cache) {
        Some(guild) => (guild.voice_states.clone(), guild.name.clone()),
        None => {
            tracing::error!("Guild not found in cache");
            return vec![];
        }
    };

    // let voice_states = &guild.voice_states;
    let mut cams = vec![];
    let mut output: String = format!("{}\n", guild_name.bright_green());

    for (user_id, voice_state) in voice_states {
        if let Some(channel_id) = voice_state.channel_id {
            let user = match user_id.to_user(&ctx.http).await {
                Ok(user) => user,
                Err(err) => {
                    tracing::error!("Error getting user: {}", err);
                    continue;
                }
            };
            let channel_name = match channel_id.to_channel(&ctx.http).await {
                Ok(channel) => match channel {
                    Channel::Guild(channel) => channel.name,
                    Channel::Private(channel) => channel.name(),
                    _ => String::from("unknown"),
                },
                Err(err) => {
                    tracing::error!("Error getting channel name for {}: {}", channel_id, err);
                    "MISSING_ACCESS".to_string()
                }
            };

            let info = MyVoiceUserInfo {
                user_id,
                guild_id,
                channel_id,
                camera_status: voice_state.self_video,
                time_last_cam_change: Instant::now(),
            };

            cams.push(info);
            output.push_str(&format!(
                "{}|{}|{}|{}|{}\n",
                &user.name,
                &user.id,
                &channel_name,
                &channel_id,
                if info.camera_status { "on" } else { "off" },
            ));
        }
    }
    tracing::warn!("{}", output.bright_cyan());
    cams
}

/// Checks the guilds' message cache for messages that are older than the timeout interval.
#[allow(dead_code)]
async fn check_delete_old_messages(
    ctx: Arc<SerenityContext>,
    data: &Data,
    guild_ids: Vec<GuildId>,
    msg_timeout_interval: chrono::Duration,
) -> Result<(), SerenityError> {
    let mut to_delete = Vec::<Message>::new();
    for guild_id in guild_ids.iter() {
        tracing::warn!("Checking guild {}", guild_id);
        data.guild_msg_cache_ordered
            .lock()
            .unwrap()
            .get_mut(guild_id);
        if let Some(guild_cache) = data
            .guild_msg_cache_ordered
            .lock()
            .unwrap()
            .get_mut(guild_id)
        {
            let now = DateTime::<Utc>::from(SystemTime::now());
            for (creat_time, msg) in guild_cache.time_ordered_messages.iter() {
                let delta = now.signed_duration_since(*creat_time);
                if delta.cmp(&msg_timeout_interval) == std::cmp::Ordering::Greater {
                    tracing::warn!("Adding old message to delete queue");
                    to_delete.push(msg.clone());
                }
            }
        }
    }
    for msg in to_delete {
        tracing::error!("Deleting old message: {:#?}", msg);
        match msg.delete(ctx.clone()).await {
            Ok(_) => {}
            Err(err) => {
                tracing::error!("Error deleting message: {}", err);
            }
        }
    }
    Ok(())
}

async fn cam_status_loop(ctx: Arc<SerenityContext>, config: Arc<BotConfig>, guilds: Vec<GuildId>) {
    tokio::spawn(async move {
        tracing::trace!(
            target = "cam_status_loop",
            "Starting camera status check loop"
        );
        let cam_kick = config.cam_kick.clone().unwrap_or_default();
        let conf_guilds = cam_kick.iter().map(|x| x.guild_id).collect::<HashSet<_>>();
        let mut cam_status: HashMap<(UserId, ChannelId), MyVoiceUserInfo> =
            HashMap::<(UserId, ChannelId), MyVoiceUserInfo>::new();
        let channels: HashMap<u64, &CamKickConfig> = cam_kick
            .iter()
            .map(|x| (x.channel_id, x))
            .collect::<HashMap<_, _>>();
        conf_guilds
            .iter()
            .for_each(|x| tracing::error!("Guild: {}", x));
        tracing::warn!("conf_guilds: {}", format!("{:?}", conf_guilds).green());
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
                if let Some(status) = cam_status.get(&cam.key()) {
                    if let Some(kick_conf) = channels.get(&status.channel_id.get()) {
                        tracing::warn!("kick_conf: {}", format!("{:?}", kick_conf).blue());
                        if status.camera_status != cam.camera_status {
                            tracing::info!(
                                "Camera status changed for user {} to {}",
                                status.user_id,
                                cam.camera_status
                            );
                            cam_status.insert(cam.key(), *cam);
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

                                // let guild = cam.guild_id.to_guild_cached(&ctx.cache).unwrap();
                                let guild_id = cam.guild_id;
                                tracing::error!("about to disconnect {:?}", cam.user_id);

                                // WARN: Disconnect the user
                                // FIXME: Should this not be it's own function?
                                // let dc_res = disconnect_member(ctx.clone(), *cam, guild).await;
                                let dc_res1 = (
                                    server_defeafen_member(ctx.clone(), *cam, guild_id).await,
                                    "deafen",
                                );
                                let dc_res2 = (
                                    server_mute_member(ctx.clone(), *cam, guild_id).await,
                                    "mute",
                                );

                                for (dc_res, state) in vec![dc_res1, dc_res2] {
                                    match dc_res {
                                        Ok(_) => {
                                            tracing::error!(
                                                "User {} has been violated: {}",
                                                user.name,
                                                state
                                            );
                                            if state == "deafen" && kick_conf.send_msg_deafen
                                                || state == "mute" && kick_conf.send_msg_mute
                                                || state == "disconnect" && kick_conf.send_msg_dc
                                            {
                                                let channel = ChannelId::new(kick_conf.channel_id);
                                                let _ = channel
                                                    .send_message(
                                                        &ctx.http,
                                                        CreateMessage::default().content({
                                                            format!(
                                                                "{} {}: {}",
                                                                user.mention(),
                                                                kick_conf.dc_message,
                                                                state
                                                            )
                                                        }),
                                                    )
                                                    .await;
                                                // cam_status.remove(&cam.key());
                                            }
                                            cam_status.remove(&cam.key());
                                            // if state == "disconnect" {
                                            //     cam_status.remove(&cam.key());
                                            // }
                                        }
                                        Err(err) => {
                                            tracing::error!("Error violating user: {}", err);
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    new_cams.push(cam);
                }
            }
            let res: i32 = new_cams
                .iter()
                .map(|x| {
                    if cam_status.insert(x.key(), **x).is_some() {
                        0
                    } else {
                        1
                    }
                })
                .sum();

            tracing::warn!("num new cams: {}", res);
            tracing::warn!(
                "Sleeping for {} seconds",
                config.get_video_status_poll_interval()
            );
            tokio::time::sleep(Duration::from_secs(config.get_video_status_poll_interval())).await;
        }
    });
}

#[allow(dead_code)]
async fn disconnect_member(
    ctx: Arc<SerenityContext>,
    cam: MyVoiceUserInfo,
    guild: GuildId,
) -> Result<Member, SerenityError> {
    guild
        .edit_member(
            &ctx.http,
            cam.user_id,
            EditMember::default().disconnect_member(),
        )
        .await
}

async fn server_defeafen_member(
    ctx: Arc<SerenityContext>,
    cam: MyVoiceUserInfo,
    guild: GuildId,
) -> Result<Member, SerenityError> {
    guild
        .edit_member(&ctx.http, cam.user_id, EditMember::default().deafen(true))
        .await
}

async fn server_mute_member(
    ctx: Arc<SerenityContext>,
    cam: MyVoiceUserInfo,
    guild: GuildId,
) -> Result<Member, SerenityError> {
    guild
        .edit_member(&ctx.http, cam.user_id, EditMember::default().mute(true))
        .await
}

pub fn voice_state_diff_str(
    old: &Option<VoiceState>,
    new: &VoiceState,
    cache: impl AsRef<serenity::Cache>,
) -> String {
    let premium = true; //DEFAULT_PREMIUM;
    let old = match old {
        Some(old) => old,
        None => {
            let user_name = &new.member.as_ref().unwrap().user.name;
            let user_id = new.user_id;
            let channel_id = new.channel_id.unwrap();
            let channel_mention = channel_id
                .to_channel_cached(cache.as_ref())
                .unwrap()
                .mention();
            let now_str = chrono::Local::now().to_string();

            return format!(
                "Member joined voice channel\n{} joined {}\nID: {} * {}",
                user_name, channel_mention, user_id, now_str
            );
        }
    };
    let user = if premium {
        if old.member.is_none() {
            new.member.as_ref().unwrap().user.mention().to_string()
        } else {
            old.member.as_ref().unwrap().user.mention().to_string()
        }
    } else if old.member.is_none() {
        new.member.as_ref().unwrap().user.name.to_string()
    } else {
        old.member.as_ref().unwrap().user.name.to_string()
    };
    let mut result = String::new();
    if old.channel_id != new.channel_id {
        if new.channel_id.is_none() {
            let user_name = &new.member.as_ref().unwrap().user.name;
            let user_mention = new.member.as_ref().unwrap().user.mention();
            let channel_id = old.channel_id.unwrap();
            let channel_mention = channel_id
                .to_channel_cached(cache.as_ref())
                .unwrap()
                .mention();

            let user = if premium {
                user_mention.to_string()
            } else {
                user_name.to_string()
            };

            let channel = if premium {
                channel_mention.to_string()
            } else {
                channel_id.to_string()
            };

            return format!("Member left voice channel\n{} left {}\n", user, channel);
        } else if old.channel_id.is_none() {
            let user_name = &new.member.as_ref().unwrap().user.name;
            let channel_id = new.channel_id.unwrap();
            let channel_mention = channel_id
                .to_channel_cached(cache.as_ref())
                .unwrap()
                .mention();

            return format!(
                "Member joined voice channel\n{} joined {}\n",
                user_name, channel_mention
            );
        } else {
            let old_channel_id = old.channel_id.unwrap();
            let new_channel_id = new.channel_id.unwrap();
            let old_channel_mention = old_channel_id
                .to_channel_cached(cache.as_ref())
                .unwrap()
                .mention();
            let new_channel_mention = new_channel_id
                .to_channel_cached(cache.as_ref())
                .unwrap()
                .mention();
            result.push_str(&format!(
                "Switched voice channels: {} -> {}\n",
                old_channel_mention, new_channel_mention
            ));
        }
    }
    if old.deaf != new.deaf {
        if new.deaf {
            result.push_str(&format!("{} deafend\n", user));
        } else {
            result.push_str(&format!("{} undeafend\n", user));
        }
    }
    if old.mute != new.mute {
        if new.mute {
            result.push_str(&format!("{} muted\n", user));
        } else {
            result.push_str(&format!("{} unmuted\n", user));
        }
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
        if old.self_video {
            result.push_str(&format!("{} turned off their camera\n", user));
        } else {
            result.push_str(&format!("{} turned on their camera\n", user));
        }
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
