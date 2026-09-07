use crate::{
    // commands::queue_aux_metadata,
    commands::music::gp::{handle_gp_component, GP_CUSTOM_ID_PREFIX},
    db::GuildEntity,
    errors::CrackedError,
    guild::settings::{GuildSettings, DEFAULT_ACTIVITY},
    handlers::voice_chat_stats::cam_status_loop,
    sources::spotify::{Spotify, SPOTIFY},
    BotConfig,
    Data,
};
use ::serenity::{
    all::Message,
    builder::{CreateEmbed, CreateMessage, EditMember},
    gateway::ActivityData,
};
use chrono::{DateTime, Utc};
use colored::Colorize;
// use dashmap;
use poise::serenity_prelude::{self as serenity, Error as SerenityError, Member, Mentionable};
use serenity::CacheHttp;
use serenity::{
    async_trait,
    model::{application::Interaction, gateway::Ready, id::GuildId, prelude::VoiceState},
    GenericChannelId, {Context as SerenityContext, EventHandler, FullEvent},
};
use std::{
    sync::{atomic::Ordering, Arc},
    time::SystemTime,
};

pub struct SerenityHandler {
    pub data: Data,
    pub is_loop_running: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl EventHandler for SerenityHandler {
    /// serenity's `EventHandler` collapsed its per-event methods into a single
    /// `dispatch`. The handlers below keep their old shapes; this just routes
    /// `FullEvent` variants to them.
    async fn dispatch(&self, ctx: &SerenityContext, event: &FullEvent) {
        match event {
            FullEvent::Ready { data_about_bot } => {
                self.on_ready(ctx.clone(), data_about_bot.clone()).await;
            },
            FullEvent::GuildCreate { guild, .. } => {
                self.on_guild_create(ctx.clone(), guild.clone()).await;
            },
            FullEvent::GuildMemberAddition { new_member } => {
                self.on_guild_member_addition(ctx.clone(), new_member.clone())
                    .await;
            },
            FullEvent::VoiceStateUpdate { old, new } => {
                self.on_voice_state_update(ctx.clone(), old.clone(), new.clone())
                    .await;
            },
            FullEvent::CacheReady { guilds } => {
                self.on_cache_ready(ctx.clone(), guilds.clone()).await;
            },
            // `/gp` dropdown picks and 👍s. Poise ignores component
            // interactions, so they are routed here by custom-id prefix.
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(mci),
            } if mci.data.custom_id.starts_with(GP_CUSTOM_ID_PREFIX) => {
                if let Err(e) = handle_gp_component(&self.data, ctx, mci).await {
                    tracing::warn!("gp component: {e}");
                }
            },
            _ => {},
        }

        // poise removed `FrameworkOptions::event_handler`, so the event log /
        // router that used to hang off it is driven from here instead.
        if let Err(e) = crate::handlers::handle_event(ctx, event, ctx.data::<Data>()).await {
            // A guild that has not configured a log channel is the ordinary
            // case, not a failure -- logging it at ERROR put one line on every
            // event in every such guild.
            match e.downcast_ref::<CrackedError>() {
                Some(CrackedError::LogChannelWarning(..)) => {
                    tracing::trace!("Event not logged: {e}")
                },
                _ => tracing::error!("Error handling event: {e}"),
            }
        }
    }
}

impl SerenityHandler {
    async fn on_ready(&self, ctx: SerenityContext, ready: Ready) {
        tracing::info!(
            "{} {}",
            ready.user.name,
            crate::messaging::messages::CONNECTED
        );

        ctx.set_activity(Some(ActivityData::listening(DEFAULT_ACTIVITY)));

        // Sync THIS build's command list to Discord.
        //
        // Nothing did this before. The setup closure that called
        // `register_globally_cracked` was commented out when poise dropped the
        // old `Framework` builder, and the only remaining path was an admin
        // manually running `register`. So the application kept whatever the
        // last bot to run under it had registered -- a stale list that never
        // contained `/spotify`.
        //
        // `Ready` fires once per shard and again after a full reconnect, so
        // this is gated to once per process.
        static COMMANDS_REGISTERED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !COMMANDS_REGISTERED.swap(true, Ordering::SeqCst) {
            let commands = crate::commands::commands_to_register();
            match crate::commands::register_globally_cracked(&ctx.http, &commands).await {
                Ok(()) => tracing::info!("Registered {} global commands", commands.len()),
                Err(e) => {
                    tracing::error!("Failed to register global commands: {e}");
                    // Clear the gate so a later `Ready` retries; otherwise one
                    // transient failure leaves the process with no commands.
                    COMMANDS_REGISTERED.store(false, Ordering::SeqCst);
                },
            }
        }

        // Attempt to authenticate to Spotify, and SAY SO EITHER WAY.
        //
        // This result used to be stored and never read, so a bot booting with no
        // Spotify credentials looked identical to one booting with working ones.
        // The first anybody learned of it was a user pasting a Spotify link and
        // getting an error -- or, worse, autoplay quietly switching itself off.
        let spotify_auth = Spotify::auth(None).await;
        match &spotify_auth {
            Ok(_) => tracing::info!("{}", crate::messaging::messages::SPOTIFY_ENABLED_LOG),
            Err(e) => tracing::warn!(
                "{} Reason: {}",
                crate::messaging::messages::SPOTIFY_DISABLED_LOG,
                e
            ),
        }
        *SPOTIFY.lock().await = spotify_auth;

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

        // Process-level background work, moved off `CacheReady`.
        //
        // It needs no guild set beyond the ids, which `Ready` already carries, and
        // `CacheReady` is not guaranteed to arrive at all -- see `on_guild_create`.
        // `Ready` re-fires on reconnect, so this keeps the existing `is_loop_running`
        // guard to stay once-per-process.
        if !self.is_loop_running.load(Ordering::Relaxed) {
            let interval = self.data.bot_settings.get_video_status_poll_interval();
            if interval > 0 {
                let guild_ids: Vec<GuildId> = ready.guilds.iter().map(|g| g.id).collect();
                cam_status_loop(
                    Arc::new(ctx.clone()),
                    Arc::new(self.data.bot_settings.clone()),
                    guild_ids,
                )
                .await;
            }
            self.is_loop_running.swap(true, Ordering::Relaxed);
        }

        // tracing::warn!("num_saved: {}", num_saved);
    }

    async fn on_guild_member_addition(&self, ctx: SerenityContext, new_member: Member) {
        tracing::info!(
            "{}{}",
            "new member: ".white(),
            new_member.to_string().white()
        );
        let guild_id = new_member.guild_id;
        let guild_settings_map = self.data.guild_settings_map.read().await.clone();
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
            },
        };

        tracing::trace!("welcome: {:?}", welcome);

        match (welcome.message, welcome.channel_id) {
            (None, _) => {},
            (_, None) => {},
            (Some(message), Some(channel)) => {
                let channel = serenity::GenericChannelId::new(channel);
                let x = channel
                    .send_message(
                        ctx.http(),
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
            },
        };

        if let Some(role_id) = welcome.auto_role {
            tracing::info!("{}{}", "role_id: ".white(), role_id.to_string().white());
            let role_id = serenity::RoleId::new(role_id);
            match new_member.add_role(ctx.http(), role_id, None).await {
                Ok(_) => {
                    tracing::info!("{}{}", "role added: ".white(), role_id.to_string().white());
                },
                Err(err) => {
                    tracing::error!("Error adding role: {}", err);
                },
            }
        }
    }

    async fn on_voice_state_update(
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
                .await
                .get(&new.guild_id.unwrap())
                .map(|x| x.self_deafen)
                .unwrap_or_else(|| true);
            if !do_i_deafen {
                return self.self_deafen(&ctx, new.guild_id, new).await;
            }
            return;
        }
        let manager = ctx.data::<Data>().songbird.clone();

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
        // Kicked or disconnected mid-game: don't leave a zombie game behind.
        if self.data.gp_remove(guild_id).is_some() {
            tracing::warn!("gp: bot left voice in {guild_id}, game discarded");
        }

        // update_queue_messages(&ctx, &self.data, &[], guild_id).await;
    }

    /// Load this guild's settings as the guild arrives.
    ///
    /// Deliberately **not** `CacheReady`. serenity emits that event from inside
    /// `GuildCreate` handling, and only when `cache.unavailable_guilds` has drained
    /// to exactly zero -- every guild in the `Ready` payload having checked in. One
    /// guild that is down at startup, or one the bot was removed from while offline,
    /// and it never fires at all for the life of the process. `GuildDelete` puts a
    /// guild back into that set, so even a cache that completes once can lose the
    /// condition permanently.
    ///
    /// At thirteen guilds that barrier clears every time; at a hundred and fifty it
    /// effectively never does. That is why loading settings here worked in testing
    /// and silently did nothing in production.
    ///
    /// Per-guild work belongs on the per-guild event, where it is also correct for
    /// guilds that recover from an outage or are joined while running.
    async fn on_guild_create(&self, _ctx: SerenityContext, guild: serenity::Guild) {
        let prefix = self.data.bot_settings.get_prefix();
        let name = guild.name.clone();
        let settings = match self.data.database_pool.as_ref() {
            Some(pool) => match GuildEntity::get_or_create(
                pool,
                guild.id.get() as i64,
                name.clone(),
                prefix.clone(),
            )
            .await
            {
                Ok((_guild, settings)) => settings,
                Err(err) => {
                    tracing::error!(
                        "Failed to load settings for guild {} from the database, \
                         falling back to defaults: {err}",
                        guild.id
                    );
                    GuildSettings::new(guild.id, Some(&prefix), Some(name))
                },
            },
            None => GuildSettings::new(guild.id, Some(&prefix), Some(name)),
        };
        self.data
            .guild_settings_map
            .write()
            .await
            .insert(guild.id, settings);
        tracing::info!("Loaded settings for guild {} ({})", guild.id, guild.name);
    }

    // We use the cache_ready event just in case some cache operation is required in whatever use
    // case you have for this.
    async fn on_cache_ready(&self, ctx: SerenityContext, guilds: Vec<GuildId>) {
        tracing::info!("Cache built successfully! {} guilds cached", guilds.len());
        let cache = match ctx.cache() {
            Some(cache) => cache.clone(),
            None => return,
        };

        let mut guilds_from_cache = String::new();
        for guild_id in guilds.iter() {
            match guild_id.name(&cache) {
                Some(name) => guilds_from_cache.push_str(&name),
                None => guilds_from_cache.push_str(&guild_id.to_string()),
            }
            guilds_from_cache.push_str(", ");
        }
        tracing::info!("Guilds from cache:\n{}", guilds_from_cache.purple());

        // Settings are loaded per guild in `on_guild_create`, not here. This event
        // only fires once every guild has checked in, which for a bot in more than a
        // handful of guilds may never happen -- see `on_guild_create` for why.

        // let num_inserted = {
        //     let ctx1 = arc_ctx.clone();
        //     let guild_settings_map = ctx1.data::<Data>().guild_settings_map;
        //     //let lock = ctx1.data.read().await;
        //     //let guild_settings_map = lock.get::<GuildSettingsMap>().unwrap();
        //     let mut data_write = guild_settings_map.write().await;

        //     let mut x = 0;
        //     for (key, value) in (*guild_settings_map.clone()).iter() {
        //         data_write.insert(*key, value.clone());
        //         x += 1;
        //     }
        //     x
        // };

        // tracing::warn!("num_inserted: {}", num_inserted);
    }
}

// use crate::guild::operations::GuildSettingsOperations;

impl SerenityHandler {
    // async fn _merge_guild_settings(
    //     &self,
    //     ctx: &SerenityContext,
    //     _ready: &Ready,
    //     new_settings: Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
    // ) {
    //     tracing::warn!("in merge_guild_settings");
    //     // let mut data = ctx.data.write().await;

    //     // let settings = data.get_mut::<GuildSettingsMap>().unwrap();
    //     let data = ctx.data::<Data>();
    //     let mut settings = data.get_guild_settings().await;
    //     let mut new_settings = new_settings.lock().unwrap();

    //     tracing::warn!("new_settings len: {:?}", new_settings.len());

    //     for (key, value) in new_settings.iter() {
    //         match settings.insert(*key, value.clone()) {
    //             Some(_) => tracing::info!("Guild {} settings overwritten", key),
    //             None => tracing::info!("Guild {} settings did not exist", key),
    //         }
    //     }

    //     for (key, value) in settings.iter_mut() {
    //         new_settings.insert(*key, value.clone());
    //     }
    //     tracing::warn!(
    //         "settings len: {:?}, new_settings len: {:?}",
    //         settings.len(),
    //         new_settings.len()
    //     );
    // }

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
                },
            };
            let to_write = guild_name.clone();
            tracing::info!("Loading guild settings for {guild_id}, {to_write}");

            let mut default = GuildSettings::new(guild_id, Some(&prefix), Some(guild_name));

            let pool = self.data.database_pool.clone().unwrap();
            let _ = default.load_if_exists(&pool).await.map_err(|err| {
                tracing::error!("Failed to load guild {} settings due to {}", guild_id, err);
            });

            tracing::warn!("GuildSettings: {:?}", default);

            self.data
                .guild_settings_map
                .write()
                .await
                .insert(guild_id, default.clone());

            match default.save(&pool).await {
                Ok(()) => tracing::info!("Saved guild {to_write}..."),
                Err(err) => tracing::error!("Failed to save guild {to_write} due to {err}"),
            }
        }
    }

    async fn self_deafen(&self, ctx: &SerenityContext, guild: Option<GuildId>, new: VoiceState) {
        if self.data.bot_settings.self_deafen.is_some() {
            return;
        }

        let Ok(user) = ctx.http.get_current_user().await else {
            return;
        };

        if user.id == new.user_id && !new.deaf() {
            guild
                .unwrap()
                .edit_member(ctx.http(), new.user_id, EditMember::default().deafen(true))
                .await
                .unwrap();
        }
    }
}

// /// Run a worker that writes metadata to the database.
// pub async fn queuing_worker(
//     mut receiver: mpsc::Receiver<NewAuxMetadata>,
//     ctx: Arc<SerenityContext>,
// ) {
//     while let Some(message) = receiver.recv().await {
//         tracing::trace!("Received message in run_db_worker: {}", message);
//         queue_aux_metadata(ctx, aux_metadata, msg).await;
//     }
// }

// async fn queue_tracks_worker(ctx: Arc<SerenityContext>) {
//     // Wait for work to come in on the message queue
//     let mut queue = ctx.data.write().await.get::<Queue>().unwrap().clone();
//     loop {
//         let track = queue.pop();
//         match track {
//             Some(track) => {
//                 // Do something with the track
//                 tracing::info!("Track: {:?}", track);
//             },
//             None => {
//                 // Wait for a bit before checking the queue again
//                 tokio::time::sleep(Duration::from_secs(1)).await;
//             },
//         }
//     }
// }

// Parked, not abandoned. Its only caller sat behind a literal `if false` in
// `on_cache_ready`, so it has not run for as long as that has been there; removing
// the surrounding block to move `cam_status_loop` off `CacheReady` is what made the
// deadness visible to the compiler. Kept so re-enabling it stays a one-line change.
#[allow(dead_code)]
async fn log_system_load(ctx: Arc<SerenityContext>, config: Arc<BotConfig>) {
    let cpu_load = sys_info::loadavg().unwrap();
    let mem_use = sys_info::mem_info().unwrap();

    // We can use GenericChannelId directly to send a message to a specific channel; in this case, the
    // message would be sent to the #testing channel on the discord server.
    if let Some(chan_id) = config.sys_log_channel_id {
        let message = GenericChannelId::new(chan_id)
            .send_message(
                ctx.http(),
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
        data.id_cache_map.get_mut(&(*guild_id).into());
        if let Some(guild_cache) = data.id_cache_map.get_mut(&(*guild_id).into()) {
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
        match msg.delete(ctx.http(), Some("delete old messages")).await {
            Ok(_) => {},
            Err(err) => {
                tracing::error!("Error deleting message: {}", err);
            },
        }
    }
    Ok(())
}

/// Returns a string describing the difference between two voice states.
pub async fn voice_state_diff_str(
    old: &Option<VoiceState>,
    new: &VoiceState,
    // cache: impl AsRef<serenity::Cache> + AsRef<serenity::Http>,
    cache: Arc<impl serenity::CacheHttp>,
) -> Result<String, CrackedError> {
    let guild_id = new.guild_id;
    let channel = match new.channel_id {
        Some(channel_id) => channel_id
            .widen()
            .to_channel(cache.clone(), guild_id)
            .await
            .ok(),
        None => None,
    };
    let premium = true; //DEFAULT_PREMIUM;
    let old = match old {
        Some(old) => old,
        None => {
            let user_name = &new.member.as_ref().unwrap().user.name;
            let result = match channel {
                Some(channel) => format!(
                    "Member joined voice channel\n{} joined {}",
                    user_name,
                    channel.mention()
                ),
                None => format!(
                    "Member joined voice channel\n{} joined unknown channel",
                    user_name
                ),
            };
            return Ok(result);
        },
    };
    let member = old
        .member
        .as_ref()
        .or(new.member.as_ref())
        .ok_or(CrackedError::Other("voice state update with no member"))?;
    let user = if premium {
        member.user.mention().to_string()
    } else {
        member.user.name.to_string()
    };
    let mut result = String::new();
    if old.channel_id != new.channel_id {
        match (old.channel_id, new.channel_id) {
            (Some(channel_id), None) => {
                let user_name = &member.user.name;
                let user_mention = member.user.mention();
                let channel_mention = channel_id
                    .widen()
                    .to_channel(cache, guild_id)
                    .await?
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

                return Ok(format!(
                    "Member left voice channel\n{} left {}\n",
                    user, channel
                ));
            },
            (None, Some(channel_id)) => {
                let user_name = &member.user.name;
                let channel_mention = channel_id
                    .widen()
                    .to_channel(cache, guild_id)
                    .await?
                    .mention();

                return Ok(format!(
                    "Member joined voice channel\n{} joined {}\n",
                    user_name, channel_mention
                ));
            },
            (Some(old_channel_id), Some(new_channel_id)) => {
                let old_channel_mention = old_channel_id
                    .widen()
                    .to_channel(cache.clone(), guild_id)
                    .await?
                    .mention();
                let new_channel_mention = new_channel_id
                    .widen()
                    .to_channel(cache.clone(), guild_id)
                    .await?
                    .mention();
                result.push_str(&format!(
                    "Switched voice channels: {} -> {}\n",
                    old_channel_mention, new_channel_mention
                ));
            },
            // Unreachable: this block only runs when the two differ.
            (None, None) => {},
        }
    }
    if old.deaf() != new.deaf() {
        if new.deaf() {
            result.push_str(&format!("{} was deafend\n", user));
        } else {
            result.push_str(&format!("{} was undeafend\n", user));
        }
    }
    if old.mute() != new.mute() {
        if new.mute() {
            result.push_str(&format!("{} was muted\n", user));
        } else {
            result.push_str(&format!("{} was unmuted\n", user));
        }
    }
    if old.guild_id != new.guild_id {
        result.push_str(&format!(
            "{} switched guilds?!?! guild_id: {:?} -> {:?}\n",
            user, old.guild_id, new.guild_id
        ));
    }

    if old.self_deaf() != new.self_deaf() {
        if new.self_deaf() {
            result.push_str(&format!("{} deafened themselves\n", user));
        } else {
            result.push_str(&format!("{} undeafened themselves\n", user));
        }
    }
    if old.self_mute() != new.self_mute() {
        if new.self_mute() {
            result.push_str(&format!("{} muted themselves\n", user));
        } else {
            result.push_str(&format!("{} unmuted themselves\n", user));
        }
    }
    if old.self_stream() != new.self_stream() {
        if new.self_stream().unwrap_or(false) {
            result.push_str(&format!("{} started streaming\n", user));
        } else {
            result.push_str(&format!("{} stopped streaming\n", user));
        }
    }
    if old.self_video() != new.self_video() {
        if old.self_video() {
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
    if old.suppress() != new.suppress() {
        result.push_str(&format!(
            "suppress: {:?} -> {:?}\n",
            old.suppress(),
            new.suppress()
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
    Ok(result)
}
