use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use crate::{
    errors::CrackedError,
    guild::{load_guilds_settings, settings::GuildSettings},
    handlers::serenity::voice_state_diff_str,
    sources::spotify::{Spotify, SPOTIFY},
    utils::create_log_embed,
    Data, Error,
};
use colored::Colorize;
use poise::{
    serenity_prelude::{Activity, ChannelId, ClientStatus, GuildId, Member, Presence},
    Event::*,
};
use serde::{ser::SerializeStruct, Serialize};
use serenity::{client::Context as SerenityContext, http::Http};

#[derive(Debug)]
pub struct LogEntry<T: Serialize> {
    pub name: String,
    pub notes: String,
    pub event: T,
}

impl<T: Serialize> Serialize for LogEntry<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let n = if self.notes.is_empty() { 2 } else { 3 };
        let mut state = serializer.serialize_struct("LogEntry", n)?;
        state.serialize_field("name", &self.name)?;
        if !self.notes.is_empty() {
            state.serialize_field("notes", &self.notes)?;
        }
        state.serialize_field("event", &self.event)?;
        state.end()
    }
}

pub fn get_log_channel(
    channel_name: &str,
    guild_id: &GuildId,
    data: &Data,
) -> Option<serenity::model::id::ChannelId> {
    let guild_settings_map = data.guild_settings_map.lock().unwrap().clone();
    guild_settings_map
        .get(&guild_id.into())
        .map(|x| x.get_log_channel(channel_name))
        .unwrap()
}

// #[derive(Clone)]
// pub struct HandleEventData<'a> {
//     guild_settings: Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
//     event: &'a poise::Event<'a>,
//     ctx: &'a SerenityContext,
// }

pub async fn get_channel_id(
    guild_settings_map: &Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
    guild_id: &GuildId,
    event: &poise::Event<'_>,
) -> Result<ChannelId, CrackedError> {
    let initial_values = vec![1165246445654388746];
    let hashset: HashSet<_> = initial_values.into_iter().collect();
    let guild_settings = match guild_settings_map.lock() {
        Ok(x) => x.clone(),
        Err(e) => {
            let err_string = e.to_string();
            tracing::error!("Failed to lock guild_settings_map: {err_string}");
            return Err(CrackedError::LogChannelWarning(event.name(), *guild_id));
        }
    };
    let ignore_channels = &guild_settings
        .get(guild_id)
        .map(|x| x.ignored_channels.clone())
        .unwrap_or(hashset);
    match guild_settings_map
        .lock()
        .unwrap()
        .entry(*guild_id)
        .or_default()
        .get_log_channel_type(event)
    {
        Some(channel_id) => {
            if ignore_channels.contains(&channel_id.0) {
                return Err(CrackedError::LogChannelWarning(event.name(), *guild_id));
            }
            Ok(channel_id)
        }
        None => Err(CrackedError::LogChannelWarning(event.name(), *guild_id)),
    }
}

macro_rules! log_event {
    // #[cfg(feature="no_log")]
    ($log_func:expr, $guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $http:expr, $event_log:expr, $event_name:expr) => {{
        let channel_id = get_channel_id($guild_settings, $guild_id, $event).await?;
        $log_func(channel_id, $http, $log_data).await?;
        $event_log.write_log_obj($event_name, $log_data)
    }};
}

pub async fn log_unimplemented_event<T: Serialize>(
    channel_id: ChannelId,
    _http: &Arc<Http>,
    _log_data: T,
) -> Result<serenity::model::prelude::Message, Error> {
    Err(CrackedError::UnimplementedEvent(channel_id, std::any::type_name::<T>()).into())
}

pub async fn log_guild_member_removal(
    channel_id: ChannelId,
    http: &Arc<Http>,
    log_data: &(&GuildId, &serenity::model::prelude::User, &Option<Member>),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(_guild_id, user, member_data_if_available) = log_data;
    let title = format!("Member Left: {}", user.name);
    let description = format!(
        "User: {}\nID: {}\nAccount Created: {}\nJoined: {:?}",
        user.name,
        user.id,
        user.created_at(),
        member_data_if_available.clone().and_then(|m| m.joined_at)
    );
    let avatar_url = user.avatar_url().unwrap_or_default();
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

pub async fn log_guild_member_addition(
    channel_id: ChannelId,
    http: &Arc<Http>,
    new_member: &Member,
) -> Result<serenity::model::prelude::Message, Error> {
    let title = format!("Member Joined: {}", new_member.user.name);
    let description = format!(
        "User: {}\nID: {}\nAccount Created: {}\nJoined: {:?}",
        new_member.user.name,
        new_member.user.id,
        new_member.user.created_at(),
        new_member.joined_at
    );
    let avatar_url = new_member.user.avatar_url().unwrap_or_default();
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

struct PresencePrinter {
    presence: Option<Presence>,
}

impl std::fmt::Display for PresencePrinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(presence) = self.presence.clone() {
            let activities_str = presence
                .activities
                .iter()
                .map(|activity| format!("{}\n", ActivityPrinter { activity }))
                .collect::<Vec<_>>()
                .join(", ");
            write!(
                f,
                "Status: {}\nClientStatus: {}\nActivities: {}\nGuildId: {}\nUser: {}\n",
                presence.status.name(),
                presence
                    .client_status
                    .map(|x| ClientStatusPrinter {
                        client_status: Some(x)
                    }
                    .to_string())
                    .unwrap_or_default(),
                activities_str,
                presence.guild_id.map(|x| x.to_string()).unwrap_or_default(),
                PresenceUserPrinter {
                    user: presence.user
                }
            )
        } else {
            write!(f, "None")
        }
    }
}

struct PresenceUserPrinter {
    user: serenity::model::prelude::PresenceUser,
}

impl std::fmt::Display for PresenceUserPrinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
                f,
                "User: {:?}\nID: {:?}\nDiscriminator: {:?}\nAvatar: {:?}\nBot: {:?}\nMFA Enabled: {:?}\nVerified: {:?}\nEmail: {:?}\nPublic Flags: {:?}\n",
                self.user.name,
                self.user.id,
                self.user.discriminator,
                self.user.avatar,
                self.user.bot,
                self.user.mfa_enabled,
                self.user.verified,
                self.user.email,
                self.user.public_flags,
            )
    }
}

struct ActivityPrinter<'a> {
    activity: &'a serenity::model::prelude::Activity,
}

impl std::fmt::Display for ActivityPrinter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let activity = self.activity.clone();
        let mut activity_str = String::new();
        if let Some(url) = activity.url {
            activity_str.push_str(&format!("URL: {}\n", url));
        }
        if let Some(application_id) = activity.application_id {
            activity_str.push_str(&format!("Application ID: {}\n", application_id));
        }
        if let Some(timestamps) = activity.timestamps {
            activity_str.push_str(&format!("Timestamps: {:?}\n", timestamps));
        }
        if let Some(details) = activity.details {
            activity_str.push_str(&format!("Details: {}\n", details));
        }
        if let Some(state) = activity.state {
            activity_str.push_str(&format!("State: {}\n", state));
        }
        if let Some(emoji) = activity.emoji {
            activity_str.push_str(&format!("Emoji: {:?}\n", emoji));
        }
        if let Some(party) = activity.party {
            activity_str.push_str(&format!("Party: {:?}\n", party));
        }
        if let Some(assets) = activity.assets {
            activity_str.push_str(&format!("Assets: {:?}\n", assets));
        }
        if let Some(secrets) = activity.secrets {
            activity_str.push_str(&format!("Secrets: {:?}\n", secrets));
        }
        if let Some(instance) = activity.instance {
            activity_str.push_str(&format!("Instance: {:?}\n", instance));
        }
        if let Some(flags) = activity.flags {
            activity_str.push_str(&format!("Flags: {:?}\n", flags));
        }
        write!(f, "{}", activity_str)
    }
}
struct ClientStatusPrinter {
    client_status: Option<ClientStatus>,
}

impl std::fmt::Display for ClientStatusPrinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(client_status) = self.client_status.clone() {
            let desktop = client_status.desktop.unwrap_or_default();
            let mobile = client_status.mobile.unwrap_or_default();
            let web = client_status.web.unwrap_or_default();
            write!(
                f,
                "Desktop: {}\nMobile: {}\nWeb: {}",
                desktop.name(),
                mobile.name(),
                web.name()
            )
        } else {
            write!(f, "None")
        }
    }
}

pub async fn log_presence_update(
    channel_id: ChannelId,
    http: &Arc<Http>,
    new_data: &Presence,
) -> Result<serenity::model::prelude::Message, Error> {
    let presence_str = PresencePrinter {
        presence: Some(new_data.clone()),
    }
    .to_string();

    let title = format!(
        "Presence Update: {}",
        new_data.user.name.clone().unwrap_or_default()
    );
    let description = presence_str;
    let avatar_url = format!(
        "https://cdn.discordapp.com/{user_id}/{user_avatar}.png",
        user_id = new_data.user.id,
        user_avatar = new_data.user.avatar.clone().unwrap_or_default(),
    );
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

pub async fn log_voice_state_update(
    channel_id: ChannelId,
    http: &Arc<Http>,
    log_data: &(
        &Option<serenity::model::prelude::VoiceState>,
        &serenity::model::prelude::VoiceState,
    ),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(old, new) = log_data;
    let title = format!("Voice State Update: {}", new.user_id);
    let description = voice_state_diff_str(old, new);

    let avatar_url = new
        .member
        .clone()
        .and_then(|x| x.user.avatar_url())
        .unwrap_or_default();
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

pub async fn log_typing_start(
    channel_id: ChannelId,
    http: &Arc<Http>,
    event: &serenity::model::prelude::TypingStartEvent,
) -> Result<serenity::model::prelude::Message, Error> {
    let user = event.user_id.to_user(http.clone()).await?;
    let channel_name = http
        .get_channel(channel_id.0)
        .await
        .ok()
        .map(|x| x.to_string())
        .unwrap_or_default();
    let guild = event
        .guild_id
        .unwrap_or_default()
        .to_partial_guild(http.clone())
        .await?
        .name;
    tracing::info!(
        "{}{} / {} / {} / {}",
        "TypingStart: ".bright_green(),
        user.name.bright_yellow(),
        user.id.to_string().bright_yellow(),
        channel_name.bright_yellow(),
        guild.bright_yellow(),
    );
    let title = format!("Typing Start: {}", event.user_id);
    let description = format!(
        "User: {}\nID: {}\nChannel: {}",
        user.name, event.user_id, event.channel_id
    );
    let avatar_url = user.avatar_url().unwrap_or_default();
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

pub async fn log_message(
    channel_id: ChannelId,
    http: &Arc<Http>,
    new_message: &serenity::model::prelude::Message,
) -> Result<serenity::model::prelude::Message, Error> {
    let title = format!("Message: {}", new_message.author.name);
    let description = format!(
        "User: {}\nID: {}\nChannel: {}\nMessage: {}",
        new_message.author.name, new_message.author.id, new_message.channel_id, new_message.content
    );
    let avatar_url = new_message.author.avatar_url().unwrap_or_default();
    create_log_embed(&channel_id, http, &title, &description, &avatar_url).await
}

pub async fn handle_event(
    ctx: &SerenityContext,
    event_in: &poise::Event<'_>,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = Arc::new(&data_global.event_log);
    let event_name = event_in.name();
    let guild_settings = &data_global.guild_settings_map;
    match event_in {
        PresenceUpdate { new_data } => {
            #[cfg(feature = "log_all")]
            {
                log_event!(
                    log_presence_update,
                    guild_settings,
                    event_in,
                    new_data,
                    &new_data.guild_id.unwrap(),
                    &ctx.http,
                    event_log,
                    event_name
                )
            }
            #[cfg(not(feature = "log_all"))]
            {
                let _ = new_data;
                Ok(())
            }
        }
        GuildMemberAddition { new_member } => {
            log_event!(
                log_guild_member_addition,
                guild_settings,
                event_in,
                new_member,
                &new_member.guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        poise::Event::GuildMemberRemoval {
            guild_id,
            user,
            member_data_if_available,
        } => {
            let log_data = (guild_id, user, member_data_if_available);
            log_event!(
                log_guild_member_removal,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        VoiceStateUpdate { old, new } => {
            let log_data = &(old, new);
            log_event!(
                log_voice_state_update,
                guild_settings,
                event_in,
                log_data,
                &new.guild_id.unwrap(),
                &ctx.http,
                event_log,
                event_name
            )
        }
        Message { new_message } => {
            if new_message.author.id == ctx.cache.current_user_id() {
                return Ok(());
            }
            log_event!(
                log_message,
                guild_settings,
                event_in,
                new_message,
                &new_message.guild_id.unwrap(),
                &ctx.http,
                event_log,
                event_name
            )
        }
        TypingStart { event } => {
            // let cache_http = ctx.http.clone()
            log_event!(
                log_typing_start,
                guild_settings,
                event_in,
                event,
                &event.guild_id.unwrap_or_default(),
                &ctx.http,
                event_log,
                event_name
            )
        }
        ApplicationCommandPermissionsUpdate { permission } => {
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                permission,
                &permission.guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        AutoModerationActionExecution { execution } => {
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                execution,
                &execution.guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        AutoModerationRuleCreate { rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        AutoModerationRuleUpdate { rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        AutoModerationRuleDelete { rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        CategoryCreate { category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            category,
            &category.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        CategoryDelete { category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            category,
            &category.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        ChannelDelete { channel } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            channel,
            &channel.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        ChannelPinsUpdate { pin } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            pin,
            &pin.guild_id.unwrap_or_default(),
            &ctx.http,
            event_log,
            event_name
        ),
        ChannelUpdate { old, new } => {
            let guild_id = new.clone().guild().map_or(GuildId(0), |x| x.guild_id);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &(old, new),
                &guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        poise::Event::GuildBanAddition {
            guild_id,
            banned_user,
        } => {
            let log_data = (guild_id, banned_user);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        poise::Event::GuildBanRemoval {
            guild_id,
            unbanned_user,
        } => {
            let log_data = (guild_id, unbanned_user);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(feature = "cache")]
        GuildCreate { guild } => {
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &guild,
                &guild.id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(not(feature = "cache"))]
        GuildCreate { guild, is_new } => {
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &(guild, is_new),
                &guild.id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(feature = "cache")]
        GuildDelete { incomplete } => {
            let log_data = (incomplete);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &incomplete.id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(not(feature = "cache"))]
        GuildDelete { incomplete, full } => {
            let log_data = (incomplete, full);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &incomplete.id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        GuildEmojisUpdate {
            guild_id,
            current_state,
        } => {
            let log_data = (guild_id, current_state);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        GuildIntegrationsUpdate { guild_id } => {
            let log_data = guild_id;
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(feature = "cache")]
        poise::Event::GuildMemberUpdate {
            old_if_available,
            new,
        } => {
            let log_data = (old_if_available, new);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &new.guild_id,
                &ctx.http,
                event_log,
                event_name
            )
        }
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildMemberUpdate {
            old_if_available,
            new,
        } => {
            let guild_settings = data_global.guild_settings_map.lock().unwrap().clone();
            let maybe_log_channel = guild_settings
                .get(&new.guild_id)
                .map(|x| x.get_join_leave_log_channel())
                .unwrap_or(None);

            let description = format!(
                "User: {}\nID: {}\nAccount Created: {}\nJoined: {:?}",
                new.user.name,
                new.user.id,
                new.user.created_at(),
                new.joined_at
            );
            let mut avatar_url = new.avatar_url().unwrap_or(
                old_if_available
                    .clone()
                    .map(|x| x.avatar_url().unwrap_or_default())
                    .unwrap_or_default(),
            );
            if avatar_url.is_empty() {
                avatar_url = new.user.avatar_url().unwrap_or_default().clone();
            }
            if avatar_url.is_empty() {
                avatar_url = new.user.default_avatar_url().clone();
            }

            let mut notes = "";
            let mut title: String = String::from("");

            match (maybe_log_channel, old_if_available) {
                (Some(_), Some(old)) => {
                    if old.pending && !new.pending {
                        notes = "Click Verify";
                        title = format!("Member Approved: {}", new.user.name);
                    } else {
                        title = format!("Member Updated: {}", new.user.name);
                    };
                }
                (None, Some(old)) => {
                    if old.pending && !new.pending {
                        notes = "Click Verify";
                        title = format!("Member Approved: {}", new.user.name);
                    } else {
                        title = format!("Member Updated: {}", new.user.name);
                    };
                    tracing::warn!("No join/leave log channel set for guild {}", new.guild_id);
                    tracing::warn!(title);
                }
                (Some(_), None) => {
                    title = format!("Member Updated: {}", new.user.name);
                }
                (None, None) => {
                    tracing::warn!("No join/leave log channel set for guild {}", new.guild_id);
                }
            }
            match maybe_log_channel {
                Some(channel_id) => {
                    create_log_embed(&channel_id, &ctx.http, &title, &description, &avatar_url)
                        .await?;
                }
                None => {
                    tracing::warn!("No join/leave log channel set for guild {}", new.guild_id);
                }
            }
            event_log.write_log_obj_note(event_name, Some(notes), &(old_if_available, new))
        }
        GuildMembersChunk { chunk } => event_log.write_log_obj(event_name, chunk),
        poise::Event::GuildRoleCreate { new } => event_log.write_log_obj(event_name, new),
        #[cfg(feature = "cache")]
        poise::Event::GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_global_if_available,
        } => event_log.write_log_obj(
            event_name,
            &(
                guild_id,
                removed_role_id,
                removed_role_data_global_if_available,
            ),
        ),
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_if_available,
        } => event_log.write_log_obj(
            event_name,
            &(guild_id, removed_role_id, removed_role_data_if_available),
        ),
        #[cfg(feature = "cache")]
        poise::Event::GuildRoleUpdate {
            old_data_global_if_available,
            new,
        } => event_log.write_log_obj(event_name, &(old_data_global_if_available, new)),
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildRoleUpdate {
            new,
            old_data_if_available,
        } => event_log.write_log_obj(event_name, &(new, old_data_if_available)),
        poise::Event::GuildScheduledEventCreate { event } => {
            event_log.write_log_obj(event_name, event)
        }
        poise::Event::GuildScheduledEventUpdate { event } => {
            event_log.write_log_obj(event_name, event)
        }
        GuildScheduledEventDelete { event } => event_log.write_log_obj(event_name, event),
        poise::Event::GuildScheduledEventUserAdd { subscribed } => {
            event_log.write_log_obj(event_name, subscribed)
        }
        GuildScheduledEventUserRemove { unsubscribed } => {
            event_log.write_log_obj(event_name, unsubscribed)
        }
        poise::Event::GuildStickersUpdate {
            guild_id,
            current_state,
        } => event_log.write_log_obj(event_name, &(guild_id, current_state)),
        GuildUnavailable { guild_id } => event_log.write_log_obj(event_name, &guild_id),
        #[cfg(feature = "cache")]
        poise::Event::GuildUpdate {
            old_data_global_if_available,
            new_but_incomplete,
        } => event_log.write_log_obj(
            event_name,
            &(old_data_global_if_available, new_but_incomplete),
        ),
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildUpdate {
            new_but_incomplete,
            old_data_if_available,
        } => event_log.write_log_obj(event_name, &(new_but_incomplete, old_data_if_available)),
        IntegrationCreate { integration } => event_log.write_log_obj(event_name, integration),
        IntegrationUpdate { integration } => event_log.write_log_obj(event_name, integration),
        poise::Event::IntegrationDelete {
            integration_id,
            guild_id,
            application_id,
        } => event_log.write_log_obj(event_name, &(integration_id, guild_id, application_id)),
        InteractionCreate { interaction } => event_log.write_log_obj(event_name, interaction),
        InviteCreate { data } => event_log.write_log_obj(event_name, data),
        InviteDelete { data } => event_log.write_log_obj(event_name, data),
        MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
        } => event_log.write_obj(&(channel_id, deleted_message_id, guild_id)),
        MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids,
            guild_id,
        } => event_log.write_obj(&(channel_id, multiple_deleted_messages_ids, guild_id)),
        #[cfg(feature = "cache")]
        poise::Event::MessageUpdate {
            old_if_available,
            new,
            event,
        } => event_log.write_obj(event_name, &(old_if_available, new, event)),
        #[cfg(not(feature = "cache"))]
        poise::Event::MessageUpdate {
            old_if_available,
            new,
            event,
        } => event_log.write_log_obj(event_name, &(old_if_available, new, event)),
        ReactionAdd { add_reaction } => event_log.write_log_obj(event_name, add_reaction),
        ReactionRemove { removed_reaction } => {
            event_log.write_log_obj(event_name, removed_reaction)
        }
        poise::Event::ReactionRemoveAll {
            channel_id,
            removed_from_message_id,
        } => event_log.write_log_obj(event_name, &(channel_id, removed_from_message_id)),
        PresenceReplace { new_presences } => event_log.write_log_obj(event_name, new_presences),
        Ready { data_about_bot } => {
            tracing::info!("{} is connected!", data_about_bot.user.name);

            // ctx.set_activity(Activity::listening(format!(
            //     "{}play",
            //     data_global.bot_settings.get_prefix()
            // )))
            // .await;

            // // attempts to authenticate to spotify
            // *SPOTIFY.lock().await = Spotify::auth(None).await;

            // // loads serialized guild settings
            // tracing::warn!("Loading guilds' settings");
            // let guild_settings_map =
            //     load_guilds_settings(&ctx, &data_about_bot.guilds, data_global).await;

            // // These are the guild settings defined in the config file.
            // // Should they always override the ones in the database?
            // // tracing::warn!("Merging guilds' settings");
            // // self.merge_guild_settings(&ctx, &ready, self.data.guild_settings_map.clone())
            // //     .await;

            // // *self.data.guild_settings_map.lock().unwrap() = guild_settings_map;
            // // let mut guild_settings_map = self.data().guild_settings_map.lock().unwrap();
            // data_global
            //     .guild_settings_map
            //     .lock()
            //     .unwrap()
            //     .iter()
            //     .for_each(|(k, v)| {
            //         tracing::warn!("Saving Guild: {}", k);
            //         v.save().expect("Error saving guild settings");
            //     });
            event_log.write_log_obj(event_name, data_about_bot)
        }
        Resume { event } => event_log.write_log_obj(event_name, event),
        StageInstanceCreate { stage_instance } => {
            event_log.write_log_obj(event_name, stage_instance)
        }
        StageInstanceDelete { stage_instance } => {
            event_log.write_log_obj(event_name, stage_instance)
        }
        StageInstanceUpdate { stage_instance } => {
            event_log.write_log_obj(event_name, stage_instance)
        }
        ThreadCreate { thread } => event_log.write_log_obj(event_name, thread),
        ThreadDelete { thread } => event_log.write_log_obj(event_name, thread),
        ThreadListSync { thread_list_sync } => {
            event_log.write_log_obj(event_name, thread_list_sync)
        }
        ThreadMemberUpdate { thread_member } => event_log.write_log_obj(event_name, thread_member),
        poise::Event::ThreadMembersUpdate {
            thread_members_update,
        } => event_log.write_log_obj(event_name, thread_members_update),
        ThreadUpdate { thread } => event_log.write_log_obj(event_name, thread),
        Unknown { name, raw } => event_log.write_log_obj(event_name, &(name, raw)),
        #[cfg(feature = "cache")]
        poise::Event::UserUpdate {
            old_data_global,
            new,
        } => event_log.write_log_obj(&(old_data_global, new)),
        #[cfg(not(feature = "cache"))]
        UserUpdate { old_data, new } => event_log.write_log_obj(event_name, &(old_data, new)),
        VoiceServerUpdate { update } => event_log.write_log_obj(event_name, update),
        #[cfg(feature = "cache")]
        VoiceStateUpdate { old, new } => event_log.write_obj(&(old, new)),
        WebhookUpdate {
            guild_id,
            belongs_to_channel_id,
        } => event_log.write_obj(&(guild_id, belongs_to_channel_id)),
        CacheReady { guilds } => {
            tracing::info!("{}: {}", event_in.name().bright_green(), guilds.len());
            Ok(())
        }
        _ => {
            tracing::info!("{}", event_in.name().bright_green());
            Ok(())
        }
    }
}

// #[allow(dead_code)]
// enum EventTypes {
//     PresenceUpdate,
//     GuildMemberAddition,
//     VoiceStateUpdate,
//     Message,
//     TypingStart,
//     ApplicationCommandPermissionsUpdate,
//     AutoModerationActionExecution,
//     AutoModerationRuleCreate,
//     AutoModerationRuleUpdate,
//     AutoModerationRuleDelete,
//     CategoryCreate,
//     CategoryDelete,
//     ChannelDelete,
//     ChannelPinsUpdate,
//     ChannelUpdate,
//     GuildBanAddition,
//     GuildBanRemoval,
//     GuildCreate,
//     GuildDelete,
//     GuildEmojisUpdate,
//     GuildIntegrationsUpdate,
//     GuildMemberRemoval,
//     GuildMemberUpdate,
//     GuildMembersChunk,
//     GuildRoleCreate,
//     GuildRoleDelete,
//     GuildRoleUpdate,
//     GuildScheduledEventCreate,
//     GuildScheduledEventUpdate,
//     GuildScheduledEventDelete,
//     GuildScheduledEventUserAdd,
//     GuildScheduledEventUserRemove,
//     GuildStickersUpdate,
//     GuildUnavailable,
//     GuildUpdate,
//     IntegrationCreate,
//     IntegrationUpdate,
//     IntegrationDelete,
//     InteractionCreate,
//     InviteCreate,
//     InviteDelete,
//     MessageDelete,
//     MessageDeleteBulk,
//     MessageUpdate,
//     ReactionAdd,
//     ReactionRemove,
//     ReactionRemoveAll,
//     PresenceReplace,
//     Ready,
//     Resume,
//     ShardStageUpdate,
//     StageInstanceCreate,
//     StageInstanceDelete,
//     StageInstanceUpdate,
//     ThreadCreate,
//     ThreadDelete,
//     ThreadListSync,
//     ThreadMemberUpdate,
//     ThreadMembersUpdate,
//     ThreadUpdate,
//     Unknown,
//     UserUpdate,
//     VoiceServerUpdate,
//     WebhooksUpdate,
// }
