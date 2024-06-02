use super::event_log_impl::*;
use crate::{
    errors::CrackedError, guild::settings::GuildSettings, log_event, log_event2,
    utils::send_log_embed_thumb, ArcTRwMap, Data, Error,
};
use colored::Colorize;
use poise::{
    serenity_prelude::{ChannelId, FullEvent, GuildId},
    FrameworkContext,
};
use serde::{ser::SerializeStruct, Serialize};
use serenity::all::User;

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

/// Gets the log channel for a given guild.
pub async fn get_log_channel(
    channel_name: &str,
    guild_id: &GuildId,
    data: &Data,
) -> Option<serenity::model::id::ChannelId> {
    let guild_settings_map = data.guild_settings_map.read().await;
    guild_settings_map
        .get(&guild_id.into())
        .map(|x| x.get_log_channel(channel_name))
        .unwrap()
}

/// Gets the log channel for a given event and guild.
pub async fn get_channel_id(
    guild_settings_map: &ArcTRwMap<GuildId, GuildSettings>,
    guild_id: &GuildId,
    event: &FullEvent,
) -> Result<ChannelId, CrackedError> {
    let guild_settings_map = guild_settings_map.read().await;

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
        },
        None => Err(CrackedError::LogChannelWarning(
            event.snake_case_name(),
            *guild_id,
        )),
    }
}

/// Handles (routes and logs) an event.
/// Currently doesn't handle all events.
#[cfg(not(tarpaulin_include))]
pub async fn handle_event(
    ctx: &serenity::all::Context,
    event_in: &FullEvent,
    _framework: FrameworkContext<'_, Data, Error>,
    data_global: &Data,
) -> Result<(), Error> {
    // let event_log = Arc::new(&data_global.event_log);
    let event_log = std::sync::Arc::new(&data_global.event_log_async);
    let event_name = event_in.snake_case_name();
    let guild_settings = &data_global.guild_settings_map;

    match event_in {
        #[cfg(not(feature = "ignore-presence-log"))]
        FullEvent::PresenceUpdate { new_data } => {
            log_event!(
                log_presence_update,
                guild_settings,
                event_in,
                new_data,
                &new_data.guild_id.unwrap(),
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(feature = "ignore-presence-log")]
        FullEvent::PresenceUpdate { new_data } => {
            let _ = new_data;
            Ok(())
        },
        FullEvent::GuildMemberAddition { new_member } => {
            log_event!(
                log_guild_member_addition,
                guild_settings,
                event_in,
                new_member,
                &new_member.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildMemberRemoval {
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
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::VoiceStateUpdate { old, new } => {
            let log_data = &(old, new);
            let guild_id = new.guild_id.unwrap_or(GuildId::new(1));
            log_event2!(
                log_voice_state_update,
                guild_settings,
                event_in,
                log_data,
                &guild_id,
                ctx,
                event_log,
                event_name
            )
        },
        FullEvent::Message { new_message } => {
            let guild_id = new_message.guild_id.ok_or(CrackedError::NoGuildId)?;
            if new_message.author.id == ctx.http.get_current_user().await?.id {
                let now = chrono::Utc::now();
                let _ = data_global
                    .guild_cache_map
                    .lock()
                    .await
                    .get_mut(&guild_id)
                    .map(|x| x.time_ordered_messages.insert(now, new_message.clone()))
                    .unwrap_or_default();
            }

            if new_message.author.bot {
                return Ok(());
            }
            log_event!(
                log_message,
                guild_settings,
                event_in,
                new_message,
                &new_message.guild_id.unwrap(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::TypingStart { event } => {
            // let cache_http = ctx.http.clone()
            log_event!(
                log_typing_start_noop,
                guild_settings,
                event_in,
                event,
                &event.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::CommandPermissionsUpdate { permission } => {
            log_event!(
                log_command_permissions_update,
                guild_settings,
                event_in,
                permission,
                &permission.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::AutoModActionExecution { execution } => {
            log_event!(
                log_automod_command_execution,
                guild_settings,
                event_in,
                execution,
                &execution.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::AutoModRuleCreate { rule } => log_event!(
            log_automod_rule_create,
            guild_settings,
            event_in,
            &rule,
            &rule.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::AutoModRuleUpdate { rule } => log_event!(
            log_automod_rule_update,
            guild_settings,
            event_in,
            &(event_name.to_string(), rule.clone()),
            &rule.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::AutoModRuleDelete { rule } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            &(event_name, rule),
            &rule.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::CategoryCreate { category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            &(event_name, category),
            &category.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::CategoryDelete { category } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            &(event_name, category),
            &category.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::ChannelDelete { channel, messages } => log_event!(
            log_channel_delete,
            guild_settings,
            event_in,
            &(channel, messages),
            &channel.guild_id,
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::ChannelPinsUpdate { pin } => log_event!(
            log_unimplemented_event,
            guild_settings,
            event_in,
            &(event_name, pin),
            &pin.guild_id.unwrap_or_default(),
            &ctx,
            event_log,
            event_name
        ),
        FullEvent::ChannelUpdate { old, new } => {
            let guild_id = new
                .clone()
                .guild(&ctx.cache)
                .map(|x| x.id)
                .unwrap_or_default();
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &(event_name, old, new),
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildBanAddition {
            guild_id,
            banned_user,
        } => {
            let log_data: (&str, &GuildId, &User) = (event_name, guild_id, banned_user);
            log_event!(
                log_guild_ban_addition::<&(&str, &GuildId, &User)>,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildBanRemoval {
            guild_id,
            unbanned_user,
        } => {
            let log_data = (event_name, guild_id, unbanned_user);
            log_event!(
                log_guild_ban_removal::<&(&str, &GuildId, &User)>,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(feature = "cache")]
        FullEvent::GuildCreate { guild, is_new } => {
            log_event!(
                log_guild_create,
                guild_settings,
                event_in,
                &(guild, is_new),
                &guild.id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildCreate { guild, is_new } => {
            log_event!(
                log_guild_create,
                guild_settings,
                event_in,
                &(guild, is_new, guild_settings),
                &guild.id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(feature = "cache")]
        FullEvent::GuildDelete { incomplete, full } => {
            let log_data = (event_name, incomplete, full);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &incomplete.id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildDelete { incomplete, full } => {
            let log_data = (event_name, incomplete, full);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &incomplete.id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildEmojisUpdate {
            guild_id,
            current_state,
        } => {
            let log_data = (event_name, guild_id, current_state);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildIntegrationsUpdate { guild_id } => {
            let log_data = (event_name, guild_id);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        // FIXME: Do a better diff of the old and new member data.
        // FIXME: Do we rely always on the cache from serenity or implement
        //        our in for any reason? (probably not needed).
        FullEvent::GuildMemberUpdate {
            old_if_available,
            new,
            event,
        } => {
            // let local_event: GuildMemberUpdateEvent = event.clone();
            let guild_name = event
                .guild_id
                .to_guild_cached(&ctx.cache)
                .map(|x| x.name.clone())
                .unwrap_or_default();
            let guild_settings = data_global.guild_settings_map.read().await.clone();
            let new = new.clone().unwrap();
            let maybe_log_channel = guild_settings
                .get(&new.guild_id)
                .map(|x| x.get_join_leave_log_channel())
                .unwrap_or(None);
            let id = new.user.id;
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
                avatar_url.clone_from(&new.user.avatar_url().unwrap_or_default());
            }
            if avatar_url.is_empty() {
                avatar_url.clone_from(&new.user.default_avatar_url());
            }

            let mut notes = "";
            let mut title: String = String::from("");

            if let Some(old) = old_if_available {
                if old.user.avatar.is_none()
                    || new.user.avatar.is_none()
                    || old.user.avatar.unwrap() != new.user.avatar.unwrap()
                {
                    title = format!("Avatar Updated: {}", new.user.name);
                }
            }

            match (maybe_log_channel, old_if_available) {
                (Some(_), Some(old)) => {
                    if old.pending && !new.pending {
                        notes = "Click Verify";
                        title = format!("Member Approved: {}", new.user.name);
                    } else {
                        title = format!("Member Updated: {}", new.user.name);
                    };
                },
                (None, Some(old)) => {
                    if old.pending && !new.pending {
                        notes = "Click Verify";
                        title = format!("Member Approved: {}", new.user.name);
                    } else if !title.is_empty() {
                        title = format!("Member Updated: {}", new.user.name);
                    };
                },
                (Some(_), None) => {
                    title = format!("Member Updated: {}", new.user.name);
                },
                _ => {},
            }
            match maybe_log_channel {
                Some(channel_id) => {
                    send_log_embed_thumb(
                        &guild_name,
                        &channel_id,
                        &ctx,
                        &id.to_string(),
                        &title,
                        &description,
                        &avatar_url,
                    )
                    .await?;
                },
                None => {
                    tracing::debug!("No join/leave log channel set for guild {}", new.guild_id);
                    tracing::debug!(title);
                },
            }
            event_log
                .write_log_obj_note_async(event_name, Some(notes), &(old_if_available, new, event))
                .await
        },
        FullEvent::GuildMembersChunk { chunk } => {
            event_log.write_log_obj_async(event_name, chunk).await
        },
        FullEvent::GuildRoleCreate { new } => {
            log_event!(
                log_guild_role_create,
                guild_settings,
                event_in,
                &new,
                &new.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildRoleDelete {
            guild_id,
            removed_role_id,
            removed_role_data_if_available,
        } => {
            let log_data = (guild_id, removed_role_id, removed_role_data_if_available);
            log_event!(
                log_guild_role_delete,
                guild_settings,
                event_in,
                &log_data,
                guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(feature = "cache")]
        FullEvent::GuildRoleUpdate {
            old_data_if_available,
            new,
        } => {
            let log_data = (old_data_if_available, new);
            log_event!(
                log_guild_role_update,
                guild_settings,
                event_in,
                &log_data,
                &new.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildRoleUpdate {
            new,
            old_data_if_available,
        } => {
            let log_data = (old_data_if_available, new);
            log_event!(
                log_guild_role_update,
                guild_settings,
                event_in,
                &log_data,
                &new.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildScheduledEventCreate { event } => {
            // event_log.write_log_obj_async(event_name, event)
            let log_data = event;
            log_event!(
                log_guild_scheduled_event_create,
                guild_settings,
                event_in,
                &log_data,
                &event.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildScheduledEventUpdate { event } => {
            let log_data = event;
            log_event!(
                log_guild_scheduled_event_update,
                guild_settings,
                event_in,
                &log_data,
                &event.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildScheduledEventDelete { event } => {
            let log_data = event;
            log_event!(
                log_guild_scheduled_event_delete,
                guild_settings,
                event_in,
                &log_data,
                &event.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildScheduledEventUserAdd { subscribed } => {
            let log_data = subscribed;
            log_event!(
                log_guild_scheduled_event_user_add,
                guild_settings,
                event_in,
                &log_data,
                &subscribed.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildScheduledEventUserRemove { unsubscribed } => {
            let log_data = unsubscribed;
            log_event!(
                log_guild_scheduled_event_user_remove,
                guild_settings,
                event_in,
                &log_data,
                &unsubscribed.guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildStickersUpdate {
            guild_id,
            current_state,
        } => {
            let log_data = (guild_id, current_state);
            log_event!(
                log_guild_stickers_update,
                guild_settings,
                event_in,
                &log_data,
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::GuildAuditLogEntryCreate { entry, guild_id } => {
            event_log
                .write_log_obj_async(event_name, &(entry, guild_id))
                .await
        },
        #[cfg(feature = "cache")]
        FullEvent::GuildUpdate {
            old_data_if_available,
            new_data,
        } => {
            let log_data = (old_data_if_available, new_data);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &new_data.id,
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(not(feature = "cache"))]
        FullEvent::GuildUpdate {
            old_data_if_available,
            new_data,
        } => {
            let log_data = (old_data_if_available, new_data);
            log_event!(
                log_unimplemented_event,
                guild_settings,
                event_in,
                &log_data,
                &new_data.id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::IntegrationCreate { integration } => {
            let log_data = integration;
            log_event!(
                log_integration_create,
                guild_settings,
                event_in,
                &log_data,
                &integration.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::IntegrationUpdate { integration } => {
            let log_data = integration;
            log_event!(
                log_integration_update,
                guild_settings,
                event_in,
                &log_data,
                &integration.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::IntegrationDelete {
            integration_id,
            guild_id,
            application_id,
        } => {
            let log_data = &(integration_id, guild_id, application_id);
            log_event!(
                log_integration_delete,
                guild_settings,
                event_in,
                &log_data,
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::InteractionCreate { interaction } => {
            let log_data = interaction;
            let guild_id =
                crate::utils::interaction_to_guild_id(interaction).unwrap_or(GuildId::new(1));
            log_event!(
                log_interaction_create,
                guild_settings,
                event_in,
                &log_data,
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::InviteCreate { data } => {
            let log_data = data;
            log_event!(
                log_invite_create,
                guild_settings,
                event_in,
                &log_data,
                &data.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::InviteDelete { data } => {
            log_event!(
                log_invite_delete,
                guild_settings,
                event_in,
                data,
                &data.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::MessageDelete {
            channel_id,
            deleted_message_id,
            guild_id,
        } => {
            let log_data = (channel_id, deleted_message_id, guild_id);
            log_event!(
                log_message_delete,
                guild_settings,
                event_in,
                &log_data,
                &guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::MessageDeleteBulk {
            channel_id,
            multiple_deleted_messages_ids,
            guild_id,
        } => {
            event_log
                .write_obj(&(channel_id, multiple_deleted_messages_ids, guild_id))
                .await
        },
        #[cfg(not(feature = "cache"))]
        FullEvent::MessageUpdate {
            old_if_available,
            new,
            event,
        } => {
            if new.as_ref().map(|x| x.author.bot).unwrap_or(false)
                || old_if_available
                    .as_ref()
                    .map(|x| x.author.bot)
                    .unwrap_or(false)
            {
                return Ok(());
            }
            let log_data: (
                &Option<serenity::model::prelude::Message>,
                &Option<serenity::model::prelude::Message>,
                &serenity::model::prelude::MessageUpdateEvent,
            ) = (old_if_available, new, event);
            log_event!(
                log_message_update,
                guild_settings,
                event_in,
                &log_data,
                &event.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        #[cfg(feature = "cache")]
        FullEvent::MessageUpdate {
            old_if_available,
            new,
            event,
        } => {
            if new.as_ref().map(|x| x.author.bot).unwrap_or(false)
                || old_if_available
                    .as_ref()
                    .map(|x| x.author.bot)
                    .unwrap_or(false)
            {
                return Ok(());
            }
            let log_data: (
                &Option<serenity::model::prelude::Message>,
                &Option<serenity::model::prelude::Message>,
                &serenity::model::prelude::MessageUpdateEvent,
            ) = (old_if_available, new, event);
            log_event!(
                log_message_update,
                guild_settings,
                event_in,
                &log_data,
                &event.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
            // event_log.write_log_obj_async(event_name, &(old_if_available, new, event))
        },
        FullEvent::ReactionAdd { add_reaction } => {
            log_event!(
                log_reaction_add,
                guild_settings,
                event_in,
                add_reaction,
                &add_reaction.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::ReactionRemove { removed_reaction } => {
            log_event!(
                log_reaction_remove,
                guild_settings,
                event_in,
                removed_reaction,
                &removed_reaction.guild_id.unwrap_or_default(),
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::ReactionRemoveAll {
            channel_id,
            removed_from_message_id,
        } => {
            event_log
                .write_log_obj_async(event_name, &(channel_id, removed_from_message_id))
                .await
        },
        FullEvent::Ready { data_about_bot } => {
            tracing::info!("{} is connected!", data_about_bot.user.name);
            event_log
                .write_log_obj_async(event_name, data_about_bot)
                .await
        },
        FullEvent::Resume { event } => event_log.write_log_obj_async(event_name, event).await,
        FullEvent::StageInstanceCreate { stage_instance } => {
            event_log
                .write_log_obj_async(event_name, stage_instance)
                .await
        },
        FullEvent::StageInstanceDelete { stage_instance } => {
            event_log
                .write_log_obj_async(event_name, stage_instance)
                .await
        },
        FullEvent::StageInstanceUpdate { stage_instance } => {
            event_log
                .write_log_obj_async(event_name, stage_instance)
                .await
        },
        FullEvent::ThreadCreate { thread } => {
            event_log.write_log_obj_async(event_name, thread).await
        },
        FullEvent::ThreadDelete {
            thread,

            full_thread_data: _,
        } => event_log.write_log_obj_async(event_name, thread).await,
        FullEvent::ThreadListSync { thread_list_sync } => {
            event_log
                .write_log_obj_async(event_name, thread_list_sync)
                .await
        },
        FullEvent::ThreadMemberUpdate { thread_member } => {
            event_log
                .write_log_obj_async(event_name, thread_member)
                .await
        },
        FullEvent::ThreadMembersUpdate {
            thread_members_update,
        } => {
            event_log
                .write_log_obj_async(event_name, thread_members_update)
                .await
        },
        FullEvent::ThreadUpdate { old, new } => {
            event_log.write_log_obj_async(event_name, &(old, new)).await
        },
        // FullEvent::Unknown { name, raw } => event_log.write_log_obj_async(event_name, &(name, raw)),
        FullEvent::UserUpdate { old_data, new } => {
            let log_data = (old_data, new);
            let guild_id = new.member.as_ref().unwrap().guild_id.unwrap();
            log_event!(
                log_user_update,
                guild_settings,
                event_in,
                &log_data,
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::VoiceServerUpdate { event } => {
            event_log.write_log_obj_async(event_name, event).await
        },
        FullEvent::VoiceChannelStatusUpdate {
            old,
            status,
            id,
            guild_id,
        } => {
            let log_data = (old, status, id, guild_id);
            log_event!(
                log_voice_channel_status_update,
                guild_settings,
                event_in,
                &log_data,
                &guild_id,
                &ctx,
                event_log,
                event_name
            )
        },
        FullEvent::WebhookUpdate {
            guild_id,
            belongs_to_channel_id,
        } => {
            event_log
                .write_obj(&(guild_id, belongs_to_channel_id))
                .await
        },
        FullEvent::CacheReady { guilds } => {
            tracing::info!(
                "{}: {}",
                event_in.snake_case_name().bright_green(),
                guilds.len()
            );
            Ok(())
        },
        _ => {
            tracing::info!("{}", event_in.snake_case_name().bright_green());
            Ok(())
        },
    }
}
