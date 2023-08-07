use crate::{handlers::serenity::voice_state_diff_str, Data, Error};
use colored::Colorize;
use poise::Event::*;
use serde::{ser::SerializeStruct, Serialize};
use serenity::client::Context as SerenityContext;

#[derive(Debug)]
pub struct LogEntry<T: Serialize> {
    pub name: String,
    pub event: T,
}

impl<T: Serialize> Serialize for LogEntry<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("LogEntry", 2)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("event", &self.event)?;
        state.end()
    }
}

pub async fn handle_event(
    ctx: &SerenityContext,
    event: &poise::Event<'_>,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = data_global.event_log.clone();
    let event_name = event.name();
    match event {
        PresenceUpdate { new_data } => {
            let _ = new_data;
            tracing::trace!("Got a presence update: {:?}", new_data);
            event_log.write_log_obj(event.name(), new_data)
        }
        GuildMemberAddition { new_member } => {
            tracing::info!("Got a new member: {:?}", new_member);
            event_log.write_log_obj(event.name(), new_member)
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
        #[cfg(feature = "cache")]
        ChannelUpdate { old, new } => event_log.write_log_obj(event.name(), &(old, new)),
        #[cfg(not(feature = "cache"))]
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
        // // poise::Event::GuildMemberAddition { new_member } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(&new_member)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        #[cfg(feature = "cache")]
        poise::Event::GuildMemberRemoval {
            guild_id,
            user,
            member_data_global_if_available,
        } => event_log.write_obj(&(guild_id, user, member_data_global_if_available)),
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildMemberRemoval {
            guild_id,
            user,
            member_data_if_available,
        } => event_log.write_log_obj(event_name, &(guild_id, user, member_data_if_available)),
        #[cfg(feature = "cache")]
        poise::Event::GuildMemberUpdate {
            old_if_available,
            new,
        } => event_log.write_log_obj(event_name, &(old_if_available, new)),
        #[cfg(not(feature = "cache"))]
        poise::Event::GuildMemberUpdate {
            old_if_available,
            new,
        } => event_log.write_log_obj(event_name, &(old_if_available, new)),
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
        // // poise::Event::PresenceUpdate { new_data_global } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(new_data_global)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        Ready { data_about_bot } => event_log.write_log_obj(event_name, data_about_bot),
        Resume { event } => event_log.write_log_obj(event_name, event),
        // // poise::Event::ShardStageUpdate { ShardStageUpdateEvent{  } } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(update)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
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
        // // poise::Event::TypingStart { event } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(event)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
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
