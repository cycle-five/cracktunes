use crate::{handlers::serenity::voice_state_diff_str, Data, Error};
use colored::Colorize;
//use poise::serenity_prelude::Event::*;
use poise::serenity_prelude::FullEvent::*;
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
    event: &poise::serenity_prelude::FullEvent,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = data_global.event_log.clone();
    let event_name = event.snake_case_name();
    match event {
        PresenceUpdate { new_data, ctx } => {
            let _ = new_data;
            tracing::trace!("Got a presence update: {:?}", new_data);
            event_log.write_log_obj(event.snake_case_name(), new_data)
        }
        GuildMemberAddition { new_member, ctx } => {
            tracing::info!("Got a new member: {:?}", new_member);
            event_log.write_log_obj(event.snake_case_name(), new_member)
        }
        VoiceStateUpdate { old, new, ctx } => {
            tracing::debug!(
                "VoiceStateUpdate: {}",
                voice_state_diff_str(old.clone(), new).bright_yellow()
            );
            event_log.write_log_obj(event.snake_case_name(), &(old, new))
        }
        Message { new_message, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), &new_message)
        }
        TypingStart { event, ctx } => {
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
                .to_guild_cached(&ctx.cache)
                .map(|guild| guild.name.clone())
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
        CommandPermissionsUpdate { permission, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), permission)
        }
        AutoModActionExecution { execution, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), execution)
        }
        AutoModRuleCreate { rule, ctx: _ } => {
            event_log.write_log_obj(event.snake_case_name(), rule)
        }
        AutoModRuleUpdate { rule, ctx: _ } => {
            event_log.write_log_obj(event.snake_case_name(), rule)
        }
        AutoModRuleDelete { rule, ctx: _ } => {
            event_log.write_log_obj(event.snake_case_name(), rule)
        }
        CategoryCreate { category, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), category)
        }
        CategoryDelete { category, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), category)
        }
        ChannelDelete {
            channel,
            ctx,
            messages,
        } => event_log.write_log_obj(event.snake_case_name(), &(channel, messages)),
        ChannelPinsUpdate { pin, ctx } => event_log.write_log_obj(event.snake_case_name(), pin),
        #[cfg(feature = "cache")]
        ChannelUpdate { old, new, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), &(old, new))
        }
        #[cfg(not(feature = "cache"))]
        ChannelUpdate { old, new, ctx } => {
            event_log.write_log_obj(event.snake_case_name(), &(old, new))
        }
        GuildBanAddition {
            guild_id,
            banned_user,
            ctx,
        } => event_log.write_log_obj(event.snake_case_name(), &(guild_id, banned_user)),
        GuildBanRemoval {
            guild_id,
            unbanned_user,
            ctx,
        } => event_log.write_log_obj(event.snake_case_name(), &(guild_id, unbanned_user)),
        #[cfg(feature = "cache")]
        GuildCreate { guild, is_new, ctx } => event_log.write_log_obj(
            event.snake_case_name(),
            &serde_json::to_vec(&(guild, is_new)).unwrap(),
        ),
        #[cfg(not(feature = "cache"))]
        GuildCreate { guild, is_new, ctx } => event_log.write_log_obj(
            event.snake_case_name(),
            &serde_json::to_vec(&(guild, is_new)).unwrap(),
        ),
        #[cfg(feature = "cache")]
        GuildDelete {
            incomplete,
            full,
            ctx,
        } => event_log.write_log_obj(
            event.snake_case_name(),
            &serde_json::to_vec(&(incomplete, full)).unwrap(),
        ),
        #[cfg(not(feature = "cache"))]
        GuildDelete {
            incomplete,
            full,
            ctx,
        } => event_log.write_obj(&(incomplete, full)),
        GuildEmojisUpdate {
            guild_id,
            current_state,
            ctx,
        } => event_log.write_obj(&(guild_id, current_state)),
        GuildIntegrationsUpdate { guild_id, ctx } => event_log.write_obj(&guild_id),
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
            ctx,
        } => event_log.write_obj(&(guild_id, user, member_data_global_if_available)),
        #[cfg(not(feature = "cache"))]
        GuildMemberRemoval {
            guild_id,
            user,
            member_data_if_available,
            ctx,
        } => event_log.write_log_obj(event_name, &(guild_id, user, member_data_if_available)),
        #[cfg(feature = "cache")]
        GuildMemberUpdate {
            old_if_available,
            new,
            ctx,
        } => event_log.write_log_obj(event_name, &(old_if_available, new)),
        #[cfg(not(feature = "cache"))]
        GuildMemberUpdate {
            old_if_available,
            new,
            ctx,
            event,
        } => event_log.write_log_obj(event_name, &(old_if_available, new)),
        GuildMembersChunk { chunk, ctx } => event_log.write_log_obj(event_name, chunk),
        GuildRoleCreate { new, ctx } => event_log.write_log_obj(event_name, new),
        #[cfg(feature = "cache")]
        GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_global_if_available,
            ctx,
        } => event_log.write_log_obj(
            event_name,
            &(
                guild_id,
                removed_role_id,
                removed_role_data_global_if_available,
            ),
        ),
        #[cfg(not(feature = "cache"))]
        GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_if_available,
            ctx,
        } => event_log.write_log_obj(
            event_name,
            &(guild_id, removed_role_id, removed_role_data_if_available),
        ),
        #[cfg(feature = "cache")]
        GuildRoleUpdate {
            old_data_global_if_available,
            new,
            ctx,
        } => event_log.write_log_obj(event_name, &(old_data_global_if_available, new)),
        #[cfg(not(feature = "cache"))]
        GuildRoleUpdate {
            new,
            old_data_if_available,
            ctx,
        } => event_log.write_log_obj(event_name, &(new, old_data_if_available)),
        GuildScheduledEventCreate { event, ctx } => event_log.write_log_obj(event_name, event),
        GuildScheduledEventUpdate { event, ctx } => event_log.write_log_obj(event_name, event),
        GuildScheduledEventDelete { event, ctx } => event_log.write_log_obj(event_name, event),
        GuildScheduledEventUserAdd { subscribed, ctx } => {
            event_log.write_log_obj(event_name, subscribed)
        }
        GuildScheduledEventUserRemove { unsubscribed, ctx } => {
            event_log.write_log_obj(event_name, unsubscribed)
        }
        GuildStickersUpdate {
            guild_id,
            current_state,
            ctx,
        } => event_log.write_log_obj(event_name, &(guild_id, current_state)),
        // GuildUnavailable { guild_id } => {
        //     event_log.write_log_obj(event_name, &guild_id)
        // },
        #[cfg(feature = "cache")]
        GuildUpdate {
            old_data_global_if_available,
            new_but_incomplete,
            ctx,
        } => event_log.write_log_obj(
            event_name,
            &(old_data_global_if_available, new_but_incomplete),
        ),
        #[cfg(not(feature = "cache"))]
        GuildUpdate {
            old_data_if_available,
            ctx,
            new_data,
        } => event_log.write_log_obj(event_name, &(old_data_if_available, new_data)),
        IntegrationCreate { integration, ctx } => event_log.write_log_obj(event_name, integration),
        IntegrationUpdate { integration, ctx } => event_log.write_log_obj(event_name, integration),
        IntegrationDelete {
            integration_id,
            guild_id,
            application_id,
            ctx,
        } => event_log.write_log_obj(event_name, &(integration_id, guild_id, application_id)),
        InteractionCreate { interaction, ctx } => event_log.write_log_obj(event_name, interaction),
        InviteCreate { data, ctx } => event_log.write_log_obj(event_name, data),
        InviteDelete { data, ctx } => event_log.write_log_obj(event_name, data),
        MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
            ctx,
        } => event_log.write_obj(&(channel_id, deleted_message_id, guild_id)),
        MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids,
            guild_id,
            ctx,
        } => event_log.write_obj(&(channel_id, multiple_deleted_messages_ids, guild_id)),
        #[cfg(feature = "cache")]
        MessageUpdate {
            old_if_available,
            new,
            event,
            ctx,
        } => event_log.write_obj(event_name, &(old_if_available, new, event)),
        #[cfg(not(feature = "cache"))]
        MessageUpdate {
            old_if_available,
            new,
            event,
            ctx,
        } => event_log.write_log_obj(event_name, &(old_if_available, new, event)),
        ReactionAdd { add_reaction, ctx } => event_log.write_log_obj(event_name, add_reaction),
        ReactionRemove {
            removed_reaction,
            ctx,
        } => event_log.write_log_obj(event_name, removed_reaction),
        ReactionRemoveAll {
            channel_id,
            removed_from_message_id,
            ctx,
        } => event_log.write_log_obj(event_name, &(channel_id, removed_from_message_id)),
        PresenceReplace { ctx, presences } => event_log.write_log_obj(event_name, presences),
        // // poise::Event::PresenceUpdate { new_data_global } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(new_data_global)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        Ready {
            data_about_bot,
            ctx,
        } => event_log.write_log_obj(event_name, data_about_bot),
        Resume { event, ctx } => event_log.write_log_obj(event_name, event),
        // // poise::Event::ShardStageUpdate { ShardStageUpdateEvent{  } } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(update)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        StageInstanceCreate {
            stage_instance,
            ctx,
        } => event_log.write_log_obj(event_name, stage_instance),
        StageInstanceDelete {
            stage_instance,
            ctx,
        } => event_log.write_log_obj(event_name, stage_instance),
        StageInstanceUpdate {
            stage_instance,
            ctx,
        } => event_log.write_log_obj(event_name, stage_instance),
        ThreadCreate { thread, ctx } => event_log.write_log_obj(event_name, thread),
        ThreadDelete { thread, ctx } => event_log.write_log_obj(event_name, thread),
        ThreadListSync {
            thread_list_sync,
            ctx,
        } => event_log.write_log_obj(event_name, thread_list_sync),
        ThreadMemberUpdate { thread_member, ctx } => {
            event_log.write_log_obj(event_name, thread_member)
        }
        ThreadMembersUpdate {
            thread_members_update,
            ctx,
        } => event_log.write_log_obj(event_name, thread_members_update),
        ThreadUpdate { thread, ctx } => event_log.write_log_obj(event_name, thread),
        // // poise::Event::TypingStart { event } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(event)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // UnknownEvent {
        //     name,
        //     raw,
        //     kind,
        //     value,
        // } => event_log.write_log_obj(event_name, &(name, raw)),
        #[cfg(feature = "cache")]
        poise::Event::UserUpdate {
            old_data_global,
            new,
        } => event_log.write_log_obj(&(old_data_global, new)),
        #[cfg(not(feature = "cache"))]
        UserUpdate { old_data, new, ctx } => event_log.write_log_obj(event_name, &(old_data, new)),
        VoiceServerUpdate { ctx, event } => event_log.write_log_obj(event_name, event),
        #[cfg(feature = "cache")]
        VoiceStateUpdate { old, new, ctx } => event_log.write_obj(&(old, new)),
        WebhookUpdate {
            guild_id,
            belongs_to_channel_id,
            ctx,
        } => event_log.write_obj(&(guild_id, belongs_to_channel_id)),
        _ => {
            tracing::info!("{}", event.snake_case_name().bright_green());
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
