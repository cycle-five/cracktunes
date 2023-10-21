use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{
    errors::CrackedError, guild::settings::GuildSettings, handlers::serenity::voice_state_diff_str,
    utils::create_log_embed, Data, Error,
};
use colored::Colorize;
use poise::{
    serenity_prelude::{ChannelId, GuildId, Member, Presence},
    Event::*,
};
use serde::{ser::SerializeStruct, Serialize};
use serenity::client::Context as SerenityContext;

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
    let guild_settings_map = data.guild_settings_map.lock().unwrap();
    guild_settings_map
        .get(&guild_id.into())
        .map(|x| x.get_log_channel(channel_name))
        .unwrap()
}

pub struct HandleEventData<'a> {
    guild_settings: Arc<Mutex<HashMap<GuildId, GuildSettings>>>,
    event_log: crate::EventLog,
    event: poise::Event<'a>,
    #[allow(dead_code)]
    ctx: &'a SerenityContext,
}

impl HandleEventData<'_> {
    pub async fn get_channel_id(self, guild_id: &GuildId) -> Result<ChannelId, CrackedError> {
        match self
            .guild_settings
            .lock()
            .unwrap()
            .get(guild_id)
            .map(|x| x.get_log_channel_type(&self.event))
            .unwrap_or(None)
        {
            Some(channel_id) => Ok(channel_id),
            None => Err(CrackedError::LogChannelWarning(
                self.event.name(),
                *guild_id,
            )),
        }
    }

    pub fn internal_log<T: Serialize>(&self, log_data: &T) -> Result<(), CrackedError> {
        self.event_log
            .write_log_obj(self.event.name(), &log_data)
            .map_err(CrackedError::Poise)
    }
}

// macro_rules! handle_event {
//     ($handle_event_data:expr, $guild_id:expr, $log_data:expr, $block:expr) => {{
//         let _ = $handle_event_data
//             .event_log
//             .write_log_obj($handle_event_data.event.name(), &$log_data);
//         if let Some(channel_id) = $handle_event_data
//             .guild_settings
//             .lock()
//             .unwrap()
//             .get(&$guild_id)
//             .map(|x| x.get_log_channel_type(&$handle_event_data.event))
//         {
//             let channel_id_func = $block;
//             channel_id_func(channel_id).await?;
//         } else {
//             tracing::warn!(
//                 "No log channel set for {} guild {}",
//                 $handle_event_data.event.name(),
//                 $guild_id
//             );
//         };
//         Ok(())
//     }};
// }

pub async fn some_chan_func(
    ctx: &SerenityContext,
    new_member: &Member,
    channel_id: ChannelId,
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
    tracing::warn!("Avatar URL: {}", avatar_url);
    create_log_embed(&channel_id, &ctx.http, &title, &description, &avatar_url).await
}

pub async fn log_presence_update<'a, 'b>(
    ctx: &'a SerenityContext,
    new_data: &'b Presence,
    channel_id: ChannelId,
) -> Result<(), Error> {
    let title = format!(
        "Presence Update: {}",
        new_data.user.name.clone().unwrap_or_default()
    );
    let description = format!(
        "User: {}\nID: {}\nPresence Falgs: {:?}",
        new_data.user.name.clone().unwrap_or_default(),
        new_data.user.id,
        new_data.user.public_flags.unwrap_or_default(),
    );
    let avatar_url = format!(
        "https://cdn.discordapp.com/{user_id}/{user_avatar}.png",
        user_id = new_data.user.id,
        user_avatar = new_data.user.avatar.clone().unwrap_or_default(),
    );
    create_log_embed(&channel_id, &ctx.http, &title, &description, &avatar_url)
        .await
        .map(|_| ())
}

pub async fn handle_event(
    ctx: &SerenityContext,
    event: &poise::Event<'_>,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = data_global.event_log.clone();
    let event_name = event.name();
    let handle_event_data = HandleEventData {
        guild_settings: Arc::clone(&data_global.guild_settings_map),
        event_log: event_log.clone(),
        event: event.clone(),
        ctx,
    };
    match event {
        PresenceUpdate { new_data } => {
            handle_event_data.internal_log::<Presence>(new_data)?;
            let channel_id = handle_event_data
                .get_channel_id(&new_data.guild_id.unwrap())
                .await?;
            log_presence_update(ctx, new_data, channel_id).await
        }
        GuildMemberAddition { new_member } => {
            tracing::info!("Got a new member: {:?}", new_member);
            let log_data = new_member;
            let guild_settings = data_global.guild_settings_map.lock().unwrap().clone();
            match guild_settings
                .get(&new_member.guild_id)
                .unwrap()
                .get_log_channel_type(event)
            {
                Some(channel_id) => {
                    some_chan_func(ctx, new_member, channel_id).await?;
                }
                None => {
                    tracing::warn!(
                        "No join/leave log channel set for guild {}",
                        new_member.guild_id
                    );
                }
            };
            event_log.write_log_obj(event.name(), log_data)
        }
        poise::Event::GuildMemberRemoval {
            guild_id,
            user,
            member_data_if_available,
        } => {
            let log_data = (guild_id, user, member_data_if_available);
            let guild_settings = data_global.guild_settings_map.lock().unwrap().clone();
            tracing::info!("Member left: {:?}", member_data_if_available);
            match guild_settings
                .get(guild_id)
                .unwrap()
                .get_log_channel_type(event)
            {
                Some(channel_id) => {
                    let title = format!("Member Left: {}", user.name);
                    let description = format!(
                        "User: {}\nID: {}\nAccount Created: {}\nJoined: {:?}",
                        user.name,
                        user.id,
                        user.created_at(),
                        member_data_if_available.clone().and_then(|m| m.joined_at)
                    );
                    let avatar_url = user.avatar_url().unwrap_or_default();
                    create_log_embed(&channel_id, &ctx.http, &title, &description, &avatar_url)
                        .await?;
                }
                None => {
                    tracing::warn!("No join/leave log channel set for guild {}", guild_id);
                }
            }
            event_log.write_log_obj(event_name, &log_data)
        }
        VoiceStateUpdate { old, new } => {
            tracing::debug!(
                "VoiceStateUpdate: {}",
                voice_state_diff_str(old.clone(), new).bright_yellow()
            );
            event_log.write_log_obj(event.name(), &(old, new))
        }
        Message { new_message } => event_log.write_log_obj(event.name(), &new_message),
        TypingStart { event } => {
            let _ = event_log.write_log_obj(event_name, &event);
            let cache_http = ctx.http.clone();
            let channel = event
                .channel_id
                .to_channel_cached(ctx.cache.clone())
                .unwrap();
            let user = event.user_id.to_user(cache_http.clone()).await.unwrap();
            let channel_name = channel
                .guild()
                .map(|guild| guild.name)
                .unwrap_or("DM".to_string());
            let guild = event
                .guild_id
                .unwrap_or_default()
                .to_guild_cached(ctx.cache.clone())
                .map(|guild| guild.name)
                .unwrap_or("DM".to_string());

            tracing::info!(
                "{}{} / {} / {} / {}",
                "TypingStart: ".bright_green(),
                user.name.bright_yellow(),
                user.id.to_string().bright_yellow(),
                channel_name.bright_yellow(),
                guild.bright_yellow(),
            );
            Ok(())
        }
        ApplicationCommandPermissionsUpdate { permission } => {
            event_log.write_log_obj(event.name(), permission)
        }
        AutoModerationActionExecution { execution } => {
            event_log.write_log_obj(event.name(), execution)
        }
        AutoModerationRuleCreate { rule } => event_log.write_log_obj(event.name(), rule),
        AutoModerationRuleUpdate { rule } => event_log.write_log_obj(event.name(), rule),
        AutoModerationRuleDelete { rule } => event_log.write_log_obj(event.name(), rule),
        CategoryCreate { category } => event_log.write_log_obj(event.name(), category),
        CategoryDelete { category } => event_log.write_log_obj(event.name(), category),
        ChannelDelete { channel } => event_log.write_log_obj(event.name(), channel),
        ChannelPinsUpdate { pin } => event_log.write_log_obj(event.name(), pin),
        ChannelUpdate { old, new } => event_log.write_log_obj(event.name(), &(old, new)),
        poise::Event::GuildBanAddition {
            guild_id,
            banned_user,
        } => event_log.write_log_obj(event.name(), &(guild_id, banned_user)),
        poise::Event::GuildBanRemoval {
            guild_id,
            unbanned_user,
        } => event_log.write_log_obj(event.name(), &(guild_id, unbanned_user)),
        #[cfg(feature = "cache")]
        GuildCreate { guild, is_new } => {
            event_log.write_log_obj(event.name(), &serde_json::to_vec(&(guild, is_new)).unwrap())
        }
        #[cfg(not(feature = "cache"))]
        GuildCreate { guild, is_new } => {
            event_log.write_log_obj(event.name(), &serde_json::to_vec(&(guild, is_new)).unwrap())
        }
        #[cfg(feature = "cache")]
        GuildDelete { incomplete, full } => event_log.write_log_obj(
            event.name(),
            &serde_json::to_vec(&(incomplete, full)).unwrap(),
        ),
        #[cfg(not(feature = "cache"))]
        GuildDelete { incomplete, full } => event_log.write_obj(&(incomplete, full)),
        GuildEmojisUpdate {
            guild_id,
            current_state,
        } => event_log.write_obj(&(guild_id, current_state)),
        GuildIntegrationsUpdate { guild_id } => event_log.write_obj(&guild_id),
        #[cfg(feature = "cache")]
        poise::Event::GuildMemberUpdate {
            old_if_available,
            new,
        } => event_log.write_log_obj(event_name, &(old_if_available, new)),
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
        Ready { data_about_bot } => event_log.write_log_obj(event_name, data_about_bot),
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
        _ => {
            tracing::info!("{}", event.name().bright_green());
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
