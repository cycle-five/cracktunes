use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    errors::CrackedError,
    guild::settings::GuildSettings, // handlers::serenity::voice_state_diff_str,
    utils::create_log_embed,
    Data,
    Error,
};
use colored::Colorize;
use poise::{
    serenity_prelude::{
        ChannelId, ClientStatus, Event, FullEvent, GuildId, GuildMemberAddEvent,
        GuildMemberRemoveEvent, Member, Presence, PresenceUpdateEvent, TypingStartEvent,
        VoiceStateUpdateEvent,
    },
    FrameworkContext,
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

pub async fn get_channel_id(
    guild_settings_map: &Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
    guild_id: &GuildId,
    // event: &serenity::all::Event,
    event: &FullEvent,
) -> Result<ChannelId, CrackedError> {
    // let initial_values: Vec<u64> = vec![1165246445654388746];
    // let hashset: HashSet<_> = initial_values.into_iter().collect();

    let x = {
        let guild_settings_map = guild_settings_map.lock().unwrap().clone();

        let guild_settings = guild_settings_map
            .get(guild_id)
            .map(Ok)
            .unwrap_or_else(|| {
                tracing::error!("Failed to get guild_settings for guild_id {}", guild_id);
                Err(CrackedError::LogChannelWarning(
                    event.snake_case_name(),
                    *guild_id,
                ))
            })?
            .clone();
        match guild_settings.get_log_channel_type_fe(event) {
            Some(channel_id) => {
                if guild_settings.ignored_channels.contains(&channel_id.get()) {
                    return Err(CrackedError::LogChannelWarning(
                        event.snake_case_name(),
                        *guild_id,
                    ));
                }
                Ok(channel_id)
            }
            None => Err(CrackedError::LogChannelWarning(
                event.snake_case_name(),
                *guild_id,
            )),
        }
    };
    x
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
        user_avatar = new_data
            .user
            .avatar
            .map(|x| x.to_string())
            .unwrap_or_default(),
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
    let description = format!("FIXCME: {old:?} / {new:?}"); // voice_state_diff_str(old, new);

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
        .get_channel(channel_id)
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

macro_rules! log_event {
    // #[cfg(feature="no_log")]
    ($log_func:expr, $guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $http:expr, $event_log:expr, $event_name:expr) => {{
        let channel_id = get_channel_id($guild_settings, $guild_id, $event).await?;
        $log_func(channel_id, $http, $log_data).await?;
        $event_log.write_log_obj($event_name, $log_data)
    }};
}

/// Macro to concisely generate the handle_event match statement
// macro_rules! handle_macro {
//     ($guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $http:expr, $event_log:expr, $event_name:expr, $(
//         $fn_name:ident => $variant_name:ident { $( $arg_name:ident: $arg_type:ty ),* },
//     )*) => {
//         $($variant_name { $( $arg_name, )* }  => {
//             let log_data = ($( $arg_name, )*);
//             log_event!(
//                 $fn_name,
//                 $guild_settings,
//                 $event,
//                 $log_data,
//                 $guild_id,
//                 $http,
//                 $event_log,
//                 $event_name
//             )
//         })*
//     };
// }

pub async fn handle_event(
    event_in: &FullEvent,
    //ctx: &SerenityContext,
    // event_in: &poise::serenity_prelude::Event,
    framework: FrameworkContext<'_, Data, Error>,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = Arc::new(&data_global.event_log);
    let event_name = event_in.snake_case_name();
    let guild_settings = &data_global.guild_settings_map;
    //     match event_in {
    //         handle_macro! (
    //             guild_settings, event_in, &log_data, guild_id, &ctx.http, event_log, event_name,
    //             guild_member_addition => GuildMemberAddition { new_member: serenity::Member },
    //             guild_member_removal => GuildMemberRemoval { guild_id: serenity::GuildId, user: serenity::User, member_data_if_available: Option<serenity::Member> },
    //         );
    //         _ => todo!()
    //     }
    // }
    // event_log.write_log_obj(event_name, event_in)?;

    match event_in {
        FullEvent::PresenceUpdate { ctx: _, new_data } => {
            #[cfg(feature = "log_all")]
            {
                log_event!(
                    log_presence_update,
                    guild_settings,
                    event_in,
                    presence,
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
        FullEvent::GuildMemberAddition {
            ctx, new_member, ..
        } => {
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
        FullEvent::GuildMemberRemoval {
            ctx,
            member_data_if_available,
            guild_id,
            user,
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
        FullEvent::VoiceStateUpdate { ctx, old, new } => {
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
        FullEvent::Message { ctx, new_message } => {
            if new_message.author.id == ctx.cache.current_user().id {
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
        FullEvent::TypingStart { ctx, event } => {
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
        FullEvent::CommandPermissionsUpdate { ctx, permission } => {
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
        FullEvent::AutoModActionExecution { ctx, execution } => {
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
        FullEvent::AutoModRuleCreate { ctx, rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::AutoModRuleUpdate { ctx, rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::AutoModRuleDelete { ctx, rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            rule,
            &rule.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::CategoryCreate { ctx, category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            category,
            &category.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::CategoryDelete { ctx, category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            category,
            &category.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::ChannelDelete {
            ctx,
            channel,
            messages,
        } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            &(channel, messages),
            &channel.guild_id,
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::ChannelPinsUpdate { ctx, pin } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            pin,
            &pin.guild_id.unwrap_or_default(),
            &ctx.http,
            event_log,
            event_name
        ),
        FullEvent::ChannelUpdate { ctx, old, new } => {
            let guild_id = new.clone().guild().map_or(GuildId::new(0), |x| x.guild_id);
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
        FullEvent::GuildBanAddition {
            guild_id,
            banned_user,
            ctx,
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
        FullEvent::GuildBanRemoval {
            guild_id,
            unbanned_user,
            ctx,
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
        FullEvent::GuildCreate { ctx, guild } => {
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
        FullEvent::GuildCreate { ctx, guild, is_new } => {
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
        FullEvent::GuildDelete { ctx, incomplete } => {
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
        FullEvent::GuildDelete {
            ctx,
            incomplete,
            full,
        } => {
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
        FullEvent::GuildEmojisUpdate {
            guild_id,
            current_state,
            ctx,
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
        FullEvent::GuildIntegrationsUpdate { ctx, guild_id } => {
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
        FullEvent::GuildMemberUpdate {
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
        FullEvent::GuildMemberUpdate {
            ctx,
            old_if_available,
            new,
            event,
        } => {
            let guild_settings = data_global.guild_settings_map.lock().unwrap().clone();
            let new = new.clone().unwrap();
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
        FullEvent::GuildMembersChunk { chunk, ctx } => event_log.write_log_obj(event_name, chunk),
        FullEvent::GuildRoleCreate { new, ctx } => event_log.write_log_obj(event_name, new),
        #[cfg(feature = "cache")]
        GuildRoleDelete {
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
        FullEvent::GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_if_available,
            ctx,
        } => event_log.write_log_obj(
            event_name,
            &(guild_id, removed_role_id, removed_role_data_if_available),
        ),
        #[cfg(feature = "cache")]
        FullEvent::GuildRoleUpdate {
            old_data_global_if_available,
            new,
        } => event_log.write_log_obj(event_name, &(old_data_global_if_available, new)),
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildRoleUpdate {
            new,
            old_data_if_available,
            ctx: _,
        } => event_log.write_log_obj(event_name, &(new, old_data_if_available)),
        FullEvent::GuildScheduledEventCreate { event, ctx: _ } => {
            event_log.write_log_obj(event_name, event)
        }
        FullEvent::GuildScheduledEventUpdate { event, ctx: _ } => {
            event_log.write_log_obj(event_name, event)
        }
        FullEvent::GuildScheduledEventDelete { event, ctx: _ } => {
            event_log.write_log_obj(event_name, event)
        }
        FullEvent::GuildScheduledEventUserAdd { subscribed, ctx: _ } => {
            event_log.write_log_obj(event_name, subscribed)
        }
        FullEvent::GuildScheduledEventUserRemove {
            unsubscribed,
            ctx: _,
        } => event_log.write_log_obj(event_name, unsubscribed),
        FullEvent::GuildStickersUpdate {
            ctx: _,
            guild_id,
            current_state,
        } => event_log.write_log_obj(event_name, &(guild_id, current_state)),
        FullEvent::GuildAuditLogEntryCreate {
            ctx: _,
            entry,
            guild_id,
        } => event_log.write_log_obj(event_name, &(entry, guild_id)),
        #[cfg(feature = "cache")]
        FullEvent::GuildUpdate {
            old_data_global_if_available,
            new_but_incomplete,
        } => event_log.write_log_obj(
            event_name,
            &(old_data_global_if_available, new_but_incomplete),
        ),
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildUpdate {
            old_data_if_available,
            ctx: _,
            new_data,
        } => event_log.write_log_obj(event_name, &(old_data_if_available, new_data)),
        FullEvent::IntegrationCreate {
            integration,
            ctx: _,
        } => event_log.write_log_obj(event_name, integration),
        FullEvent::IntegrationUpdate { integration, ctx } => {
            event_log.write_log_obj(event_name, integration)
        }
        FullEvent::IntegrationDelete {
            integration_id,
            guild_id,
            application_id,
            ctx,
        } => event_log.write_log_obj(event_name, &(integration_id, guild_id, application_id)),
        FullEvent::InteractionCreate { interaction, ctx } => {
            event_log.write_log_obj(event_name, interaction)
        }
        FullEvent::InviteCreate { data, ctx } => event_log.write_log_obj(event_name, data),
        FullEvent::InviteDelete { data, ctx } => event_log.write_log_obj(event_name, data),
        FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
            ctx,
        } => event_log.write_obj(&(channel_id, deleted_message_id, guild_id)),
        FullEvent::MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids,
            guild_id,
            ctx,
        } => event_log.write_obj(&(channel_id, multiple_deleted_messages_ids, guild_id)),
        #[cfg(feature = "cache")]
        FullEvent::MessageUpdate {
            old_if_available,
            new,
            event,
        } => event_log.write_obj(event_name, &(old_if_available, new, event)),
        #[cfg(not(feature = "cache"))]
        FullEvent::MessageUpdate {
            old_if_available,
            new,
            event,
            ctx,
        } => event_log.write_log_obj(event_name, &(old_if_available, new, event)),
        FullEvent::ReactionAdd {
            add_reaction,
            ctx: _,
        } => event_log.write_log_obj(event_name, add_reaction),
        FullEvent::ReactionRemove {
            removed_reaction,
            ctx: _,
        } => event_log.write_log_obj(event_name, removed_reaction),
        FullEvent::ReactionRemoveAll {
            channel_id,
            removed_from_message_id,
            ctx: _,
        } => event_log.write_log_obj(event_name, &(channel_id, removed_from_message_id)),
        FullEvent::PresenceReplace { ctx, presences } => {
            event_log.write_log_obj(event_name, presences)
        }
        FullEvent::Ready {
            data_about_bot,
            ctx: _,
        } => {
            tracing::info!("{} is connected!", data_about_bot.user.name);
            event_log.write_log_obj(event_name, data_about_bot)
        }
        FullEvent::Resume { ctx, event } => event_log.write_log_obj(event_name, event),
        FullEvent::StageInstanceCreate {
            stage_instance,
            ctx: _,
        } => event_log.write_log_obj(event_name, stage_instance),
        FullEvent::StageInstanceDelete {
            stage_instance,
            ctx: _,
        } => event_log.write_log_obj(event_name, stage_instance),
        FullEvent::StageInstanceUpdate {
            stage_instance,
            ctx: _,
        } => event_log.write_log_obj(event_name, stage_instance),
        FullEvent::ThreadCreate { thread, ctx } => event_log.write_log_obj(event_name, thread),
        FullEvent::ThreadDelete {
            thread,
            ctx: _,
            full_thread_data,
        } => event_log.write_log_obj(event_name, thread),
        FullEvent::ThreadListSync {
            thread_list_sync,
            ctx: _,
        } => event_log.write_log_obj(event_name, thread_list_sync),
        FullEvent::ThreadMemberUpdate {
            thread_member,
            ctx: _,
        } => event_log.write_log_obj(event_name, thread_member),
        FullEvent::ThreadMembersUpdate {
            thread_members_update,
            ctx,
        } => event_log.write_log_obj(event_name, thread_members_update),
        FullEvent::ThreadUpdate { ctx, old, new } => {
            event_log.write_log_obj(event_name, &(old, new))
        }
        // FullEvent::Unknown { name, raw } => event_log.write_log_obj(event_name, &(name, raw)),
        #[cfg(feature = "cache")]
        UserUpdate {
            old_data_global,
            new,
        } => event_log.write_log_obj(&(old_data_global, new)),
        #[cfg(not(feature = "cache"))]
        FullEvent::UserUpdate { old_data, new, ctx } => {
            event_log.write_log_obj(event_name, &(old_data, new))
        }
        FullEvent::VoiceServerUpdate { ctx, event } => event_log.write_log_obj(event_name, event),
        #[cfg(feature = "cache")]
        VoiceStateUpdate { old, new } => event_log.write_obj(&(old, new)),
        FullEvent::WebhookUpdate {
            guild_id,
            belongs_to_channel_id,
            ctx: _,
        } => event_log.write_obj(&(guild_id, belongs_to_channel_id)),
        FullEvent::CacheReady { guilds, ctx } => {
            tracing::info!(
                "{}: {}",
                event_in.snake_case_name().bright_green(),
                guilds.len()
            );
            Ok(())
        }
        _ => {
            tracing::info!("{}", event_in.snake_case_name().bright_green());
            // event_log.write_log_obj(event_name, event_in);
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
