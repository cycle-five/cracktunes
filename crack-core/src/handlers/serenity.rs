use crate::commands::registration::{
    refresh_guilds, sync_global, CommandRegistration, LiveCommandsDiscord,
};
use crate::{
    // commands::queue_aux_metadata,
    commands::music::gp::{handle_gp_component, GP_CUSTOM_ID_PREFIX},
    db::GuildEntity,
    errors::CrackedError,
    guild::settings::{GuildSettings, Provenance, DEFAULT_ACTIVITY},
    handlers::voice_chat_stats::cam_status_loop,
    sources::spotify::{Spotify, SPOTIFY},
    BotConfig,
    Data,
};
use ::serenity::{
    builder::{CreateEmbed, CreateMessage},
    gateway::ActivityData,
};
use colored::Colorize;
// use dashmap;
use poise::serenity_prelude::{self as serenity, Member, Mentionable};
use serenity::CacheHttp;
use serenity::{
    async_trait,
    model::{application::Interaction, gateway::Ready, id::GuildId, prelude::VoiceState},
    GenericChannelId, {Context as SerenityContext, EventHandler, FullEvent},
};
use std::sync::{atomic::Ordering, Arc};

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
            FullEvent::Message { new_message, .. } => {
                // Credits for chatting: counted, never awaited.
                crate::economy::earn::on_message(&self.data, new_message);
            },
            FullEvent::GuildMemberAddition { new_member } => {
                self.on_guild_member_addition(ctx.clone(), new_member.clone())
                    .await;
            },
            FullEvent::VoiceStateUpdate { old, new } => {
                // The mod log first: it never waits, so a leave or move is
                // timed when it arrived. It takes CrackTunes' own changes too
                // (their audit units must be consumed, M8a).
                crate::modlog::live::on_voice_state(&self.data, ctx, old.as_ref(), new);
                // A member who just joined voice has pranks waiting to be
                // undone (their mute could not land while they were away).
                if let (None, Some(guild)) = (old.as_ref().and_then(|o| o.channel_id), new.guild_id)
                {
                    if new.channel_id.is_some() {
                        let discord = crate::economy::discord::LiveEconomyDiscord {
                            http: ctx.http.clone(),
                        };
                        let join = crate::economy::effects::on_voice_join(
                            &self.data,
                            &discord,
                            guild,
                            new.user_id,
                            chrono::Utc::now(),
                        );
                        // Bounded: rooms' handling waits behind this.
                        if tokio::time::timeout(std::time::Duration::from_secs(5), join)
                            .await
                            .is_err()
                        {
                            tracing::warn!(
                                "economy: undoing pranks for {} on joining voice timed out",
                                new.user_id
                            );
                        }
                    }
                    // A walk-on theme waiting for them plays now. Spawned:
                    // it never holds up this event.
                    if let Some(channel) = new.channel_id {
                        crate::economy::theme::on_voice_join(
                            self.data.clone(),
                            ctx.clone(),
                            guild,
                            new.user_id,
                            channel,
                        );
                    }
                }
                // A jailed member who changed channel is moved back.
                if let Some(guild) = new.guild_id {
                    let from = old.as_ref().and_then(|o| o.channel_id);
                    if from != new.channel_id {
                        let discord = crate::economy::discord::LiveEconomyDiscord {
                            http: ctx.http.clone(),
                        };
                        let moved = crate::economy::lock::on_voice_move(
                            &self.data,
                            &discord,
                            guild,
                            new.user_id,
                            new.channel_id,
                            tokio::time::Instant::now(),
                            chrono::Utc::now(),
                        );
                        match tokio::time::timeout(std::time::Duration::from_secs(5), moved).await {
                            // Out again inside the rate limit: one re-check
                            // at its end, so moving twice doesn't escape.
                            Ok(Some(at)) => crate::economy::lock::spawn_jail_recheck(
                                self.data.clone(),
                                ctx.http.clone(),
                                ctx.cache.clone(),
                                guild,
                                new.user_id,
                                at,
                            ),
                            Ok(None) => {},
                            Err(_) => {
                                tracing::warn!(
                                    "economy: moving jailed {} back timed out",
                                    new.user_id
                                );
                            },
                        }
                    }
                }
                // Then voice rooms: the handler below returns early for
                // anyone but the bot.
                crate::rooms::lifecycle::on_voice_state(&self.data, ctx, old.as_ref(), new).await;
                self.on_voice_state_update(ctx.clone(), old.clone(), new.clone())
                    .await;
            },
            // Server mutes and deafens for the mod log. The event log after
            // this match sees the entry too.
            FullEvent::GuildAuditLogEntryCreate { entry, guild_id } => {
                // Read out first: the cache guard must not be held across an await.
                let bot = ctx.cache.current_user().id;
                crate::modlog::live::on_audit_event(&self.data, ctx, *guild_id, entry).await;
                // The lock and moderators' overrides act whether or not the
                // mod log is on.
                let discord = crate::economy::discord::LiveEconomyDiscord {
                    http: ctx.http.clone(),
                };
                let audit = crate::economy::lock::on_audit_entry(
                    &self.data, &discord, *guild_id, entry, bot,
                );
                if tokio::time::timeout(std::time::Duration::from_secs(5), audit)
                    .await
                    .is_err()
                {
                    tracing::warn!("economy: handling an audit entry in {guild_id} timed out");
                }
            },
            // A voice room deleted by hand drops its row and its timer.
            FullEvent::ChannelDelete { channel, .. } => {
                crate::rooms::lifecycle::on_channel_delete(&self.data, channel.id).await;
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
            // Now-playing buttons: routed by custom-id prefix, like `/gp`'s.
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(mci),
            } if mci
                .data
                .custom_id
                .starts_with(crate::messaging::buttons::NP_PREFIX) =>
            {
                crate::messaging::buttons::handle(&self.data, ctx, mci).await;
            },
            // Voice room panel buttons (`room:`), and the modals some of them
            // open (`roomm:`). Neither prefix starts with the other.
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(mci),
            } if mci
                .data
                .custom_id
                .starts_with(crate::rooms::ids::ROOM_PREFIX) =>
            {
                crate::rooms::interact::handle_press(&self.data, ctx, mci).await;
            },
            FullEvent::InteractionCreate {
                interaction: Interaction::Modal(mi),
            } if mi
                .data
                .custom_id
                .starts_with(crate::rooms::ids::ROOM_MODAL_PREFIX) =>
            {
                crate::rooms::interact::handle_submit(&self.data, ctx, mi).await;
            },
            // Prank card buttons (`econ:`), and the modal Rename them back
            // opens (`econm:`). Neither prefix starts with the other.
            FullEvent::InteractionCreate {
                interaction: Interaction::Component(mci),
            } if mci
                .data
                .custom_id
                .starts_with(crate::economy::buttons::BUTTON_PREFIX) =>
            {
                crate::economy::buttons::handle_press(&self.data, ctx, mci).await;
            },
            FullEvent::InteractionCreate {
                interaction: Interaction::Modal(mi),
            } if mi
                .data
                .custom_id
                .starts_with(crate::economy::buttons::MODAL_PREFIX) =>
            {
                crate::economy::buttons::handle_submit(&self.data, ctx, mi).await;
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

        // Sync THIS build's command list to Discord. The registration mode
        // decides the set: every command globally, or only `/register`.
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
        let mode = self.data.bot_settings.command_registration;
        let commands = crate::commands::commands_for(mode);
        let api = LiveCommandsDiscord {
            http: ctx.http.clone(),
        };
        static COMMANDS_REGISTERED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        if !COMMANDS_REGISTERED.swap(true, Ordering::SeqCst) {
            match sync_global(&api, mode, &commands).await {
                Ok(n) => {
                    tracing::info!("Registered {n} global commands");
                    if mode == CommandRegistration::Manual {
                        tracing::info!(
                            "Command registration is manual: only /register is global, and servers opt in with it"
                        );
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to register global commands: {e}");
                    // Clear the gate so a later `Ready` retries; otherwise one
                    // transient failure leaves the process with no commands.
                    COMMANDS_REGISTERED.store(false, Ordering::SeqCst);
                },
            }
        }

        // Every `Ready` carries its own shard's servers. A server that opted in
        // gets this build's list; a repeat after a reconnect rewrites the same
        // set, which is harmless.
        if mode == CommandRegistration::Manual {
            let guilds: Vec<GuildId> = ready.guilds.iter().map(|g| g.id).collect();
            tokio::spawn(async move {
                let r = refresh_guilds(&api, mode, &commands, &guilds).await;
                tracing::info!(
                    "Refreshed commands in {} of {} servers ({} failed)",
                    r.refreshed,
                    r.checked,
                    r.failed
                );
            });
        }

        // Whether a Spotify LINK will resolve. This is the question an operator
        // actually has at boot, and it is no longer the same question as the
        // credentials check below: links go through sleevenote and need no
        // credentials, while the credentials gate only autoplay. Reporting one
        // and not the other is how "Spotify is broken" came to mean two
        // unrelated things.
        if crate::sources::sleevenote::is_configured() {
            tracing::info!(
                "{} {}",
                crate::messaging::messages::SLEEVENOTE_CONFIGURED_LOG,
                std::env::var(crack_sleevenote::BASE_URL_ENV).unwrap_or_default(),
            );
        } else {
            tracing::warn!(
                "{}",
                crate::messaging::messages::SLEEVENOTE_UNCONFIGURED_LOG
            );
        }

        // Attempt to authenticate to Spotify, and SAY SO EITHER WAY.
        //
        // This result used to be stored and never read, so a bot booting with no
        // Spotify credentials looked identical to one booting with working ones.
        // The first anybody learned of it was a user pasting a Spotify link and
        // getting an error -- or, worse, autoplay quietly switching itself off.
        //
        // Credentials now gate AUTOPLAY ONLY; see SPOTIFY_DISABLED_LOG.
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

        // The economy's once-a-minute payout. Here, not in `on_cache_ready`:
        // that event never fires on a bot in many servers. A repeat `Ready`
        // (reconnect) is harmless: the tick starts once per process. Placed
        // before the loops below in case one of them awaits for long.
        if let Some(cache) = ctx.cache() {
            crate::economy::earn::start(self.data.clone(), cache.clone());
        }
        // And the sweeper that ends timed pranks on time.
        crate::economy::effects::start_sweeper(
            self.data.clone(),
            ctx.http.clone(),
            ctx.cache.clone(),
        );

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
                #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
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

        // Only a disconnect (no channel) needs handling here.
        if new.channel_id.is_some() {
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
        // 🪤 Not `.ok()`. songbird's `remove` is `leave(..)?` *then*
        // `calls.remove(..)`, so a failing leave skips the removal and leaves
        // the connectionless `Call` registered -- the stale handle the whole
        // join path is built to defend against, since a later join finds it
        // and reports success without ever connecting. This is the handler
        // that runs when the bot is KICKED or disconnected, which is exactly
        // when the gateway is unhappy and `leave` is most likely to fail, so
        // it is the worst place in the crate to discard that signal (#505).
        //
        // `NoCall` is not a failure: it means nothing was registered, which is
        // the ordinary case for a disconnect we were never party to. The
        // `manager.get` guard that used to stand in for this test is gone --
        // it asked songbird the same question twice.
        if let Err(e) = crate::music::disconnect::disconnect(
            &self.data,
            &manager,
            guild_id,
            crate::music::audit::Actor::bot(crate::music::audit::BotReason::Kicked),
        )
        .await
        {
            if !matches!(e, songbird::error::JoinError::NoCall) {
                tracing::warn!(
                    "Could not remove the Call for {guild_id:?} after being disconnected: \
                     {e:?}. A later join may find it and skip the permission gate."
                );
            }
        }

        // Kicked or disconnected: the status says playback finished.
        crate::messaging::status::show_finished(
            &self.data,
            ctx.http.clone(),
            ctx.cache.clone(),
            guild_id,
        )
        .await;

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
    async fn on_guild_create(&self, ctx: SerenityContext, guild: serenity::Guild) {
        // First, before any await below: a re-identify must mark the mod log's
        // existing snapshots stale before a flush can run against them.
        crate::modlog::live::mark_reconnected(&self.data, guild.id);
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
                // 🔑 The ONLY place settings become persistable. get_or_create
                // is an upsert returning the live row, so Ok means Postgres and
                // memory agree the row exists -- including for a guild just
                // joined. The Err and no-pool arms below deliberately keep the
                // Fallback default, so a transient read failure cannot become a
                // permanent write at shutdown.
                Ok((_guild, settings)) => settings.with_provenance(Provenance::Database),
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

        // Voice room settings live in their own table; with no database,
        // rooms stay off. A failed read leaves rooms off and room settings
        // unsaveable for the server until a read succeeds: reconcile below
        // tries again, then once more a minute later.
        if let Err(err) = crate::rooms::ops::load_settings(&self.data, guild.id).await {
            tracing::warn!(
                "rooms: loading the room settings of {} failed: {err}",
                guild.id
            );
        }

        // The economy's settings and opted-out members, likewise (off without
        // a database). A failed read leaves the economy unsaveable and
        // earning nothing for the server until a read succeeds.
        if !crate::economy::load_settings(&self.data, guild.id).await {
            tracing::warn!("economy: loading the settings of {} failed", guild.id);
            // One retry after 60 s, in the background. If it also fails the
            // server stays unloaded (earning nothing, refusing saves) until
            // the next GuildCreate, i.e. a reconnect or a restart.
            let data = self.data.clone();
            let gid = guild.id;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                // A newer load (a reconnect's GuildCreate) may have succeeded
                // meanwhile; this retry must not overwrite it.
                if data.economy.loaded(gid) {
                    return;
                }
                if !crate::economy::load_settings(&data, gid).await {
                    tracing::warn!("economy: retrying the settings of {gid} failed too");
                }
            });
        }

        // Pranks the last run left behind: pending purchases resolved, rows
        // that came due while the bot was down finished. After the settings
        // load, though it needs none: the sweeper runs with the economy off.
        if let Some(cache) = ctx.cache().cloned() {
            let data = self.data.clone();
            let http = ctx.http.clone();
            let gid = guild.id;
            // Off the handler: Discord calls here must not delay the work below.
            tokio::spawn(async move {
                let discord = crate::economy::discord::LiveEconomyDiscord { http };
                crate::economy::effects::reconcile(
                    &data,
                    &discord,
                    cache.as_ref(),
                    gid,
                    chrono::Utc::now(),
                )
                .await;
            });
        }

        // The mod log's settings, likewise (off without a database); a server
        // with the log on has its audit snapshots seeded in the background.
        crate::modlog::live::on_guild_create(&self.data, ctx.http.clone(), guild.id).await;

        // A `/gp` game that was running when the bot went down comes back here,
        // per guild, for the same reason the settings do.
        crate::commands::music::gp_persist::gp_resume_guild(&self.data, &ctx, &guild).await;
        // And an ordinary queue that was playing when the bot went down (#595).
        // After /gp's: a guild whose game came back is not resumed twice.
        crate::music::resume::queue_resume_guild(&self.data, &ctx, &guild).await;
        // Voice rooms: drop rows for vanished channels, sweep empty rooms,
        // give the hub's occupants rooms. Runs on every reconnect too.
        crate::rooms::lifecycle::reconcile_guild(&self.data, &ctx, &guild).await;
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
#[expect(
    dead_code,
    reason = "parked: its only caller sat behind `if false` in on_cache_ready; kept so re-enabling is one line"
)]
async fn log_system_load(ctx: Arc<SerenityContext>, config: Arc<BotConfig>) {
    let cpu_load = sys_info::loadavg().unwrap();
    let mem_use = sys_info::mem_info().unwrap();

    // We can use GenericChannelId directly to send a message to a specific channel; in this case, the
    // message would be sent to the #testing channel on the discord server.
    if let Some(chan_id) = config.sys_log_channel_id {
        #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
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

#[cfg(test)]
mod provenance_wiring_tests {
    /// `on_guild_create` is an async serenity event handler taking a live
    /// `Context`; it cannot be called from a unit test. The property that
    /// matters is which of its three arms marks settings persistable, so this
    /// reads the source.
    ///
    /// If this test becomes annoying, the fix is to extract the arm selection
    /// into a pure function and test that -- not to delete the test.
    #[test]
    fn only_the_database_arm_marks_settings_persistable() {
        // `include_str!` keeps the checkout's newlines. A CRLF working tree
        // would miss a marker that contains `\n` and then count this test.
        let src = include_str!("serenity.rs").replace("\r\n", "\n");

        // Scan only the handler code above this test module. `include_str!`
        // pulls in this test's own source too, and its filter predicate and
        // assertion messages necessarily contain the string "with_provenance"
        // -- left unscoped, the test would count itself and always fail.
        let production_src = src
            .split_once("#[cfg(test)]\nmod provenance_wiring_tests")
            .map(|(before, _)| before)
            .unwrap_or(&src);

        let marks: Vec<&str> = production_src
            .lines()
            .filter(|l| l.contains("with_provenance"))
            .map(|l| l.trim())
            .collect();

        assert_eq!(
            marks.len(),
            1,
            "expected exactly one with_provenance call in this file, found {}: {:#?}",
            marks.len(),
            marks
        );
        assert!(
            marks[0].contains("Provenance::Database"),
            "the only with_provenance call should mark Database: {}",
            marks[0]
        );
        assert!(
            marks[0].contains("Ok(("),
            "with_provenance must be on the Ok arm of get_or_create, so a failed \
             load keeps the Fallback default: {}",
            marks[0]
        );
    }
}
