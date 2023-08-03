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
        // poise::Event::GuildScheduledEventDelete { event } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(event)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildScheduledEventUserAdd { subscribed } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(subscribed)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildScheduledEventUserRemove { unsubscribed } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(unsubscribed)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildStickersUpdate {
        //     guild_id,
        //     current_state,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, current_state))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildUnavailable { guild_id } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&guild_id)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildUpdate {
        //     old_data_global_if_available,
        //     new_but_incomplete,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_data_global_if_available, new_but_incomplete))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildUpdate {
        //     new_but_incomplete,
        //     old_data_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(new_but_incomplete, old_data_if_available))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::IntegrationCreate { integration } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(integration)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::IntegrationUpdate { integration } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(integration)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::IntegrationDelete {
        //     integration_id,
        //     guild_id,
        //     application_id,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(integration_id, guild_id, application_id))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::InteractionCreate { interaction } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(interaction)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::InviteCreate { data } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(data_global)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::InviteDelete { data } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(data_global)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // // message => Message { new_message: serenity::Message },
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
        //#[cfg(feature = "cache")]
        // poise::Event::MessageUpdate {
        //     old_if_available,
        //     new,
        //     event,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_if_available, new, event))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::MessageUpdate {
        //     old_if_available,
        //     new,
        //     event,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_if_available, new, event))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ReactionAdd { add_reaction } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(add_reaction)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ReactionRemove { removed_reaction } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(removed_reaction)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ReactionRemoveAll {
        //     channel_id,
        //     removed_from_message_id,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(channel_id, removed_from_message_id))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::PresenceReplace { new_presences } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(new_presences)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // // poise::Event::PresenceUpdate { new_data_global } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(new_data_global)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::Ready { data_about_bot } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(data_about_bot)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::Resume { event } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(event)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // // poise::Event::ShardStageUpdate { ShardStageUpdateEvent{  } } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(update)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::StageInstanceCreate { stage_instance } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(stage_instance)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::StageInstanceDelete { stage_instance } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(stage_instance)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::StageInstanceUpdate { stage_instance } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(stage_instance)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadCreate { thread } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadDelete { thread } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadListSync { thread_list_sync } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread_list_sync)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadMemberUpdate { thread_member } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread_member)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadMembersUpdate {
        //     thread_members_update,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread_members_update)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ThreadUpdate { thread } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(thread)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // // poise::Event::TypingStart { event } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(event)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::Unknown { name, raw } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(name, raw))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::UserUpdate {
        //     old_data_global,
        //     new,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_data_global, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::UserUpdate { old_data, new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_data, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::VoiceServerUpdate { update } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(update)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // VoiceStateUpdate { old, new } => event_log.write_obj(&(old, new)),
        // #[cfg(not(feature = "cache"))]
        // VoiceStateUpdate { old, new } => event_log.write_obj(&(old, new)),
        // WebhookUpdate {
        //     guild_id,
        //     belongs_to_channel_id,
        // } => event_log.write_obj(&(guild_id, belongs_to_channel_id)),
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
