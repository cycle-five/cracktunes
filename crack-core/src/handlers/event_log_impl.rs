use super::serenity::voice_state_diff_str;
use crate::{
    errors::CrackedError, http_utils::get_guild_name, messaging::interface::send_log_embed_thumb,
    Error,
};
use colored::Colorize;
use serde::Serialize;
use serenity::{
    all::{
        ActionExecution, ApplicationId, CacheHttp, ChannelId, ClientStatus, CommandPermissions,
        Context as SerenityContext, CurrentUser, Guild, GuildChannel, GuildId,
        GuildScheduledEventUserAddEvent, GuildScheduledEventUserRemoveEvent, Integration,
        IntegrationId, Interaction, InviteCreateEvent, InviteDeleteEvent, Member, Message,
        MessageId, MessageUpdateEvent, Presence, Role, RoleId, ScheduledEvent, Sticker, StickerId,
        UnavailableGuild,
    },
    model::guild,
    small_fixed_array::FixedString,
};
use std::str::FromStr;
use std::{collections::HashMap, sync::Arc};

/// Catchall for logging events that are not implemented.
pub async fn log_unimplemented_event<T: Serialize + std::fmt::Debug>(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: T,
) -> Result<(), Error> {
    let guild_name = crate::http_utils::get_guild_name(http, channel_id, guild_id).await?;
    tracing::info!(
        "{}",
        format!(
            "Unimplemented Event: {}, {}, {:?}",
            guild_name, channel_id, log_data
        )
        .blue()
    );
    Ok(())
}

/// Log Integration Update Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_integration_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    cache_http: &impl CacheHttp,
    log_data: &Integration,
) -> Result<(), Error> {
    let integration = log_data.clone();
    let guild_id_int = integration.guild_id.unwrap_or_default();
    if guild_id_int != guild_id {
        tracing::warn!(
            "Integration Update Event: Guild ID mismatch: {} != {}",
            guild_id_int,
            guild_id,
        );
    }
    let title = format!("Integration Create Event {}", channel_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(cache_http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        cache_http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Integration Delete Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_integration_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    cache_http: &impl CacheHttp,
    log_data: &(&IntegrationId, &GuildId, &Option<ApplicationId>),
) -> Result<(), Error> {
    let &(integration_id, _guild_id, _application_id) = log_data;
    let title = format!(
        "Integration Delete Event {} {} {}",
        integration_id, guild_id, channel_id
    );
    let description = serde_json::to_string_pretty(log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(cache_http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        cache_http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Integration Create Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_integration_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    cache_http: &impl CacheHttp,
    log_data: &Integration,
) -> Result<(), Error> {
    let integration = log_data.clone();
    let guild_id = integration.guild_id.unwrap_or_default();
    let title = format!("Integration Create Event {}", channel_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(cache_http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        cache_http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Interaction Create Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_interaction_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    cache_http: &impl CacheHttp,
    log_data: &Interaction,
) -> Result<(), Error> {
    use crate::utils::interaction_to_guild_id;

    let interaction = log_data.clone();
    // let guild_id = invite_create_event.guild_id.unwrap_or_default();
    let guild_id = interaction_to_guild_id(&interaction).unwrap_or(GuildId::new(1));
    let title = format!("Interaction Create Event {}", channel_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(cache_http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        cache_http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Invite Delete Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_invite_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &InviteDeleteEvent,
) -> Result<(), Error> {
    let invite_create_event = log_data.clone();
    let guild_id = invite_create_event.guild_id.unwrap_or_default();
    let title = format!("Guild Invite Create {}", guild_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Invite Create Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_invite_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &InviteCreateEvent,
) -> Result<(), Error> {
    let invite_create_event = log_data.clone();
    let guild_id = invite_create_event.guild_id.unwrap_or_default();
    let title = format!("Guild Invite Create {}", guild_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log Guild Stickers Update Event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_stickers_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&GuildId, &HashMap<StickerId, Sticker>),
) -> Result<(), Error> {
    let (_guild_id, _stickers): (&GuildId, &HashMap<StickerId, Sticker>) = *log_data;
    let title = format!("Guild Stickers Update for guild {}", guild_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &guild_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild scheduled event create event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_scheduled_event_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &ScheduledEvent,
) -> Result<(), Error> {
    let event = log_data.clone();
    let title = format!(
        "Guild Scheduled Event Delete: {}",
        event.creator_id.unwrap_or_default()
    );
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .creator_id
        .unwrap_or_default()
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &event.creator_id.map(|x| x.to_string()).unwrap_or_default(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild scheduled user add event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_scheduled_event_user_add(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &GuildScheduledEventUserAddEvent,
) -> Result<(), Error> {
    let event = log_data.clone();
    let title = format!("Guild Scheduled Event User Add: {}", event.user_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .user_id
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &event.user_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild scheduled user remove event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_scheduled_event_user_remove(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &GuildScheduledEventUserRemoveEvent,
) -> Result<(), Error> {
    let event = log_data.clone();
    let title = format!("Guild Scheduled Event User Remove: {}", event.user_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .user_id
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &event.user_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild scheduled event create event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_scheduled_event_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &ScheduledEvent,
) -> Result<(), Error> {
    let event = log_data.clone();
    let title = format!(
        "Guild Scheduled Event Create: {}",
        event.creator_id.unwrap_or_default()
    );
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .creator_id
        .unwrap_or_default()
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &event.creator_id.map(|x| x.to_string()).unwrap_or_default(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Logs a guild create event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&Guild, &Option<bool>),
) -> Result<(), Error> {
    let &(guild, is_new) = log_data;
    let guild_name = crate::http_utils::get_guild_name(http, channel_id, guild_id).await?;

    // FIXME!
    // // make sure we have the guild stored or store it
    // if guild_settings_map.read().await.get(&guild_id).is_none() {
    //     let new_settings =
    //         GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), Some(guild_name.clone()));
    //     guild_settings_map
    //         .write()
    //         .await
    //         .insert(guild_id, new_settings.clone());
    // }

    let title = format!("Guild Create: {}", guild.name);
    let is_new_str = if !is_new.is_some() || !is_new.unwrap() {
        "not "
    } else {
        ""
    };
    let description = format!("Guild is {}new", is_new_str);
    let id = guild.id.to_string();
    let avatar_url = "";
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &id,
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Logs a guild delete event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_delete_event(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&UnavailableGuild, &Option<Guild>),
) -> Result<(), Error> {
    let &(unavailable, full) = log_data;
    let guild_name = crate::http_utils::get_guild_name(http, channel_id, guild_id).await?;

    // FIXME!
    // // make sure we have the guild stored or store it
    // if guild_settings_map.read().await.get(&guild_id).is_none() {
    //     let new_settings =
    //         GuildSettings::new(guild_id, Some(DEFAULT_PREFIX), Some(guild_name.clone()));
    //     guild_settings_map
    //         .write()
    //         .await
    //         .insert(guild_id, new_settings.clone());
    // }

    let title = format!("Guild Delete: {}", guild_name);
    let mut description = if !unavailable.unavailable {
        "Bot was removed from the guild."
    } else {
        "Guild was deleted."
    }
    .to_string();

    if full.is_some() {
        let guild: &Guild = full.as_ref().unwrap();
        description = format!(
            "Guild was deleted.\nGuild Name: {}\nGuild ID: {}\nGuild Owner: {}",
            guild.name, guild.id, guild.owner_id
        );
    }
    let id = unavailable.id.to_string();
    let avatar_url = "";
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &id,
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Logs a guild role cteate event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_role_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &serenity::model::prelude::Role,
) -> Result<(), Error> {
    let guild_name = crate::http_utils::get_guild_name(http, channel_id, guild_id).await?;
    let title = format!("Role Created: {}", log_data.name);
    let description = guild_role_to_string(log_data);
    let avatar_url = "";
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &log_data.id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Logs a guild role delete.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_role_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&GuildId, &RoleId, &Option<Role>),
) -> Result<(), Error> {
    let (&guild_id, &role_id, role) = log_data;
    let guild_name = crate::http_utils::get_guild_name(http, channel_id, guild_id).await?;
    let default_role = Role::default();
    let role = role.as_ref().unwrap_or(&default_role);
    let title = format!("Role Deleted: {}", role.name);
    let description = guild_role_to_string(role);
    let avatar_url = "";
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &role_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log an automod rule update event
#[cfg(not(tarpaulin_include))]
pub async fn log_automod_rule_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(String, Rule),
) -> Result<(), Error> {
    let (_event_name, log_data) = log_data.clone();
    let title = format!("Automod Rule Update: {}", log_data.creator_id);
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .creator_id
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &log_data.creator_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild scheduled event create event.
#[cfg(not(tarpaulin_include))]
pub async fn log_guild_scheduled_event_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &ScheduledEvent,
) -> Result<(), Error> {
    let event = log_data.clone();
    let title = format!(
        "Guild Scheduled Event Create: {}",
        event.creator_id.unwrap_or_default()
    );
    let description = serde_json::to_string_pretty(&log_data).unwrap_or_default();
    let avatar_url = log_data
        .creator_id
        .unwrap_or_default()
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &event.creator_id.map(|x| x.to_string()).unwrap_or_default(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

use serenity::model::guild::automod::Rule;
/// Log a automod rule create event
#[cfg(not(tarpaulin_include))]
pub async fn log_automod_rule_create(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &Rule,
) -> Result<(), Error> {
    let title = format!("Automod Rule Create: {}", log_data.creator_id);
    let description = serde_json::to_string_pretty(log_data).unwrap_or_default();
    let avatar_url = log_data
        .creator_id
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &log_data.creator_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a automod command exec
#[cfg(not(tarpaulin_include))]
pub async fn log_automod_command_execution(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &ActionExecution,
) -> Result<(), Error> {
    let title = format!("Automod Action Executed: {}", log_data.rule_id);
    let description = serde_json::to_string_pretty(log_data).unwrap_or_default();
    let avatar_url = log_data
        .user_id
        .to_user(http)
        .await
        .unwrap_or_default()
        .avatar_url()
        .unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &log_data.user_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a command permissions update event
#[cfg(not(tarpaulin_include))]
pub async fn log_command_permissions_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &CommandPermissions,
) -> Result<(), Error> {
    let permissions = log_data;

    let title = format!("Command Permissions Updated: {}", permissions.id);
    let description = format!("Permissions: {:?}", permissions.permissions);
    let avatar_url = "";

    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &permissions.id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

pub async fn log_channel_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&GuildChannel, &Option<VecDequeue<Message>>),
) -> Result<(), Error> {
    let &(guild_channel, messages) = log_data;
    let del_channel_id = guild_channel.id;
    let title = format!("Channel Deleted: {}", del_channel_id);
    let description = format!(
        "messages deleted: {}",
        messages.as_ref().map(|x| x.len()).unwrap_or_default()
    );
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &channel_id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

pub async fn log_message_delete(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&ChannelId, &MessageId, &Option<GuildId>),
) -> Result<(), Error> {
    let &(del_channel_id, message_id, guild_id) = log_data;
    let id = message_id.to_string();
    let title = format!("Message Deleted: {}", id);
    let guild_id = guild_id.unwrap_or_default();
    let description = format!("ChannelId: {}\nGuildId: {}", del_channel_id, guild_id);
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &id,
        &title,
        &description,
        avatar_url,
    )
    .await
    .map(|_| ())
}

pub async fn log_user_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&Option<CurrentUser>, &CurrentUser),
) -> Result<Message, Error> {
    let &(old, new) = log_data;
    let title = format!("User Updated: {}", new.name);
    let description = format!(
            "Old User: {}\nNew User: {}",
            old.clone().map(|x| x.name.clone()).unwrap_or_else(
                || FixedString::from_str("None").expect("Failed to create FixedString")
            ),
            new.name
        );

    let name = new.name.clone();
    let avatar_url = new.avatar_url().unwrap_or_default();
    let old_avatar_url = old
        .as_ref()
        .and_then(|x| x.avatar_url())
        .unwrap_or_default();

    let description = if avatar_url != old_avatar_url {
        format!("Avatar Updated: {}", name)
    } else {
        description
    };
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;

    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &new.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

pub async fn log_reaction_remove(
    channel_id_first: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &serenity::model::prelude::Reaction,
) -> Result<(), Error> {
    let reaction = log_data;
    let member = reaction.member.clone().unwrap_or_default();
    let message_id = reaction.message_id;
    let channel_id = reaction.channel_id;
    let guild_id = reaction.guild_id.unwrap_or_default();
    let title = format!("Reaction Removed: {}", reaction.emoji);
    let description = format!(
        "Channel: {}\nMessage: {}\nEmoji: {}, Member: {}",
        channel_id, message_id, reaction.emoji, member.user.name
    );
    let avatar_url = member.avatar_url().unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id_first,
        http,
        &member.user.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

pub async fn log_reaction_add(
    channel_id_first: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &serenity::model::prelude::Reaction,
) -> Result<(), Error> {
    let reaction = log_data;
    let member = reaction.member.clone().unwrap_or_default();
    let message_id = reaction.message_id;
    let channel_id = reaction.channel_id;
    let guild_id = reaction.guild_id.unwrap_or_default();
    let title = format!("Reaction Added: {}", reaction.emoji);
    let description = format!(
        "Channel: {}\nMessage: {}\nEmoji: {}, Member: {}",
        channel_id, message_id, reaction.emoji, member.user.name
    );
    let avatar_url = member.avatar_url().unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id_first,
        http,
        &member.user.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a message update event.
pub async fn log_message_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(
        &Option<serenity::model::prelude::Message>,
        &Option<serenity::model::prelude::Message>,
        &MessageUpdateEvent,
    ),
) -> Result<(), Error> {
    // Don't log message updates from bots
    // TODO: Make this configurable
    if log_data.2.author.as_ref().map(|x| x.bot()).unwrap_or(false) {
        return Ok(());
    }

    let (id, title, description, avatar_url) = if let &(Some(old), Some(new), _msg) = log_data {
        let title = format!("Message Updated: {}", new.author.name);
        let description = format!(
            "User: {}\nID: {}\nChannel: {}\nOld Message: {}\nNew Message: {}",
            new.author.name, new.author.id, new.channel_id, old.content, new.content
        );
        let avatar_url = new.author.avatar_url().unwrap_or_default();
        let id = new.author.id.to_string();
        (id, title, description, avatar_url)
    } else if let &(None, Some(new), _msg) = log_data {
        let title = format!("Message Updated: {}", new.author.name);
        let description = format!(
            "User: {}\nID: {}\nChannel: {}\nOld Message: None\nNew Message: {}",
            new.author.name, new.author.id, new.channel_id, new.content
        );
        let avatar_url = new.author.avatar_url().unwrap_or_default();
        let id = new.author.id.to_string();
        (id, title, description, avatar_url)
    } else if let &(Some(old), None, _msg) = log_data {
        let title = format!("Message Updated: {}", old.author.name);
        let description = format!(
            "User: {}\nID: {}\nChannel: {}\nOld Message: {}\nNew Message: None",
            old.author.name, old.author.id, old.channel_id, old.content
        );
        let avatar_url = old.author.avatar_url().unwrap_or_default();
        let id = old.author.id.to_string();
        (id, title, description, avatar_url)
    } else {
        let &(_, _, msg) = log_data;
        if let Some(author) = &msg.author {
            let title = format!("Message Updated: {}", author.name);
            let description = format!(
                "User: {}\nID: {}\nChannel: {}\nOld Message: None\nNew Message: None",
                author.name, author.id, channel_id
            );
            let avatar_url = author.avatar_url().unwrap_or_default();
            let id = author.id.to_string();
            (id, title, description, avatar_url)
        } else {
            default_msg_string(msg)
        }
    };
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &id,
        &title,
        &description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

pub fn default_msg_string(msg: &MessageUpdateEvent) -> (String, String, String, String) {
    let title = "Message Updated".to_string();
    let description = msg.id.to_string();
    let avatar_url = "".to_string();
    let id = "".to_string();
    (id, title, description, avatar_url)
}

/// Log a guild ban.
pub async fn log_guild_ban_addition<T: Serialize + std::fmt::Debug>(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&str, &GuildId, &serenity::model::prelude::User),
) -> Result<(), Error> {
    let &(_event_name, _guild_id, user) = log_data;
    let title = format!("Member Banned: {}", user.name);
    // let description = format!("User: {}\nID: {}", user.name, user.id);
    let description = "";
    let avatar_url = user.avatar_url().unwrap_or_default();

    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &user.id.to_string(),
        &title,
        description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Log a guild ban removal
pub async fn log_guild_ban_removal<T: Serialize + std::fmt::Debug>(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&str, &GuildId, &serenity::model::prelude::User),
) -> Result<(), Error> {
    let &(_event, _guild_id, user) = log_data;
    let title = format!("Member Unbanned: {}", user.name);
    // let description = format!("User: {}\nID: {}", user.name, user.id);
    let description = "";
    let avatar_url = user.avatar_url().unwrap_or_default();

    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &user.id.to_string(),
        &title,
        description,
        &avatar_url,
    )
    .await
    .map(|_| ())
}

/// Guild Role to a string.
pub fn guild_role_to_string(role: &serenity::model::prelude::Role) -> String {
    format!(
        "Role: {}\nID: {}\nColor: {:#?}\nHoist: {}\nMentionable: {}\nPermissions: {:?}\nPosition: {}\n",
        role.name,
        role.id,
        role.colour,
        role.hoist(),
        role.mentionable(),
        role.permissions,
        role.position,
    )
}

/// Diff two guild roles.
pub fn guild_role_diff(
    old: &serenity::model::prelude::Role,
    new: &serenity::model::prelude::Role,
) -> String {
    let mut diff_str = String::new();
    if old.name != new.name {
        diff_str.push_str(&format!("Name: {} -> {}\n", old.name, new.name));
    }
    if old.colour != new.colour {
        diff_str.push_str(&format!("Color: {:#?} -> {:#?}\n", old.colour, new.colour));
    }
    if old.hoist() != new.hoist() {
        diff_str.push_str(&format!("Hoist: {} -> {}\n", old.hoist(), new.hoist()));
    }
    if old.mentionable() != new.mentionable() {
        diff_str.push_str(&format!(
            "Mentionable: {} -> {}\n",
            old.mentionable(),
            new.mentionable()
        ));
    }
    if old.permissions != new.permissions {
        diff_str.push_str(&format!(
            "Permissions: {:?} -> {:?}\n",
            old.permissions, new.permissions
        ));
    }
    if old.position != new.position {
        diff_str.push_str(&format!("Position: {} -> {}\n", old.position, new.position));
    }
    diff_str
}

/// Log a guild role update event.
pub async fn log_guild_role_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(
        &Option<serenity::model::prelude::Role>,
        &serenity::model::prelude::Role,
    ),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(old, new) = log_data;
    let title = format!("Role Updated: {}", new.name);
    let description = old
        .as_ref()
        .map(|r| guild_role_diff(r, new))
        .unwrap_or_else(|| guild_role_to_string(new));
    // FIXME: Use icon or emoji
    let avatar_url = "";
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &new.id.to_string(),
        &title,
        &description,
        avatar_url,
    )
    .await
}

/// Log a guild role creation event.
pub async fn log_guild_member_removal(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    log_data: &(&GuildId, &serenity::model::prelude::User, &Option<Member>),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(_guild_id, user, member_data_if_available) = log_data;
    let title = format!("Member Left: {}", user.name);
    let description = format!(
        "Account Created: {}\nJoined: {:?}",
        user.id.created_at(),
        member_data_if_available.clone().and_then(|m| m.joined_at)
    );
    let avatar_url = user.avatar_url().unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &user.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

/// Log a guild member addition event.
pub async fn log_guild_member_addition(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    new_member: &Member,
) -> Result<serenity::model::prelude::Message, Error> {
    let avatar_url = new_member.user.avatar_url().unwrap_or_default();
    let title = format!("Member Joined: {}", new_member.user.name);
    let description = format!(
        "Account Created: {}\nJoined: {:?}",
        new_member.user.id.created_at(),
        new_member.joined_at
    );
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &new_member.user.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

/// Harness struct for printing a presence.
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
                self.user.bot(),
                self.user.mfa_enabled(),
                self.user.verified(),
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

/// Log a presence update event.
#[cfg(not(tarpaulin_include))]
pub async fn log_presence_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    new_data: &Presence,
) -> Result<serenity::model::prelude::Message, Error> {
    let presence_str = PresencePrinter {
        presence: Some(new_data.clone()),
    }
    .to_string();

    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    let title = format!(
        "Presence Update: {}",
        new_data.user.name.clone().unwrap_or_default()
    );
    let user_id = new_data.user.id;
    let description = presence_str;
    let avatar_url = format!(
        "https://cdn.discordapp.com/{user_id}/{user_avatar}.png",
        user_id = user_id,
        user_avatar = new_data
            .user
            .avatar
            .map(|x| x.to_string())
            .unwrap_or_default(),
    );
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &user_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

/// Log a voice state update event.
#[cfg(not(tarpaulin_include))]
pub async fn log_voice_channel_status_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    ctx: &SerenityContext,
    log_data: &(&Option<String>, &Option<String>, &ChannelId, &GuildId),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(old, status, _, _) = log_data;
    let title = format!("Voice Channel Status Update: {:?} -> {:?}", old, status);

    let description = "";
    let avatar_url = "";
    let guild_name = get_guild_name(&ctx, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        &ctx,
        &channel_id.to_string(),
        &title,
        description,
        avatar_url,
    )
    .await
}

/// Log a voice state update event.
#[cfg(not(tarpaulin_include))]
pub async fn log_voice_state_update(
    channel_id: ChannelId,
    guild_id: GuildId,
    ctx: &SerenityContext,
    log_data: &(
        &Option<serenity::model::prelude::VoiceState>,
        &serenity::model::prelude::VoiceState,
    ),
) -> Result<serenity::model::prelude::Message, Error> {
    let &(old, new) = log_data;
    let title = format!(
        "Voice State Update: {}",
        new.user_id.to_user(ctx).await?.name
    );
    let description = voice_state_diff_str(old, new, Arc::new(ctx)).await?;

    let avatar_url = new
        .member
        .clone()
        .and_then(|x| x.user.avatar_url())
        .unwrap_or_default();
    let guild_name = get_guild_name(&ctx, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        &ctx,
        &new.user_id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

/// Noop log a typing start event.
pub async fn log_typing_start_noop(
    _channel_id: ChannelId,
    _guild_id: GuildId,
    _http: &impl CacheHttp,
    _event: &serenity::model::prelude::TypingStartEvent,
) -> Result<serenity::model::prelude::Message, Error> {
    Ok(serenity::model::prelude::Message::default())
}

/// Log a typing start event.
pub async fn log_typing_start(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    event: &serenity::model::prelude::TypingStartEvent,
) -> Result<serenity::model::prelude::Message, Error> {
    let guild_id = event.guild_id; //.ok_or(CrackedError::NoGuildId)?;
    let guild_id_no_opt = guild_id.ok_or(CrackedError::NoGuildId)?;
    let user = event.user_id.to_user(http).await?;
    let channel_name = channel_id.to_channel(http, guild_id).await?.to_string();
    let name = user.name.clone();
    let guild = event
        .guild_id
        .unwrap_or_default()
        .to_partial_guild(http)
        .await?
        .name;
    tracing::info!(
        "{}{} / {} / {} / {}",
        "TypingStart: ".bright_green(),
        name.bright_yellow(),
        user.id.to_string().bright_yellow(),
        channel_name.bright_yellow(),
        guild.bright_yellow(),
    );
    let title = format!("Typing Start: {}", event.user_id);
    let description = format!(
        "User: {}\nID: {}\nChannel: {}",
        name, event.user_id, event.channel_id
    );
    let avatar_url = user.avatar_url().unwrap_or_default();
    let guild_id = event.guild_id.unwrap_or_default();
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &user.id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

pub async fn log_message(
    channel_id: ChannelId,
    guild_id: GuildId,
    http: &impl CacheHttp,
    new_message: &serenity::model::prelude::Message,
) -> Result<serenity::model::prelude::Message, Error> {
    let guild_name = get_guild_name(http, channel_id, guild_id).await?;
    let title = format!("Message: {}", new_message.author.name);
    let id = new_message.author.id;
    let description = format!(
        "User: {}\nID: {}\nChannel: {}\nMessage: {}",
        new_message.author.name, id, new_message.channel_id, new_message.content
    );
    let avatar_url = new_message.author.avatar_url().unwrap_or_default();
    send_log_embed_thumb(
        &guild_name,
        &channel_id,
        http,
        &id.to_string(),
        &title,
        &description,
        &avatar_url,
    )
    .await
}

#[macro_export]
macro_rules! log_event {
    ($log_func:expr, $guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $http:expr, $event_log:expr, $event_name:expr) => {{
        $event_log
            .write_log_obj_async($event_name, $log_data)
            .await?;
        let channel_id = get_channel_id($guild_settings, $guild_id, $event).await?;
        $log_func(channel_id, $guild_id, $http, $log_data)
            .await
            .map(|_| ())
    }};
}

#[macro_export]
macro_rules! log_event2 {
    ($log_func:expr, $guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $ctx:expr, $event_log:expr, $event_name:expr) => {{
        $event_log
            .write_log_obj_async($event_name, $log_data)
            .await?;
        let channel_id = get_channel_id($guild_settings, $guild_id, $event).await?;
        $log_func(channel_id, $guild_id, $ctx, $log_data)
            .await
            .map(|_| ())
    }};
}

#[macro_export]
macro_rules! log_event_async {
    ($log_func:expr, $guild_settings:expr, $event:expr, $log_data:expr, $guild_id:expr, $http:expr, $event_log:expr, $event_name:expr) => {{
        $event_log
            .write_log_obj_async($event_name, $log_data)
            .await?;
        let channel_id = get_channel_id($guild_settings, $guild_id, $event).await?;
        $log_func(channel_id, $guild_id, $http, $log_data)
            .await
            .map(|_| ())
    }};
}
