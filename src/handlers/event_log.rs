use std::io::Write;

use crate::{errors::CrackedError, handlers::serenity::voice_state_diff_str, Data, Error};
use colored::Colorize;
use poise::Event::*;
use serenity::client::Context as SerenityContext;

pub async fn handle_event(
    ctx: &SerenityContext,
    event: &poise::Event<'_>,
    data_global: &Data,
) -> Result<(), Error> {
    let event_log = data_global.event_log.clone();
    match event {
        PresenceUpdate { new_data } => {
            let _ = new_data;
            tracing::trace!("Got a presence update: {:?}", new_data);
            Ok(())
        }
        GuildMemberAddition { new_member } => {
            tracing::info!("Got a new member: {:?}", new_member);
            Ok(())
        }
        VoiceStateUpdate { old, new } => {
            tracing::debug!(
                "VoiceStateUpdate: {}",
                voice_state_diff_str(old.clone(), new).bright_yellow()
            );
            let serde_msg = serde_json::to_vec(&(old, new)).unwrap();
            event_log.write(&serde_msg)
        }
        Message { new_message } => {
            let serde_msg = serde_json::to_vec(&new_message).unwrap();
            event_log.write(&serde_msg)
        }
        TypingStart { event } => {
            let serde_msg = serde_json::to_vec(event).unwrap();
            let _ = event_log.write(&serde_msg);
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
            event_log.write(&serde_json::to_vec(&permission).unwrap())
        }
        AutoModerationActionExecution { execution } => {
            event_log.write(&serde_json::to_vec(&execution).unwrap())
        }
        AutoModerationRuleCreate { rule } => event_log.write(&serde_json::to_vec(&rule).unwrap()),
        AutoModerationRuleUpdate { rule } => event_log.write(&serde_json::to_vec(&rule).unwrap()),
        AutoModerationRuleDelete { rule } => event_log.write(&serde_json::to_vec(&rule).unwrap()),
        // poise::Event::CategoryCreate { category } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(category)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::CategoryDelete { category } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(category)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ChannelDelete { channel } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(channel)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::ChannelPinsUpdate { pin } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(pin)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::ChannelUpdate { old, new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::ChannelUpdate { old, new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildBanAddition {
        //     guild_id,
        //     banned_user,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, banned_user))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildBanRemoval {
        //     guild_id,
        //     unbanned_user,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, unbanned_user))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildCreate { guild, is_new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&poise::Event::GuildCreate { guild, is_new })
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildCreate { guild, is_new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild, is_new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // GuildDelete { incomplete, full } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&poise::Event::GuildDelete { incomplete, full })
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildDelete { incomplete, full } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(incomplete, full))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildEmojisUpdate {
        //     guild_id,
        //     current_state,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, current_state))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildIntegrationsUpdate { guild_id } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&guild_id)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // // poise::Event::GuildMemberAddition { new_member } => data_global.event_log
        // //     .lock()
        // //     .await
        // //     .write_obj(&new_member)
        // //     .await
        // //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildMemberRemoval {
        //     guild_id,
        //     user,
        //     member_data_global_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, user, member_data_global_if_available))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildMemberRemoval {
        //     guild_id,
        //     user,
        //     member_data_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, user, member_data_if_available))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildMemberUpdate {
        //     old_if_available,
        //     new,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_if_available, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildMemberUpdate {
        //     old_if_available,
        //     new,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_if_available, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildMembersChunk { chunk } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(chunk)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildRoleCreate { new } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(new)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildRoleDelete {
        //     guild_id,
        //     removed_role_id,
        //     removed_role_data_global_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(
        //         guild_id,
        //         removed_role_id,
        //         removed_role_data_global_if_available,
        //     ))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildRoleDelete {
        //     guild_id,
        //     removed_role_id,
        //     removed_role_data_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(guild_id, removed_role_id, removed_role_data_if_available))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
        // poise::Event::GuildRoleUpdate {
        //     old_data_global_if_available,
        //     new,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old_data_global_if_available, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::GuildRoleUpdate {
        //     new,
        //     old_data_if_available,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(new, old_data_if_available))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildScheduledEventCreate { event } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(event)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::GuildScheduledEventUpdate { event } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(event)
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
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
        // poise::Event::MessageDelete {
        //     channel_id,
        //     deleted_message_id,
        //     guild_id,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(channel_id, deleted_message_id, guild_id))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // poise::Event::MessageDeleteBulk {
        //     channel_id,
        //     multiple_deleted_messages_ids,
        //     guild_id,
        // } => data_global
        //     .event_log
        //     .lock()
        //     .await
        //     .write_obj(&(channel_id, multiple_deleted_messages_ids, guild_id))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(feature = "cache")]
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
        // poise::Event::VoiceStateUpdate { old, new } => data_global.event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        // #[cfg(not(feature = "cache"))]
        // poise::Event::VoiceStateUpdate { old, new } => data_global.event_log
        //     .lock()
        //     .await
        //     .write_obj(&(old, new))
        //     .await
        //     .map_err(|e| CrackedError::SerdeStream(e).into()),
        poise::Event::WebhookUpdate {
            guild_id,
            belongs_to_channel_id,
        } => data_global
            .event_log
            .lock()
            .unwrap()
            .write_all(&serde_json::to_vec(&(guild_id, belongs_to_channel_id)).unwrap())
            .map_err(|e| CrackedError::IO(e).into()),
        _ => {
            tracing::info!("{}", event.name().bright_green());
            Ok(())
        }
    }
}
